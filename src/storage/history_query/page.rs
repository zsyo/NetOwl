//! 历史页状态:视图形态、筛选、查询结果缓存与按需重载判定(防抖门控)。
//! 大库聚合查询秒级,SQL 执行在 app 层派发的工作线程完成(run_query),
//! 本模块只负责判定何时需要查询(due_for_query)与结果缓存。

use std::time::{Duration, Instant};

use super::filter::parse_remote_filter;
use super::filter::{AggregateSort, Filter, SummarySort};
use super::fmt::{local_tz_offset_secs, month_start};
use super::query::{query_aggregate, query_detail, query_summary, query_usage};
use crate::model::Protocol;
use crate::storage::history::{Db, Writer, unix_now};

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

impl ViewMode {
    /// 日志用中文名(与查询告警口径一致)
    pub fn label(&self) -> &'static str {
        match self {
            ViewMode::Detail => "明细",
            ViewMode::Aggregate => "聚合",
            ViewMode::Summary => "汇总",
            ViewMode::Usage => "用量",
        }
    }
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

    /// 日志用档位名
    pub fn label(&self) -> &'static str {
        match self {
            Range::LastHour => "近1小时",
            Range::Last6Hours => "近6小时",
            Range::Last24Hours => "近24小时",
            Range::Last7Days => "近7天",
            Range::ThisMonth => "本月",
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

impl Rows {
    /// 指定视图形态的空结果:派发查询前占位,保证与 view 对齐,
    /// 旧视图数据不再闪现
    pub fn empty(view: ViewMode) -> Self {
        match view {
            ViewMode::Detail => Rows::Detail(Vec::new()),
            ViewMode::Aggregate => Rows::Aggregate(Vec::new()),
            ViewMode::Summary => Rows::Summary(Vec::new()),
            ViewMode::Usage => Rows::Usage(Vec::new()),
        }
    }

    /// 行数(加载态判定与查询耗时日志)
    pub fn len(&self) -> usize {
        match self {
            Rows::Detail(r) => r.len(),
            Rows::Aggregate(r) => r.len(),
            Rows::Summary(r) => r.len(),
            Rows::Usage(r) => r.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 一次历史页查询的参数快照:派发时从 PageState 收集,move 进工作线程执行
pub struct QuerySpec {
    pub filter: Filter,
    pub view: ViewMode,
    /// 时间范围档位(日志展示;起点已固化进 filter.start)
    pub range: Range,
    /// 聚合视图排序键与方向
    pub aggregate_sort: (AggregateSort, bool),
    /// 汇总视图排序键与方向
    pub summary_sort: (SummarySort, bool),
    /// 用量视图分桶宽(秒)
    pub usage_bucket: u64,
    /// 本地时区偏移秒(用量分桶)
    pub tz_off: i64,
}

impl QuerySpec {
    /// 查询耗时日志的参数描述(单行,逗号分隔;筛选项仅列出激活的)
    pub fn describe(&self) -> String {
        let mut s = format!(
            "窗口={}({} 起)",
            self.range.label(),
            super::fmt::fmt_local(self.filter.start)
        );
        match self.view {
            ViewMode::Aggregate => {
                let (k, asc) = self.aggregate_sort;
                s += &format!(", 排序={} {}", k.label(), dir_label(asc));
            }
            ViewMode::Summary => {
                let (k, asc) = self.summary_sort;
                s += &format!(", 排序={} {}", k.label(), dir_label(asc));
            }
            ViewMode::Usage => {
                let bucket = if self.usage_bucket == 86400 {
                    "按天".to_owned()
                } else if self.usage_bucket == 3600 {
                    "按小时".to_owned()
                } else {
                    format!("按{}秒", self.usage_bucket)
                };
                s += &format!(", 桶={bucket}");
            }
            ViewMode::Detail => {}
        }
        let f = &self.filter;
        let mut items: Vec<String> = Vec::new();
        if !f.process.is_empty() {
            items.push(format!("进程={}", f.process));
        }
        if let Some(r) = &f.remote {
            items.push(format!("远端={}", remote_desc(r)));
        }
        if let Some(p) = f.proto {
            items.push(format!("协议={}", p.as_str()));
        }
        // 与噪音过滤 UI 文案同口径(勾选=包含):排除时打"不含"
        if f.hide_local {
            items.push("不含本地流量".to_owned());
        }
        if f.hide_lan {
            items.push("不含局域网流量".to_owned());
        }
        if !items.is_empty() {
            s += &format!(", 筛选[{}]", items.join(" "));
        }
        s
    }
}

/// 排序方向日志标签
fn dir_label(asc: bool) -> &'static str {
    if asc { "升序" } else { "降序" }
}

/// 远端筛选区间的日志展示:单 IP 只显一段,区间显 lo~hi
fn remote_desc(r: &super::filter::RemoteFilter) -> String {
    use std::net::{Ipv4Addr, Ipv6Addr};
    match r {
        super::filter::RemoteFilter::V4(lo, hi) if lo == hi => Ipv4Addr::from(*lo).to_string(),
        super::filter::RemoteFilter::V4(lo, hi) => {
            format!("{}~{}", Ipv4Addr::from(*lo), Ipv4Addr::from(*hi))
        }
        super::filter::RemoteFilter::V6(lo, hi) if lo == hi => Ipv6Addr::from(*lo).to_string(),
        super::filter::RemoteFilter::V6(lo, hi) => {
            format!("{}~{}", Ipv6Addr::from(*lo), Ipv6Addr::from(*hi))
        }
    }
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
    /// 查询在途(工作线程执行中):表格区域显示加载动画,不渲染旧数据
    pub loading: bool,
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
            loading: false,
            db_size: 0,
            pending_delete: None,
            purge_pending: None,
            filter_debounce: None,
            summary_merged: None,
        }
    }

    /// 外部跳转设置进程筛选并立即重查(清防抖,跨页跳转不经键盘输入)
    pub fn set_process_filter(&mut self, process: String) {
        self.process = process;
        self.refresh_now();
    }

    /// 离散操作(视图/排序/档位/开关/手动刷新)即时生效:置 dirty 并取消
    /// 筛选防抖。防抖只为合并连续键盘输入,不该让离散操作也等 300ms
    /// (防抖期内切视图会短暂渲染上一个视图的数据)
    pub fn refresh_now(&mut self) {
        self.dirty = true;
        self.filter_debounce = None;
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

    /// 按需重载判定:dirty(进入页面/筛选变化)或清理完成后到期,返回
    /// 本次查询的参数快照(调用方派发工作线程执行);本地/局域网过滤
    /// 取自 config(两页共享,唯一来源)
    pub fn due_for_query(&mut self, hide_local: bool, hide_lan: bool) -> Option<QuerySpec> {
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
                return None;
            }
        }
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        // 筛选按视图裁剪(与工具栏控件裁剪同步):汇总页只应用进程与
        // 协议筛选,用量页仅协议——控件不可见时筛选仍生效会变成看不见
        // 的隐藏条件;明细/聚合页全量应用
        let (process, remote, proto) = match self.view {
            ViewMode::Summary => (self.process.trim().to_owned(), None, self.proto),
            ViewMode::Usage => (String::new(), None, self.proto),
            _ => (
                self.process.trim().to_owned(),
                parse_remote_filter(&self.remote),
                self.proto,
            ),
        };
        Some(QuerySpec {
            filter: Filter {
                start: self.range.start(unix_now()),
                process,
                remote,
                proto,
                hide_local,
                hide_lan,
            },
            view: self.view,
            range: self.range,
            aggregate_sort: self.aggregate_sort,
            summary_sort: self.summary_sort,
            usage_bucket: self.usage_bucket,
            tz_off: local_tz_offset_secs(),
        })
    }
}

