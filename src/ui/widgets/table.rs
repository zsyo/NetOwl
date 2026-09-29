//! 数据表格公共件:统一表头、可排序表头、行悬停底色。

use eframe::egui;
use egui::{Button, Color32, CornerRadius, Label, RichText, Stroke};

use super::super::{theme, widgets};

/// 表头单元格(纯展示列):小号加粗弱化文字
pub fn header_cell(ui: &mut egui::Ui, text: &str) {
    ui.add(
        Label::new(
            RichText::new(text)
                .size(theme::font::SM)
                .strong()
                .color(theme::c().text_dim),
        )
        .wrap_mode(egui::TextWrapMode::Extend),
    );
}

/// 可排序表头单元格:激活列高亮并带方向三角,点击返回(由调用方更新排序状态)
pub fn header_sort_cell(
    ui: &mut egui::Ui,
    text: &str,
    active: bool,
    ascending: bool,
) -> egui::Response {
    let p = theme::c();
    let mut text = text.to_owned();
    if active {
        text = format!("{} {}", text, widgets::segmented::sort_caret(ascending));
    }
    let label = RichText::new(text)
        .size(theme::font::SM)
        .strong()
        .color(if active { p.text } else { p.text_dim });
    ui.add(
        Button::new(label)
            .fill(if active {
                p.accent_soft
            } else {
                Color32::TRANSPARENT
            })
            .stroke(if active {
                Stroke::new(1.0, p.accent.gamma_multiply(0.4))
            } else {
                Stroke::NONE
            })
            .corner_radius(CornerRadius::same(theme::RADIUS_SM)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// 行底色:绘制在 Order::Background,垫在行内容之下(悬停高亮用)
pub fn row_background(ui: &egui::Ui, rect: egui::Rect, fill: Color32) {
    let bg_painter = egui::Painter::new(
        ui.ctx().clone(),
        egui::LayerId::new(egui::Order::Background, ui.layer_id().id),
        ui.clip_rect(),
    );
    bg_painter.rect_filled(rect, 0.0, fill);
}
