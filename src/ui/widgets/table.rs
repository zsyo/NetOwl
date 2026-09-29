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

/// 表格行间距(Grid spacing.y):行底色范围与行高测量共用
pub const ROW_SPACING_Y: f32 = 9.0;

/// 行悬停高亮辅助:跨帧记录实测行高。
/// 行底色必须画在行内容之前才能垫底(同层内先画者在下,见 ui 层根背景层说明),
/// 而行高要 end_row 后才确定,故行首用上一帧实测行高判定悬停并垫底;
/// 行高由行内最高单元格决定,列表稳态下恒定,首帧(未测量)不高亮
#[derive(Default)]
pub struct RowHover {
    row_h: f32,
}

impl RowHover {
    /// 行首调用(读行顶之后、本行单元格之前):指针落在本行则垫底色
    pub fn begin(&mut self, ui: &egui::Ui, left: f32, right: f32, top: f32) {
        if self.row_h <= 0.0 {
            return;
        }
        let rect =
            egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, top + self.row_h));
        if ui.rect_contains_pointer(rect) {
            ui.painter().rect_filled(rect, 0.0, theme::c().hover_bg);
        }
    }

    /// 行末调用(end_row 之后):记录本帧行高供下一帧行首判定
    pub fn end(&mut self, ui: &egui::Ui, top: f32) {
        self.row_h = (ui.cursor().top() - ROW_SPACING_Y - top).max(0.0);
    }
}
