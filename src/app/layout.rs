//! 每帧 ui 编排的可下沉函数体:标题栏动作落地、UiCtx 与页面借用构造、
//! 地图面板声明、重绘节奏判定、边缘缩放热区。eframe::App trait impl
//! 必须同块于 frame,本文件经方法调用参与每帧绘制编排。

use std::time::Duration;

use eframe::egui;

use super::{MIN_WINDOW_SIZE, NetOwlApp, REPAINT_ANIMATED, REPAINT_IDLE, REPAINT_STATIC};
use crate::collector::CollectorKind;
use crate::map::world;
use crate::net::geoip;
use crate::platform::resize;
use crate::ui::{self, Page, UiCtx, theme};

impl NetOwlApp {
    /// 无边框窗口边缘缩放热区(winit 对 undecorated 窗口无 hit-test):
    /// 命中即设置缩放光标并驱动拖拽缩放;放帧末使光标覆盖页面控件设置
    pub(super) fn resize_hotspots(&mut self, ui: &mut egui::Ui) {
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

    /// 标题栏动作落地(窗口动作直接处理,此时 page/UiCtx 借用尚未建立,
    /// Close 可安全写 self.window_visible)
    pub(super) fn handle_title_action(
        &mut self,
        action: ui::titlebar::TitleAction,
        ctx: &egui::Context,
    ) {
        match action {
            ui::titlebar::TitleAction::Minimize => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            ui::titlebar::TitleAction::ToggleMaximize => {
                let maximized = ctx.input(|i| i.viewport().maximized).unwrap_or(false);
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            ui::titlebar::TitleAction::Close => {
                // 与系统关闭事件同路径:隐藏到托盘(logic 的 close 拦截兜底 Alt+F4)
                self.window_visible = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            ui::titlebar::TitleAction::None => {}
        }
    }

    /// 三个主面板声明(顺序即布局:导航 → 地图左右面板 → 中央画布),
    /// 返回页面产生的配置变更标志。字段级拆借用:conns/rdns/logo 只读,
    /// config/map_view 需可变(设置页与地图交互)
    pub(super) fn show_panels(&mut self, ui: &mut egui::Ui, collector_kind: CollectorKind) -> bool {
        let page = &mut self.page;
        let logo = &self.logo_tex;
        // 本机点位:公网 IP 归属(探测失败/未收录时用默认位置)
        let local_pos = self
            .local_place
            .map(geoip::place_pos)
            .unwrap_or((world::LOCAL.lon, world::LOCAL.lat));
        let mut ctx = UiCtx {
            conns: &self.conns,
            i18n: &mut self.i18n,
            map_view: &mut self.map_view,
            config: &mut self.config,
            rdns: &self.rdns,
            rates: self.rates,
            rate_hist: &self.rate_hist,
            conn_rates: &self.conn_rates,
            conn_sort: &mut self.conn_sort,
            conn_view: &mut self.conn_view,
            conn_grouped: &mut self.conn_grouped,
            conn_collapsed: &mut self.conn_collapsed,
            nav_request: &mut self.nav_request,
            listens: &self.listens,
            conn_search: &mut self.conn_search,
            focus_conn_search: &mut self.focus_conn_search,
            hotkey_capture: &mut self.hotkey_capture,
            conn_proto: &mut self.conn_proto,
            update_check_request: &mut self.update_request,
            update_checking: self.update_rx.is_some(),
            update_result: &mut self.update_result,
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
            .show(ui, |ui| ui::nav_ui(ui, page, &ctx, logo, collector_kind));

        declare_map_panels(ui, page, &mut ctx);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::c().bg_base)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ui, |ui| {
                config_changed |= ui::central_ui(ui, page, &mut ctx);
            });
        config_changed
    }
}

/// 地图页操作面板:面板必须先于中央面板声明(egui 的面板顺序约束),
/// 位于导航栏与中央画布之间,宽度可拖拽调节
pub(super) fn declare_map_panels(ui: &mut egui::Ui, page: &Page, ctx: &mut UiCtx) {
    if *page != Page::Map {
        return;
    }
    if ctx.map_panels.show_left {
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
            .show(ui, |ui| ui::map_panel::list_panel(ui, ctx));
    }
    if ctx.map_panels.show_right {
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
            .show(ui, |ui| ui::map_inspector::inspector_panel(ui, ctx));
    }
}

/// 重绘节奏:后台(托盘/最小化)统一低频;可见时地图动画 30fps,
/// 连接页速率 500ms,历史/规则/设置等静态页 1s(页面切换与交互事件
/// 即时唤醒);日志窗口打开时 500ms 保证流式观察
pub(super) fn repaint_interval(shown: bool, page: &Page, log_open: bool) -> Duration {
    if !shown {
        REPAINT_IDLE
    } else {
        match page {
            Page::Map => REPAINT_ANIMATED,
            Page::Connections => REPAINT_IDLE,
            _ if log_open => REPAINT_IDLE,
            _ => REPAINT_STATIC,
        }
    }
}
