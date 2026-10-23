//! 轮询编排的外围任务:悬浮球数据快照、用量字节查询(悬浮球与配额
//! 共用,工作线程)、用量配额告警、历史页查询派发与收割、检查更新
//! 结果回收。与连接采集无耦合,各自按节流独立运行(悬浮球 1s /
//! 用量字节分钟级 / 历史查询与更新事件驱动);采集主流程见 super::poll。

use std::time::{Duration, Instant};

use eframe::egui;

use super::{DB_BYTES_INTERVAL, NetOwlApp};
use crate::storage::history_query;
use crate::ui::floating_ball;

impl NetOwlApp {
    /// 悬浮球数据快照:总速率与进程速率榜 1s 节流重建(与连接采集同频);
    /// 仅开关开启时计算。今日总量 = 落库当日收发(分钟级异步查库)+
    /// 活跃连接实时累计(未落库部分,每秒叠加)
    pub(super) fn poll_ball_data(&mut self) {
        if !self.config.floating_ball.enabled
            || self.ball_data_at.elapsed() < Duration::from_secs(1)
        {
            return;
        }
        self.ball_data_at = Instant::now();
        let (up_top, down_top) = floating_ball::collect_proc_rates(
            &self.conns,
            &self.conn_rates,
            &self.config,
            &self.icon_tex,
        );
        let today = (
            self.today_db_bytes.0 + self.conns.iter().map(|c| c.bytes_in).sum::<u64>(),
            self.today_db_bytes.1 + self.conns.iter().map(|c| c.bytes_out).sum::<u64>(),
        );
        self.ball_data = floating_ball::BallData {
            logo: Some(self.logo_tex.clone()),
            default_icon: self.default_icon_tex.clone(),
            rates: self.rates,
            today,
            up_top,
            down_top,
            elevated: self.elevated,
        };
    }

