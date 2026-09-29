//! 页面头部:标题 + 副标题(+ 右侧动作区)。

use eframe::egui;
use egui::{Align, Layout, RichText};

use super::super::theme;

/// 页面头部:强调色标题 + 弱化副标题(空串省略)
pub fn page_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(
        RichText::new(title)
            .size(theme::font::H1)
            .strong()
            .color(theme::c().accent),
    );
    if !subtitle.is_empty() {
        ui.label(theme::dim_text(subtitle, theme::font::BODY));
    }
}

/// 带右侧动作区的页面头部(动作区与标题块垂直居中对齐)
pub fn page_header_row<R>(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    right: impl FnOnce(&mut egui::Ui) -> R,
) {
    ui.horizontal(|ui| {
        page_header(ui, title, subtitle);
        ui.with_layout(Layout::right_to_left(Align::Center), right);
    });
}
