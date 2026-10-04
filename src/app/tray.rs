//! 托盘命令分发与系统注册表同步:显示/日志/询问开关/静默三态/退出命令,
//! 托盘常驻(IsPromoted)与开机自启动(HKCU Run)的失败重试写入,
//! 悬停 tooltip 速率刷新与双入口勾选态校准。

use std::time::Instant;

use eframe::egui;

use super::{AUTOSTART_RETRY_INTERVAL, NetOwlApp, TRAY_PIN_RETRY_INTERVAL, TRAY_TIP_INTERVAL};
use crate::collector::CollectorKind;
use crate::logging;
use crate::model::fmt_bytes;
use crate::platform::autostart;
use crate::platform::tray;
use crate::storage::history;
use crate::ui::{self, Page};

impl NetOwlApp {
    pub(super) fn handle_tray_commands(&mut self, ctx: &egui::Context) {
        for cmd in tray::drain(&self.tray_rx) {
            tracing::debug!("[Tray] 托盘命令: {cmd}");
            match cmd.as_str() {
                tray::CMD_SHOW | tray::CMD_SETTINGS => {
                    // 设置项在恢复窗口的基础上落到设置页
                    if cmd == tray::CMD_SETTINGS {
                        self.page = Page::Settings;
                    }
                    self.window_visible = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    // 最小化的窗口样式仍为可见,Visible 是空操作,需显式解除
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                tray::CMD_LOG_OPEN => {
                    // 日志窗口 viewport 在首帧预建;全隐身状态(主窗口
                    // 隐藏且悬浮窗关闭)下首帧只跑 logic 不跑 ui,
                    // viewport 建不出来——先恢复主窗口驱动首帧
                    if !ui::log_window::is_created(&self.log_window) {
                        self.window_visible = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    ui::log_window::open(&mut self.log_window);
                }
                tray::CMD_ASK_TOGGLE => {
                    // CheckMenuItem 点击时 muda 已翻转原生勾选,写回 config
                    // (唯一来源)后校准勾选态并立即重开菜单
                    self.config.general.ask_connections = !self.config.general.ask_connections;
                    self.mark_config_dirty();
                    self.sync_ask_toggle();
                    // 原生菜单点击任一项必关闭(TrackPopupMenu 无保持打开
                    // 模式):翻转后立即重开,锚点复用右键弹出位置,菜单
                    // 原位重现,视觉等效保持打开
                    self._tray.reopen_menu();
                }
                tray::CMD_QUIT => {
                    // 退出收尾:仍活跃的连接补写为已完结行,等待写线程清空队列,
                    // 再把待写配置立即落盘(日志须在 logging::flush 之前)
                    let events = self.tracker.flush(history::unix_now());
                    tracing::info!("[App] 退出收尾:补写 {} 条活跃连接", events.len());
                    self.writer.send(events);
                    self.writer.shutdown();
                    if let Some(e) = self.etw.as_mut() {
                        e.shutdown();
                    }
                    logging::flush();
                    self.config.save_to_file();
                    self.config_dirty = false;
                    self.should_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                tray::CMD_SILENT_OFF | tray::CMD_SILENT_ALLOW | tray::CMD_SILENT_DENY => {
                    // 静默模式三态切换:写 config,兜底规则与托盘勾选态由
                    // 下一帧 sync_silent_mode 统一校准
                    let mode = cmd.strip_prefix("silent-").unwrap_or("off");
                    if self.config.general.silent_mode != mode {
                        self.config.general.silent_mode = mode.to_owned();
                        self.mark_config_dirty();
                    }
                }
                _ => {}
            }
        }
    }

    /// 托盘图标常驻:配置开关变化时写注册表 IsPromoted;失败(托盘项未注册、
    /// 系统不支持)静默保持系统默认行为并定时重试,直到达成目标态
    pub(super) fn sync_tray_pinned(&mut self) {
        let want = self.config.general.tray_pinned;
        if self.tray_pinned_applied == Some(want) || Instant::now() < self.tray_pin_retry_at {
            return;
        }
        if tray::set_pinned(want) {
            self.tray_pinned_applied = Some(want);
        } else {
            self.tray_pin_retry_at = Instant::now() + TRAY_PIN_RETRY_INTERVAL;
        }
    }

    /// 开机自启动:配置开关变化时注册计划任务(管理员令牌)或写 Run 键
    /// (标准用户);失败定时重试,直到达成目标态(config 为唯一来源,
    /// 注册表/任务残留态会被纠正)
    pub(super) fn sync_autostart(&mut self) {
        let want = self.config.general.autostart;
        if self.autostart_applied == Some(want) || Instant::now() < self.autostart_retry_at {
            return;
        }
        if autostart::set_enabled(want) {
            self.autostart_applied = Some(want);
        } else {
            tracing::debug!(
                "[Autostart] 自启动应用失败,{:?} 后重试",
                AUTOSTART_RETRY_INTERVAL
            );
            self.autostart_retry_at = Instant::now() + AUTOSTART_RETRY_INTERVAL;
        }
    }

    /// 托盘悬停提示跟随总速率刷新(TrafficMonitor 式,隐藏到托盘时
    /// 也能瞥一眼当前流量);原生 tooltip 不走应用字体,纯文本排版
    pub(super) fn update_tray_tooltip(&mut self) {
        if self.tray_tip_at.elapsed() < TRAY_TIP_INTERVAL {
            return;
        }
        self.tray_tip_at = Instant::now();
        let tip = format!(
            "{}\n{} {}/s\n{} {}/s",
            self.i18n.t("tray-tooltip"),
            self.i18n.t("nav-rate-down"),
            fmt_bytes(self.rates.0),
            self.i18n.t("nav-rate-up"),
            fmt_bytes(self.rates.1),
        );
        self._tray.set_tooltip(&tip);
    }

    /// 静默模式同步(配置变化或采集器重建后执行一次):deny 时注入全通配
    /// 兜底规则(求值自动命中,连接标注/地图阻断状态联动),其余状态撤销;
    /// 同时校准托盘子菜单勾选态(设置页与托盘双入口,config 是唯一来源);
    /// mock 数据源无真实流量,不注入兜底(避免演示数据误标阻断)
    pub(super) fn sync_silent_mode(&mut self) {
        let mode = self.config.general.silent_mode.clone();
        if self.silent_synced.as_deref() == Some(mode.as_str()) {
            return;
        }
        let deny = mode == "deny" && self.collector.kind() == CollectorKind::Real;
        self.rules.set_fallback(deny);
        self._tray.sync_silent(&mode);
        tracing::info!("[Silent] 静默模式 -> {mode}");
        self.silent_synced = Some(mode);
    }

    /// 新连接询问勾选同步(配置变化后执行一次):设置页与托盘双入口,
    /// config 是唯一来源
    pub(super) fn sync_ask_toggle(&mut self) {
        let on = self.config.general.ask_connections;
        if self.ask_synced == Some(on) {
            return;
        }
        self._tray.sync_ask(on);
        tracing::info!("[Ask] 新连接询问 -> {}", if on { "开" } else { "关" });
        self.ask_synced = Some(on);
    }
}
