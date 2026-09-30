//! 数据表格公共件:统一表头、可排序表头、行悬停底色。
//!
//! 斑马纹由各页 Grid 的 `.striped(true)` 提供:奇数行底色取
//! `Visuals::faint_bg_color`(theme.rs 已接入调色板 faint 字段),
//! 行底色画在行内容之前,与行悬停垫底同为垫底层,悬停整体覆盖。

use eframe::egui;
use egui::{Align, Color32, FontId, Label, Layout, RichText, Shape, Stroke, UiBuilder};

use super::super::theme;

/// 表头单元格垂直内边距:决定表头行高手感(与旧按钮 padding.y 一致)
const HEADER_PAD_Y: f32 = 3.0;

/// 单元格内容与列边缘的水平内边距:表格列为贴列布局(Grid spacing.x = 0),
/// 列的视觉间隔由相邻格的内容留白形成(右格 pad + 左格 pad),表头激活底色
/// 则横跨整列宽,与行斑马纹/悬停底连续
pub const CELL_PAD_X: f32 = theme::sp::MD;

/// 数字单元格:右对齐 + 语义色(速率/字节等可比大小数值列统一入口)
pub fn num_cell(ui: &mut egui::Ui, text: String, color: Color32) -> egui::Response {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.label(RichText::new(text).size(theme::font::BODY).color(color))
    })
    .inner
}

/// 定宽左对齐单元格:先在父布局精确占位(推进光标、参与 Grid 列宽测量),
/// 子 UI 画进该矩形、内容左右各缩进 CELL_PAD_X。allocate_ui_with_layout 的
/// 尺寸是上限、内容小会收缩,不能用于定宽列;长文本配 TextWrapMode::Truncate
/// 防撑破列宽
pub fn fixed_cell(ui: &mut egui::Ui, w: f32, h: f32, add: impl FnOnce(&mut egui::Ui)) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let mut child = ui.new_child(UiBuilder::new().max_rect(pad_rect(rect)));
    child.with_layout(Layout::left_to_right(Align::Center), add);
}

/// 定宽右对齐单元格(数字列):行高取虚拟化表格标准行 22
pub fn fixed_num_cell(ui: &mut egui::Ui, w: f32, add: impl FnOnce(&mut egui::Ui)) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 22.0), egui::Sense::hover());
    let mut child = ui.new_child(UiBuilder::new().max_rect(pad_rect(rect)));
    child.with_layout(Layout::right_to_left(Align::Center), add);
}

/// 定宽居中单元格(徽章等自适应宽内容):水平与垂直均居中于格
pub fn fixed_center_cell(ui: &mut egui::Ui, w: f32, h: f32, add: impl FnOnce(&mut egui::Ui)) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let mut child = ui.new_child(UiBuilder::new().max_rect(pad_rect(rect)));
    child.with_layout(
        Layout::left_to_right(Align::Center).with_main_align(Align::Center),
        add,
    );
}

/// 单元格内容区:占位矩形左右各缩进 CELL_PAD_X(高度不动)
fn pad_rect(rect: egui::Rect) -> egui::Rect {
    rect.shrink2(egui::vec2(CELL_PAD_X, 0.0))
}

/// 定宽表头单元格(纯展示列):显式列宽,用于独立表头 Grid 与数据 Grid
/// 列宽对齐;内容缩进 CELL_PAD_X,与数据列内容对齐
/// (`right_align` 供数据右对齐的数字列)
pub fn header_cell_w(ui: &mut egui::Ui, w: f32, text: &str, right_align: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 16.0), egui::Sense::hover());
    let mut child = ui.new_child(UiBuilder::new().max_rect(pad_rect(rect)));
    let layout = if right_align {
        Layout::right_to_left(Align::Center)
    } else {
        Layout::left_to_right(Align::Center)
    };
    child.with_layout(layout, |ui| {
        ui.add(
            Label::new(
                RichText::new(text)
                    .size(theme::font::SM)
                    .strong()
                    .color(theme::c().text_dim),
            )
            .wrap_mode(egui::TextWrapMode::Extend),
        );
    });
}

