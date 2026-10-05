//! 历史页状态:视图形态、筛选、查询结果缓存与按需重载(防抖门控)。

use std::time::{Duration, Instant};

use super::filter::{AggregateSort, Filter, SummarySort};
use super::fmt::{local_tz_offset_secs, month_start, parse_ip_prefix};
use super::query::{query_aggregate, query_detail, query_summary, query_usage};
use crate::model::Protocol;
use crate::storage::history::{Db, Writer, db_size, unix_now};

/// 筛选输入防抖时长:连续按键合并为最后一次重查
pub(super) const FILTER_DEBOUNCE: Duration = Duration::from_millis(300);

/// 历史页视图形态
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Detail,
    Aggregate,
    Summary,
    Usage,
}

/// 时间范围档位
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Range {
    LastHour,
    Last6Hours,
    Last24Hours,
    Last7Days,
    ThisMonth,
}

impl Range {
    /// 档位对应的时间窗起点(unix 秒);本月取本地时区月初零点
    pub fn start(&self, now: u64) -> u64 {
        match self {
            Range::LastHour => now.saturating_sub(3600),
            Range::Last6Hours => now.saturating_sub(6 * 3600),
            Range::Last24Hours => now.saturating_sub(24 * 3600),
            Range::Last7Days => now.saturating_sub(7 * 86400),
            Range::ThisMonth => month_start(),
        }
    }
}

/// 查询结果(与视图形态对应)
pub enum Rows {
    Detail(Vec<super::filter::DetailRow>),
    Aggregate(Vec<super::filter::AggregateRow>),
    Summary(Vec<super::filter::SummaryRow>),
    Usage(Vec<super::filter::UsageRow>),
}

/// 历史页状态:视图、筛选、结果与维护操作
pub struct PageState {
    pub view: ViewMode,
    pub range: Range,
    /// 进程名筛选(模糊)
    pub process: String,
    /// 远端 IP 前缀筛选文本
    pub remote: String,
    pub proto: Option<Protocol>,
    /// 聚合视图排序键与方向(默认最近活动降序)
    pub aggregate_sort: (AggregateSort, bool),
    /// 汇总视图排序键与方向(默认上传总量降序)
    pub summary_sort: (SummarySort, bool),
    /// 用量视图分桶宽(秒):86400 = 按天,3600 = 按小时
    pub usage_bucket: u64,
    pub rows: Rows,
    /// 结果或库大小需要重新加载
    pub dirty: bool,
    pub db_size: u64,
    /// 右键发起的待确认批量删除(确认弹窗,同帧或次帧内决出)
    pub pending_delete: Option<super::query::PendingDelete>,
    /// 已发起清理,延迟数帧后刷新(等写线程完成)
    purge_pending: Option<Instant>,
    /// 筛选输入防抖:输入每键一次全量 SQL 查询在大库上卡顿,记下
    /// 最后按键时刻,300ms 无后续输入才真正重查
    filter_debounce: Option<Instant>,
    /// 汇总视图"活跃合并"缓存:(生成时刻, 并入活跃连接后的行)。
    /// SQL 结果克隆+活跃聚合+重排每帧执行在交互帧重复,按秒失效复用;
    /// 重查后由 refresh_if_needed 置 None
    pub summary_merged: Option<(Instant, Vec<super::filter::SummaryRow>)>,
}

impl PageState {
    pub fn new() -> Self {
        PageState {
            view: ViewMode::Detail,
            range: Range::Last24Hours,
            process: String::new(),
            remote: String::new(),
            proto: None,
            aggregate_sort: (AggregateSort::LastActive, false),
            summary_sort: (SummarySort::BytesOut, false),
            usage_bucket: 86400,
            rows: Rows::Detail(Vec::new()),
            dirty: true,
            db_size: 0,
            pending_delete: None,
            purge_pending: None,
            filter_debounce: None,
            summary_merged: None,
        }
    }

    /// 筛选文本输入中:延迟 FILTER_DEBOUNCE 再重查(合并连续按键)
    pub fn defer_refresh(&mut self) {
        self.filter_debounce = Some(Instant::now());
    }

    /// 发起清理:删除 days 天前的数据(0 = 清空全部);
    /// 还原超容提醒由调用方处理(config)
    pub fn request_purge(&mut self, writer: &Writer, days: u32) {
        writer.purge(days);
        self.purge_pending = Some(Instant::now());
    }

    /// 按需重新加载:dirty(进入页面/筛选变化)或清空完成后;
    /// 本地/局域网过滤取自 config(两页共享,唯一来源)
    pub fn refresh_if_needed(&mut self, db: &Db, hide_local: bool, hide_lan: bool) {
        if let Some(at) = self.purge_pending
            && at.elapsed() >= Duration::from_millis(300)
        {
            self.dirty = true;
            self.purge_pending = None;
        }
        // 筛选输入防抖:静默期到达后合并为一次重查
        if let Some(at) = self.filter_debounce {
            if at.elapsed() >= FILTER_DEBOUNCE {
                self.dirty = true;
                self.filter_debounce = None;
            } else {
                return;
            }
        }
        if !self.dirty {
            return;
        }
        let filter = Filter {
            start: self.range.start(unix_now()),
            process: self.process.trim().to_owned(),
            remote: parse_ip_prefix(&self.remote),
            proto: self.proto,
            hide_local,
            hide_lan,
        };
        self.rows = match self.view {
            ViewMode::Detail => Rows::Detail(query_or_warn("明细", query_detail(db, &filter))),
            ViewMode::Aggregate => {
                let (sort, asc) = self.aggregate_sort;
                Rows::Aggregate(query_or_warn(
                    "聚合",
                    query_aggregate(db, &filter, sort, asc),
                ))
            }
            ViewMode::Summary => {
                let (sort, asc) = self.summary_sort;
                Rows::Summary(query_or_warn("汇总", query_summary(db, &filter, sort, asc)))
            }
            ViewMode::Usage => Rows::Usage(query_or_warn(
                "用量",
                query_usage(db, &filter, self.usage_bucket, local_tz_offset_secs()),
            )),
        };
        self.db_size = db_size();
        self.summary_merged = None;
        self.dirty = false;
    }
}

/// 查询失败告警后降级为空结果:库损坏/磁盘满等不应静默显示为"无数据"
fn query_or_warn<T>(what: &str, r: rusqlite::Result<Vec<T>>) -> Vec<T> {
    match r {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("[History] {what}查询失败: {e}");
            Vec::new()
        }
    }
}
