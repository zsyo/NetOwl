//! 进程图标单元格:图标或默认兜底(保证行内文字起点对齐)。

use eframe::egui;
use egui::Sense;

/// 进程图标:优先真实图标,无路径/提取失败时用 Windows 默认
/// "应用程序"图标兜底(`default` 未就绪时占位等尺寸空白)
pub fn proc_icon(
    ui: &mut egui::Ui,
    tex: Option<&egui::TextureHandle>,
    default: Option<&egui::TextureHandle>,
    size: f32,
) {
    let shown = tex.or(default);
    match shown {
        Some(t) => {
            ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(size, size)));
        }
        None => {
            ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
        }
    }
}
