//! 数据表格公共件:统一表头、可排序表头、行悬停底色。
//!
//! 斑马纹由各页 Grid 的 `.striped(true)` 提供:奇数行底色取
//! `Visuals::faint_bg_color`(theme.rs 已接入调色板 faint 字段),
//! 行底色画在行内容之前,与行悬停垫底同为垫底层,悬停整体覆盖。

use eframe::egui;
use egui::{Color32, FontId, Label, RichText, Stroke};

use super::super::{theme, widgets};

/// 表头单元格垂直内边距:决定表头行高手感(与旧按钮 padding.y 一致)
const HEADER_PAD_Y: f32 = 3.0;

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

/// galley 首行基线(相对行顶):不同字体 galley 拼接时按基线对齐
fn first_baseline(galley: &egui::Galley) -> f32 {
    galley
        .rows
        .first()
        .and_then(|row| row.glyphs.first())
        .map_or(galley.size().y, |glyph| glyph.pos.y)
}

/// 可排序表头单元格:整个单元格可点(点击区 = 本列宽 x 表头行高),激活列
/// 整格高亮并带方向三角,点击返回(由调用方更新排序状态)。
/// 方向三角始终参与测量,非激活时以全透明字形占位:列宽在非激活时即含三角位,
/// 点击排序不会因列宽变化引起页面抖动;文字贴列缘左对齐,与数据列缘一致。
/// 占位测量按内容宽度推进(列宽可随数据收缩),点击/高亮区经 interact 扩到
/// 整格宽度,不参与 Grid 列宽测量。
pub fn header_sort_cell(
    ui: &mut egui::Ui,
    text: &str,
    active: bool,
    ascending: bool,
) -> egui::Response {
    let p = theme::c();
    let painter = ui.painter().clone();
    let font = FontId::proportional(theme::font::SM);
    let text_color = if active { p.text } else { p.text_dim };
    let text_galley = painter.layout_no_wrap(text.to_owned(), font.clone(), text_color);
    // 三角前留一个空格,与旧按钮文字串同构;非激活仅占测量宽度不可见
    let caret_galley = painter.layout_no_wrap(
        format!(" {}", widgets::segmented::sort_caret(ascending)),
        font,
        if active {
            text_color
        } else {
            Color32::TRANSPARENT
        },
    );
    let content_w = text_galley.size().x + caret_galley.size().x;
    let row_h = (text_galley.size().y + 2.0 * HEADER_PAD_Y).max(ui.spacing().interact_size.y);
    // available_width = 上一帧列宽(首帧未知时偏小,以内容宽兜底)
    let cell_w = ui.available_width().max(content_w);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(content_w, row_h), egui::Sense::click());
    let resp = ui.interact(
        egui::Rect::from_min_size(rect.min, egui::vec2(cell_w, row_h)),
        ui.id().with(("sort-header-cell", text)),
        egui::Sense::click(),
    );

    if active {
        painter.rect_filled(resp.rect, theme::RADIUS_SM, p.accent_soft);
        painter.rect_stroke(
            resp.rect,
            theme::RADIUS_SM,
            Stroke::new(1.0, p.accent.gamma_multiply(0.4)),
            egui::StrokeKind::Inside,
        );
    } else if resp.hovered() {
        painter.rect_filled(resp.rect, theme::RADIUS_SM, p.hover_bg);
    }

    // 文字贴列缘、行内垂直居中;三角与文字按基线对齐(图标字体行高不同)
    let y = rect.center().y - text_galley.size().y * 0.5;
    let dy = first_baseline(&text_galley) - first_baseline(&caret_galley);
    let caret_x = rect.left() + text_galley.size().x;
    painter.galley(egui::pos2(rect.left(), y), text_galley, text_color);
    painter.galley(egui::pos2(caret_x, y + dy), caret_galley, text_color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
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
