//! 柱状图悬停数据卡:浮动在悬停桶柱顶上方(水平跟随桶、钳制在图内),
//! 柱很高无上方空间时翻到柱顶下方;浮层投影 + 1px 描边(地图悬停卡同款)。

use eframe::egui;
use egui::{Align2, CornerRadius, FontId, Rect, Stroke, StrokeKind, Vec2};

use super::UsageBar;
use crate::model::fmt_bytes;
use crate::ui::icons;
use crate::ui::theme;

/// 悬停数据卡:内边距与行高
const CARD_PAD: f32 = 7.0;
const CARD_LINE1_H: f32 = 15.0;
const CARD_LINE2_H: f32 = 17.0;

/// 文本宽度(卡片尺寸测量)
fn text_w(painter: &egui::Painter, text: &str, font: &FontId) -> f32 {
    painter
        .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE)
        .rect
        .width()
}

/// 绘制悬停桶的数据卡(`center_x` 为槽中心,由调用方钳制后传入)
pub(super) fn hover_card(
    painter: &egui::Painter,
    bars: &[UsageBar],
    i: usize,
    bar_top: f32,
    plot: Rect,
    center_x: f32,
    p: &theme::Palette,
) {
    let b = &bars[i];
    let font = FontId::proportional(theme::font::SM);
    let time_font = FontId::proportional(theme::font::XS);
    let down_text = format!("{} {}", icons::ARROW_DOWN, fmt_bytes(b.down));
    let up_text = format!("{} {}", icons::ARROW_UP, fmt_bytes(b.up));
    let w_time = text_w(painter, &b.label, &time_font);
    let w_down = text_w(painter, &down_text, &font);
    let w_up = text_w(painter, &up_text, &font);
    let card_w = (w_time.max(w_down + 10.0 + w_up)) + CARD_PAD * 2.0;
    let card_h = CARD_LINE1_H + CARD_LINE2_H + CARD_PAD * 2.0;

    let card_top = if bar_top - 4.0 - card_h >= plot.top() {
        bar_top - 4.0 - card_h
    } else {
        (bar_top + 4.0).min(plot.bottom() - card_h)
    };
    // 水平:槽中心,边缘桶钳制在图内;极少数据居中时图宽可能小于
    // 卡片宽,钳制区间反转会让 f32::clamp panic,下界兜底为上界
    let half = card_w * 0.5;
    let lo = plot.left() + half;
    let hi = (plot.right() - half).max(lo);
    let cx = center_x.clamp(lo, hi);
    let card = Rect::from_min_size(
        egui::pos2(cx - card_w * 0.5, card_top),
        Vec2::new(card_w, card_h),
    );

    // 浮层投影 + 卡底 + 1px 描边(地图悬停卡同款浮起层次)
    painter.add(theme::popup_shadow().as_shape(card, CornerRadius::same(theme::RADIUS_SM)));
    painter.rect_filled(card, CornerRadius::same(theme::RADIUS_SM), p.bg_float);
    painter.rect_stroke(
        card,
        CornerRadius::same(theme::RADIUS_SM),
        Stroke::new(1.0, p.stroke),
        StrokeKind::Inside,
    );

    painter.text(
        egui::pos2(
            card.left() + CARD_PAD,
            card.top() + CARD_PAD + CARD_LINE1_H * 0.5,
        ),
        Align2::LEFT_CENTER,
        &b.label,
        time_font,
        p.text_dim,
    );
    let line2_y = card.top() + CARD_PAD + CARD_LINE1_H + CARD_LINE2_H * 0.5;
    let x_down = card.left() + CARD_PAD;
    painter.text(
        egui::pos2(x_down, line2_y),
        Align2::LEFT_CENTER,
        &down_text,
        font.clone(),
        p.inbound,
    );
    painter.text(
        egui::pos2(x_down + w_down + 10.0, line2_y),
        Align2::LEFT_CENTER,
        &up_text,
        font,
        p.outbound,
    );
}
