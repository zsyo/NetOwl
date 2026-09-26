//! NetOwl:Windows 平台网络连接监控工具(Little Snitch 复刻,首期只读监控)。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod collector;
mod icon;
mod map;
mod model;
mod text;
mod theme;
mod tray;
mod ui;
mod world;

use eframe::egui;

use crate::text::APP_NAME;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([920.0, 620.0])
            .with_icon(std::sync::Arc::new(icon::window_icon())),
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(app::NetOwlApp::new(cc)))),
    )
}
