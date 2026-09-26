//! 应用编排:托盘事件、采集 tick、页面切换、关闭到托盘与退出。
//!
//! eframe 0.36 的 App trait 拆分为 logic(每帧逻辑,窗口隐藏时仍会被调用)
//! 与 ui(绘制)。托盘命令与关闭拦截放在 logic,保证窗口隐藏时仍可响应。

use std::sync::mpsc::Receiver;
use std::time::Duration;

use eframe::egui;

use crate::collector::{Collector, MockCollector};
use crate::model::Connection;
use crate::theme;
use crate::tray::{self, Tray};
use crate::ui::{self, Page};

/// 重绘节奏:地图页动画 30fps,静态页面低频
const REPAINT_ANIMATED: Duration = Duration::from_millis(33);
const REPAINT_IDLE: Duration = Duration::from_millis(500);

pub struct NetOwlApp {
    page: Page,
    collector: Box<dyn Collector>,
    conns: Vec<Connection>,
    tray_rx: Receiver<String>,
    should_exit: bool,
    /// 托盘句柄保活,drop 时移除托盘图标
    _tray: Tray,
}

impl NetOwlApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        let (_tray, tray_rx) = tray::create(cc.egui_ctx.clone());
        NetOwlApp {
            page: Page::Map,
            collector: Box::new(MockCollector::new()),
            conns: Vec::new(),
            tray_rx,
            should_exit: false,
            _tray,
        }
    }

    fn handle_tray_commands(&mut self, ctx: &egui::Context) {
        for cmd in tray::drain(&self.tray_rx) {
            match cmd.as_str() {
                tray::CMD_SHOW => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                tray::CMD_HIDE => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
                tray::CMD_QUIT => {
                    self.should_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                _ => {}
            }
        }
    }
}

impl eframe::App for NetOwlApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_tray_commands(ctx);
        self.conns = self.collector.snapshot();

        // 点关闭按钮 = 隐藏到托盘;仅托盘"退出"命令置位后才放行
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && !self.should_exit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let page = &mut self.page;
        let conns = &self.conns;

        egui::Panel::left("nav")
            .exact_size(210.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_PANEL)
                    .inner_margin(egui::Margin { left: 14, right: 14, top: 18, bottom: 14 }),
            )
            .show(ui, |ui| ui::nav_ui(ui, page, conns));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_BASE)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ui, |ui| ui::central_ui(ui, page, conns));

        let repaint = match page {
            Page::Map => REPAINT_ANIMATED,
            _ => REPAINT_IDLE,
        };
        ui.ctx().request_repaint_after(repaint);
    }
}