/// 表头排序双三角几何:实心直角三角取 Bootstrap caret-fill 同款比例
/// (宽:高 ≈ 2:1),上下两枚间距 1.5px,块总高约 9.5px,与 SM 表头文字协调
const CARET_W: f32 = 7.0;
const CARET_H: f32 = 4.0;
const CARET_GAP_Y: f32 = 1.5;
/// 表头文字与三角块的水平间距
const CARET_GAP_X: f32 = 4.0;

/// 可排序表头单元格:整个单元格可点(点击区 = 本列宽 x 表头行高),激活列
/// 整格高亮(横跨整列宽)。表头常驻置灰上下双三角标识本列可排序,激活列按
/// 当前方向点亮其中一枚(浅色主题置黑、深色主题置亮),另一枚保持灰色;
/// 点击返回(由调用方更新排序状态)。
/// 三角块占位与激活状态、方向无关:列宽在非激活时即含三角位,点击排序不会
/// 因列宽变化引起页面抖动;内容左右缩进 CELL_PAD_X,与数据列内容对齐
/// (`right_align` 供数据右对齐的数字列)。
/// `cell_w` = 本列宽(与数据列定宽一致,独立表头 Grid 亦能对齐);
/// 内容宽超过列宽时以内容宽兜底
pub fn header_sort_cell(
    ui: &mut egui::Ui,
    text: &str,
    active: bool,
    ascending: bool,
    right_align: bool,
    cell_w: f32,
) -> egui::Response {
    let p = theme::c();
    let painter = ui.painter().clone();
    let text_color = if active { p.text } else { p.text_dim };
    let text_galley = painter.layout_no_wrap(
        text.to_owned(),
        FontId::proportional(theme::font::SM),
        text_color,
    );
    let text_w = text_galley.size().x;
    let content_w = text_w + CARET_GAP_X + CARET_W;
    let row_h = (text_galley.size().y + 2.0 * HEADER_PAD_Y).max(ui.spacing().interact_size.y);
    let w = cell_w.max(content_w + 2.0 * CELL_PAD_X);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, row_h), egui::Sense::click());
    let resp = ui.interact(
        rect,
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

    // 内容贴列缘再缩进 CELL_PAD_X(左右随数据对齐)、行内垂直居中;三角块
    // 中心与文字 galley 中心同基准(字形视觉中心与 galley 中心偏差不足 1px)
    let x = if right_align {
        rect.left() + w - content_w - CELL_PAD_X
    } else {
        rect.left() + CELL_PAD_X
    };
    let y = rect.center().y - text_galley.size().y * 0.5;
    painter.galley(egui::pos2(x, y), text_galley, text_color);

    // 双三角:未激活列整体置灰;激活列点亮当前方向(升序上、降序下)
    let caret_cx = x + text_w + CARET_GAP_X + CARET_W * 0.5;
    let cy = rect.center().y;
    let tri_offset = (CARET_H + CARET_GAP_Y) * 0.5;
    let up_color = if active && ascending {
        p.text
    } else {
        p.text_dim
    };
    let down_color = if active && !ascending {
        p.text
    } else {
        p.text_dim
    };
    caret_tri(&painter, caret_cx, cy - tri_offset, true, up_color);
    caret_tri(&painter, caret_cx, cy + tri_offset, false, down_color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// 表头排序方向三角:实心等腰直角三角形,cx/cy 为三角中心,`up` 时尖朝上
fn caret_tri(painter: &egui::Painter, cx: f32, cy: f32, up: bool, color: Color32) {
    let (x0, x1) = (cx - CARET_W * 0.5, cx + CARET_W * 0.5);
    let (y0, y1) = (cy - CARET_H * 0.5, cy + CARET_H * 0.5);
    let pts = if up {
        vec![egui::pos2(x0, y1), egui::pos2(x1, y1), egui::pos2(cx, y0)]
    } else {
        vec![egui::pos2(x0, y0), egui::pos2(x1, y0), egui::pos2(cx, y1)]
    };
    painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
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
