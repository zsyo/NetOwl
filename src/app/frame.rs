//! eframe::App trait 实现(trait impl 不可拆分,logic 与 ui 必须同块):
//! logic = 每帧逻辑编排(窗口隐藏时仍被调用);ui = 每帧绘制编排
//! (标题栏、导航栏、地图面板、中央面板、重绘节奏、边缘缩放、浮层)。

use std::time::Instant;

use eframe::egui;

use super::{
    CONFIG_SAVE_DEBOUNCE, MIN_WINDOW_SIZE, NetOwlApp, REPAINT_ANIMATED, REPAINT_IDLE,
    REPAINT_STATIC,
};
use crate::map::world;
use crate::net::geoip;
use crate::platform::{resize, shutdown_hook, single_instance};
use crate::ui::ask as ui_ask;
use crate::ui::{self, Page, theme};

impl NetOwlApp {
    /// 界面语言与配置不一致(设置页切换)时同步进配置
    pub(super) fn sync_language_to_config(&mut self) {
        if self.i18n.current_lang != self.config.general.language {
            tracing::info!("[App] 界面语言 -> {}", self.i18n.current_lang);
            self.config.set_language(self.i18n.current_lang.clone());
            self.mark_config_dirty();
        }
    }

    /// 界面主题与配置不一致(设置页切换)时同步进配置
    pub(super) fn sync_theme_to_config(&mut self) {
        let current = theme::theme_str();
        if current != self.config.general.theme {
            tracing::info!("[App] 主题 -> {current}");
            self.config.set_theme(current.to_owned());
            self.mark_config_dirty();
        }
    }

    pub(super) fn mark_config_dirty(&mut self) {
        self.config_dirty = true;
        self.config_dirty_since = Instant::now();
    }

    /// 无边框窗口边缘缩放热区(winit 对 undecorated 窗口无 hit-test):
    /// 命中即设置缩放光标并驱动拖拽缩放;放帧末使光标覆盖页面控件设置
    fn resize_hotspots(&mut self, ui: &mut egui::Ui) {
        let maximized = ui.ctx().input(|i| i.viewport().maximized).unwrap_or(false);
        let hit = if maximized {
            None
        } else {
            let screen = ui.ctx().input(|i| i.viewport_rect());
            ui.ctx()
                .pointer_latest_pos()
                .and_then(|pos| resize::edge_hit_test(pos, screen))
        };
        let press = ui.ctx().input(|i| i.pointer.primary_pressed());
        let held = ui.ctx().input(|i| i.pointer.primary_down());
        self.window_resize.update(
            self.main_hwnd,
            hit,
            press,
            held,
            MIN_WINDOW_SIZE,
            ui.ctx().pixels_per_point(),
        );
        let cursor_hit = self.window_resize.active().or(hit);
        if let Some(h) = cursor_hit {
            ui.ctx().set_cursor_icon(h.cursor_icon());
        }
    }
}