/// 执行一次查询(工作线程,独立连接):按视图形态分派对应 SQL。
/// 汇总/用量优先尝试统计表快路径(覆盖天已物化且筛选可下推),
/// 未命中回退明细全扫。查询失败告警后降级为空结果:库损坏/磁盘满等
/// 不应静默显示为"无数据"
pub fn run_query(db: &Db, spec: &QuerySpec) -> Rows {
    let f = &spec.filter;
    match spec.view {
        ViewMode::Detail => Rows::Detail(query_or_warn("明细", query_detail(db, f))),
        ViewMode::Aggregate => {
            let (sort, asc) = spec.aggregate_sort;
            Rows::Aggregate(query_or_warn("聚合", query_aggregate(db, f, sort, asc)))
        }
        ViewMode::Summary => {
            let (sort, asc) = spec.summary_sort;
            let rows = super::stats_query::try_query_summary_stats(db, f, sort, asc)
                .unwrap_or_else(|| query_or_warn("汇总", query_summary(db, f, sort, asc)));
            Rows::Summary(rows)
        }
        ViewMode::Usage => {
            let rows =
                super::stats_query::try_query_usage_stats(db, f, spec.usage_bucket, spec.tz_off)
                    .unwrap_or_else(|| {
                        query_or_warn("用量", query_usage(db, f, spec.usage_bucket, spec.tz_off))
                    });
            Rows::Usage(rows)
        }
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
