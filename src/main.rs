//! NetOwl:Windows 平台网络连接监控工具(Little Snitch 复刻,首期只读监控)。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod collector;
mod config;
mod db;
mod i18n;
mod icon;
mod map;
mod model;
mod paths;
mod theme;
mod tray;
mod ui;
mod world;

use std::sync::Arc;

use eframe::egui;

/// 应用品牌名(各语言一致,不参与翻译)
const APP_NAME: &str = "NetOwl";

fn main() -> eframe::Result {
    // 数据根 = exe 同级;此后 config.toml、data/ 均为相对路径
    paths::init_data_root();

    let mut i18n = i18n::I18n::new();
    let available: Vec<String> =
        i18n.available_langs.iter().map(|info| info.code.clone()).collect();
    let cfg = config::Config::load(&i18n.current_lang, &available);
    // 配置文件中保存的语言优先于系统语言(load 内已校验有效性)
    i18n.set_language(cfg.general.language.clone());

    let _db = db::open();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([920.0, 620.0])
            .with_icon(Arc::new(icon::window_icon())),
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(app::NetOwlApp::new(cc, i18n, cfg)))),
    )
}