impl eframe::App for NetOwlApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_tray_commands(ctx);
        self.calibrate_window_visible();
        self.sync_tray_pinned();
        self.sync_autostart();
        self.poll_ball_data();
        self.update_tray_tooltip();
        // 关机/注销落库钩子:首帧安装一次,内部防重复(winit 不处理
        // ENDSESSION,关机时唯一能把活跃连接落库的路径)
        shutdown_hook::install(
            if self.main_hwnd != 0 {
                self.main_hwnd
            } else {
                single_instance::main_hwnd(crate::APP_NAME)
            },
            &mut self.tracker,
            &mut self.writer,
        );
        self.ensure_collector();
        self.poll_local_ip();
        self.lan.poll(&self.history_db);
        self.poll_traffic(ctx);
        self.poll_conns(ctx);
        self.writer.set_retention(self.config.general.history_days);
        self.sync_silent_mode();
        self.sync_ask_toggle();
        self.poll_wfp();

        // 进入设置页时重扫 locales,加载运行期间新增的词条文件;
        // 进入历史页时标记重新加载(结果与库大小)
        if self.page == Page::Settings && self.last_page != Page::Settings {
            self.i18n.refresh_languages();
        }
        if self.page == Page::History && self.last_page != Page::History {
            self.history.dirty = true;
        }
        self.last_page = self.page;

        self.sync_language_to_config();
        self.sync_theme_to_config();
        self.restore_window_geometry(ctx);
        self.capture_window_geometry(ctx);

        // 点关闭按钮 = 隐藏到托盘;仅托盘"退出"命令置位后才放行
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && !self.should_exit {
            self.window_visible = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // 配置写盘节流:防抖窗口内合并连续变更,只落盘最后一次
        if self.config_dirty && self.config_dirty_since.elapsed() >= CONFIG_SAVE_DEBOUNCE {
            self.config.save_to_file();
            self.config_dirty = false;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 页面与重绘判定先行(page 借用持续到帧末,后续不能碰 self)
        let shown = self.is_shown(ui.ctx());
        let log_open = self.log_window.open;

        // 自绘标题栏(顶栏最先声明,位于导航栏之上);窗口动作直接处理,
        // 此时 page/UiCtx 借用尚未建立,Close 可安全写 self.window_visible
        let title_action = egui::Panel::top("titlebar")
            .exact_size(ui::titlebar::HEIGHT)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::c().bg_panel))
            .show(ui, |ui| ui::titlebar::show(ui, crate::APP_NAME))
            .inner;
        match title_action {
            ui::titlebar::TitleAction::Minimize => {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            ui::titlebar::TitleAction::ToggleMaximize => {
                let maximized = ui.ctx().input(|i| i.viewport().maximized).unwrap_or(false);
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            ui::titlebar::TitleAction::Close => {
                // 与系统关闭事件同路径:隐藏到托盘(logic 的 close 拦截兜底 Alt+F4)
                self.window_visible = false;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            ui::titlebar::TitleAction::None => {}
        }

        // 字段级拆借用:conns/rdns 只读,config/map_view 需可变(设置页与地图交互)
        let page = &mut self.page;
        let collector_kind = self.collector.kind();
        // 本机点位:公网 IP 归属(探测失败/未收录时用默认位置)
        let local_pos = self
            .local_place
            .map(geoip::place_pos)
            .unwrap_or((world::LOCAL.lon, world::LOCAL.lat));
        let mut ctx = ui::UiCtx {
            conns: &self.conns,
            i18n: &mut self.i18n,
            map_view: &mut self.map_view,
            config: &mut self.config,
            rdns: &self.rdns,
            rates: self.rates,
            rate_hist: &self.rate_hist,
            conn_rates: &self.conn_rates,
            conn_sort: &mut self.conn_sort,
            conn_search: &mut self.conn_search,
            conn_row_hover: &mut self.conn_row_hover,
            rules_row_hover: &mut self.rules_row_hover,
            log_window: &mut self.log_window,
            icon_tex: &self.icon_tex,
            default_icon_tex: self.default_icon_tex.as_ref(),
            history: &mut self.history,
            history_db: &self.history_db,
            rules: &mut self.rules,
            rules_page: &mut self.rules_page,
            map_panels: &mut self.map_panels,
            wfp_status: self.wfp.status(),
            elevated: self.elevated,
            writer: &self.writer,
            lan_devices: self.lan.devices(),
            local_pos,
        };
        let mut config_changed = false;

        egui::Panel::left("nav")
            .exact_size(210.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::c().bg_panel)
                    .inner_margin(egui::Margin {
                        left: 14,
                        right: 14,
                        top: 18,
                        bottom: 14,
                    }),
            )
            .show(ui, |ui| {
                ui::nav_ui(ui, page, &ctx, &self.logo_tex, collector_kind)
            });

        // 地图页操作面板:面板必须先于中央面板声明(egui 的面板顺序约束),
        // 位于导航栏与中央画布之间,宽度可拖拽调节
        if *page == Page::Map && ctx.map_panels.show_left {
            egui::Panel::left("map-list")
                .resizable(true)
                .default_size(300.0)
                .min_size(260.0)
                .max_size(460.0)
                .frame(
                    egui::Frame::new()
                        .fill(theme::c().bg_panel)
                        .inner_margin(egui::Margin::same(10)),
                )
                .show(ui, |ui| ui::map_panel::list_panel(ui, &mut ctx));
        }
        if *page == Page::Map && ctx.map_panels.show_right {
            egui::Panel::right("map-inspector")
                .resizable(true)
                .default_size(320.0)
                .min_size(260.0)
                .max_size(480.0)
                .frame(
                    egui::Frame::new()
                        .fill(theme::c().bg_panel)
                        .inner_margin(egui::Margin::same(10)),
                )
                .show(ui, |ui| ui::map_inspector::inspector_panel(ui, &mut ctx));
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::c().bg_base)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ui, |ui| {
                config_changed |= ui::central_ui(ui, page, &mut ctx);
            });
        // 后台(托盘/最小化)统一低频;可见时地图动画 30fps,连接页速率 500ms,
        // 历史/规则/设置等静态页 1s(页面切换与交互事件即时唤醒);
        // 日志窗口打开时 500ms 保证流式观察
        let repaint = if !shown {
            REPAINT_IDLE
        } else {
            match page {
                Page::Map => REPAINT_ANIMATED,
                Page::Connections => REPAINT_IDLE,
                _ if log_open => REPAINT_IDLE,
                _ => REPAINT_STATIC,
            }
        };
        ui.ctx().request_repaint_after(repaint);

        self.resize_hotspots(ui);

        // 新连接询问弹窗(独立 viewport,预建常驻);决策即时生效
        let decision = ui_ask::show(
            ui.ctx(),
            &mut self.asker,
            &mut self.ask_ui_visible,
            &self.i18n,
        );
        if let Some(d) = decision {
            self.apply_decision(d);
        }
        // 日志浏览窗口(独立 viewport;内存层增量拉取,关闭时停止收集)
        ui::log_window::show(ui.ctx(), &mut self.log_window, &self.i18n);

        // 悬浮窗(独立 viewport;主窗口隐藏时低频帧仍维持显示与数据刷新)
        self.show_floating_ball(ui);

        if config_changed {
            self.mark_config_dirty();
        }
    }
}
