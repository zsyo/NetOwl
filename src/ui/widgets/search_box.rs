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
    let mut focus = false;
    search_box_focus(ui, text, hint, width, &mut focus)
}

/// 搜索输入框(带聚焦请求):`focus` 为 true 时请求焦点并立即清零
/// (Ctrl+F 快捷键经 App 层标志位驱动,见 connections 工具栏)
pub fn search_box_focus(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: String,
    width: f32,
    focus: &mut bool,
) -> egui::Response {
    let resp = ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(RichText::new(hint).size(theme::font::BODY))
            .font(FontId::proportional(theme::font::BODY))
            .desired_width(width),
    );
    if *focus {
        resp.request_focus();
        *focus = false;
    }
    resp
}