    /// 今日/当月落库用量查询(悬浮球今日总量与配额告警共用,单次扫描
    /// 月初窗口):分钟级节流派发工作线程执行,主线程只收割写缓存,
    /// 消费方(poll_ball_data/poll_quota)读缓存。两个消费方都关闭时
    /// 不查
    pub(super) fn poll_db_bytes(&mut self) {
        match self.db_bytes_rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(bytes)) => {
                self.db_bytes_rx = None;
                self.today_db_bytes = bytes.today;
                self.month_db_bytes = bytes.month;
            }
            Some(Err(std::sync::mpsc::TryRecvError::Empty)) => return,
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                // 工作线程异常退出:复位防节流判定永久卡在在途
                self.db_bytes_rx = None;
                tracing::warn!("[History] 用量查询线程异常退出,本次结果丢弃");
            }
            None => {}
        }
        let want = self.config.floating_ball.enabled || self.config.general.usage_quota_gb != 0;
        if !want || self.db_bytes_rx.is_some() || self.db_bytes_at.elapsed() < DB_BYTES_INTERVAL {
            return;
        }
        self.db_bytes_at = Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let db = crate::storage::db::open();
            let bytes = history_query::query_db_bytes(&db);
            // 发送失败 = 接收端已关闭(进程退出),静默退出
            let _ = tx.send(bytes);
        });
        self.db_bytes_rx = Some(rx);
    }

    /// 用量配额告警:分钟级检查当月累计(落库缓存,由 poll_db_bytes
    /// 分钟级异步刷新)+ 活跃连接字节,达到
    /// 80%/100% 阈值时 toast 告警;每级会话内一次,用量回落到阈值下
    /// (月切换/数据清理)自动重置
    pub(super) fn poll_quota(&mut self) {
        let quota_gb = self.config.general.usage_quota_gb;
        if quota_gb == 0 {
            self.quota_flags = (false, false);
            return;
        }
        if self.quota_checked_at.elapsed() < Duration::from_secs(60) {
            return;
        }
        self.quota_checked_at = Instant::now();
        let month = self.month_db_bytes;
        let active: u64 = self.conns.iter().map(|c| c.total_bytes()).sum();
        let total = month + active;
        // 阈值比较全程 saturating:quota_gb 来自配置(极大值时 u64 乘法
        // 溢出,release 回绕成小数导致告警误触发/失效,debug 直接 panic)
        let quota = u64::from(quota_gb).saturating_mul(1 << 30);
        // 两侧同量纲比较:80% 阈值即 quota 的 4/5。曾写成
        // `total * 100 >= quota / 5 * 4`(左侧放大 100 倍),等效阈值降到
        // 0.008 倍配额,且每级只报一次使真实 80% 永不到达
        let want = (total >= quota / 5 * 4, total >= quota);
        if want.0 && !self.quota_flags.0 {
            let text = self.i18n.t_with_args(
                "quota-warn-toast",
                &[("used", crate::model::fmt_bytes(total))],
            );
            crate::ui::toast::push(&mut self.toasts, crate::ui::toast::ToastKind::Warn, text);
        }
        if want.1 && !self.quota_flags.1 {
            let text = self.i18n.t_with_args(
                "quota-exceed-toast",
                &[("used", crate::model::fmt_bytes(total))],
            );
            crate::ui::toast::push(&mut self.toasts, crate::ui::toast::ToastKind::Warn, text);
        }
        self.quota_flags = want;
    }

    /// 历史页查询派发与收割:大库聚合查询秒级,不能在 UI 线程同步执行;
    /// 切视图/改筛选当帧只切换 UI(表格区显示加载动画),数据由工作
    /// 线程查回后落地重绘。查询在独立连接上执行(WAL 读者不互斥)。
    pub(super) fn poll_history_query(&mut self, ctx: &egui::Context) {
        // 冻结中(右键菜单打开):不派发不收割,rows 定格在打开瞬间;
        // rows 为空时仍放行建立基线(空表冻结无意义)
        let frozen = matches!(
            self.menu_frozen,
            Some(crate::ui::menu_float::MenuFrozen::History)
        );
        if frozen && !self.history.rows.is_empty() {
            return;
        }
        // 收割:在途结果落地(Ok)、动画帧节奏(Empty)、异常兜底(Disconnected)
        match self.history_query_rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(rows)) => {
                self.history_query_rx = None;
                self.history.rows = rows;
                self.history.loading = false;
                self.history.db_size = crate::storage::history::db_size();
                self.history.summary_merged = None;
                ctx.request_repaint();
            }
            Some(Err(std::sync::mpsc::TryRecvError::Empty)) => {
                // 在途:33ms 重绘驱动加载动画;期间可能有新交互置 dirty,
                // 落到派发分支做替换式重派
                ctx.request_repaint_after(Duration::from_millis(33));
            }
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                // 工作线程异常退出:复位加载态防 UI 永久转圈,本次查询放弃
                self.history_query_rx = None;
                self.history.loading = false;
                tracing::warn!("[History] 查询线程异常退出,本次结果丢弃");
                ctx.request_repaint();
            }
            None => {}
        }
        // 派发:到期判定通过则快照参数,替换在途查询(旧通道直接丢弃)
        let Some(spec) = self
            .history
            .due_for_query(self.config.general.hide_local, self.config.general.hide_lan)
        else {
            return;
        };
        self.history.rows = history_query::Rows::empty(spec.view);
        self.history.loading = true;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let started = Instant::now();
            let db = crate::storage::db::open();
            let rows = history_query::run_query(&db, &spec);
            tracing::debug!(
                "[History] [query] {} 完成: {} 行, 耗时 {:?}, {}",
                spec.view.label(),
                rows.len(),
                started.elapsed(),
                spec.describe()
            );
            // 发送失败 = 查询已被更新参数替换(接收端关闭),静默退出
            let _ = tx.send(rows);
        });
        self.history_query_rx = Some(rx);
    }

    /// 检查更新:接收设置页按钮触发,派发后台线程请求 GitHub Releases
    /// 并收割结果(网络阻塞调用不进主线程;完成即请求重绘,结果即时可见)
    pub(super) fn poll_update_check(&mut self, ctx: &egui::Context) {
        if self.update_request {
            self.update_request = false;
            if self.update_rx.is_none() {
                let preview = self.config.general.update_channel == "preview";
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let result = crate::platform::update::fetch_latest(preview);
                    if tx.send(result).is_err() {
                        tracing::warn!("[Update] 检查结果发送失败:接收端已关闭");
                    }
                });
                self.update_rx = Some(rx);
                self.update_result = None;
                tracing::info!(
                    "[Update] 检查更新已发起(渠道 {})",
                    if preview { "preview" } else { "stable" }
                );
            }
        }
        let Some(rx) = &self.update_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                match &result {
                    Ok(Some(info)) => {
                        let newer = crate::platform::update::is_newer(
                            &info.tag_name,
                            crate::platform::update::CURRENT_VERSION,
                        );
                        tracing::info!(
                            "[Update] 最新发布 {} (prerelease={}),{}更新",
                            info.tag_name,
                            info.prerelease,
                            if newer { "需要" } else { "无需" }
                        );
                    }
                    Ok(None) => tracing::info!("[Update] 仓库暂无发布版本"),
                    Err(e) => tracing::warn!("[Update] 检查失败: {e}"),
                }
                self.update_result = Some(result);
                self.update_rx = None;
                ctx.request_repaint();
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // 发送端先行 panic 的异常路径:复位避免 UI 永久停在检查中
                self.update_rx = None;
                self.update_result = Some(Err("更新检查线程异常退出".to_owned()));
            }
        }
    }
}
