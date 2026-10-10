//! 轮询编排的外围任务:悬浮球数据快照、用量配额告警、检查更新结果
//! 回收。与连接采集无耦合,各自按节流独立运行(悬浮球 1s / 配额 60s /
//! 更新事件驱动);采集主流程见 super::poll。

use std::time::{Duration, Instant};

use eframe::egui;

use super::{NetOwlApp, TODAY_BYTES_INTERVAL};
use crate::ui::floating_ball;

impl NetOwlApp {
    /// 悬浮球数据快照:总速率与进程速率榜 1s 节流重建(与连接采集同频);
    /// 仅开关开启时计算。今日总量 = 落库当日收发(分钟级查库)+ 活跃
    /// 连接实时累计(未落库部分,每秒叠加)
    pub(super) fn poll_ball_data(&mut self) {
        if !self.config.floating_ball.enabled
            || self.ball_data_at.elapsed() < Duration::from_secs(1)
        {
            return;
        }
        self.ball_data_at = Instant::now();
        if self.today_bytes_at.elapsed() >= TODAY_BYTES_INTERVAL {
            self.today_bytes_at = Instant::now();
            self.today_db_bytes =
                crate::storage::history_query::query_today_bytes(&self.history_db);
        }
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

    /// 用量配额告警:分钟级轻查当月累计(落库)+ 活跃连接字节,达到
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
        let month = crate::storage::history_query::query_month_bytes(&self.history_db);
        let active: u64 = self.conns.iter().map(|c| c.total_bytes()).sum();
        let total = month + active;
        // 阈值比较全程 saturating:quota_gb 来自配置(极大值时 u64 乘法
        // 溢出,release 回绕成小数导致告警误触发/失效,debug 直接 panic)
        let quota = u64::from(quota_gb).saturating_mul(1 << 30);
        let want = (total.saturating_mul(100) >= quota / 5 * 4, total >= quota);
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
