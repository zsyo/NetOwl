//! 进程图标单元格:16x16 图标或等尺寸占位(保证行内文字起点对齐)。

use eframe::egui;
use egui::Sense;

/// 进程图标:`tex` 为 None 时占位等尺寸空白(占位保证各行文字对齐不跳动)
pub fn proc_icon(ui: &mut egui::Ui, tex: Option<&egui::TextureHandle>, size: f32) {
    match tex {
        Some(t) => {
            ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(size, size)));
        }
        None => {
            ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
        }
    }
}
