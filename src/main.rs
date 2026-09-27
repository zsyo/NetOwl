//! NetOwl:Windows 平台网络连接监控工具(Little Snitch 复刻,首期只读监控)。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod basemap;
mod collector;
mod config;
mod db;
mod geoip;
mod i18n;
mod icon;
mod map;
mod model;
mod paths;
mod theme;
mod tray;
mod triangulate;
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

    let restore_target = cfg.window_position();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(APP_NAME)
        .with_min_inner_size([920.0, 620.0])
        .with_icon(Arc::new(icon::window_icon()));
    match restore_target {
        Some((x, y, w, h)) => {
            // 物理坐标作为逻辑值传入:系统 DPI 100% 时 winit 经主屏 scale(1.0)转换,
            // 窗口创建即位于目标位置(eframe 首帧渲染后立即显示窗口,创建位置就是
            // 唯一能消除闪现的手段);非 100% DPI 下的残余偏差由首帧命令修正(app.rs)
            viewport = viewport
                .with_position([x as f32, y as f32])
                .with_inner_size([w as f32, h as f32]);
        }
        None => {
            viewport = viewport.with_inner_size([1180.0, 760.0]);
        }
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(app::NetOwlApp::new(cc, i18n, cfg)))),
    )
}
