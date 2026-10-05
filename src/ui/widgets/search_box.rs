//! 统一搜索/筛选输入框:单行 TextEdit + BODY 字号;宽度由调用方给
//! (工具栏内定宽、面板内全宽),避免各页各搭一套。

use eframe::egui;
use egui::{FontId, RichText};

use super::super::theme;

/// 搜索输入框;`hint` 为空态提示词
pub fn search_box(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: String,
    width: f32,
) -> egui::Response {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(RichText::new(hint).size(theme::font::BODY))
            .font(FontId::proportional(theme::font::BODY))
            .desired_width(width),
    )
}
