//! 双序列柱状图:用量视图的时间桶柱(入站/出站语义色)。
//!
//! 左侧 Y 轴刻度固定,内容置于横向 ScrollArea 实现三段式桶宽:数据少时
//! 桶宽封顶并整体居中,中等时铺满视口,数据多时恒定最小桶宽横向滚动
//! (粘性跟随最新:停在右端时数据增长自动跟随,向左回看不打扰)。
//! 纵轴取 nice number 刻度画网格;悬停桶整槽高亮,数据卡浮动在该桶柱顶
//! 上方(跟随桶而非鼠标),柱很高无空间时翻到柱顶下方。

use eframe::egui;
use egui::{Align2, CornerRadius, FontId, Rect, Sense, Stroke, StrokeKind, Vec2};

use crate::model::fmt_bytes;
use crate::ui::icons;
use crate::ui::theme;

/// 柱宽占桶槽位比例(两柱 + 间隙恒小于 1,组间留白)
const BAR_RATIO: f32 = 0.3;
const BAR_GAP: f32 = 0.08;
/// 最小桶宽:低于此值进入横向滚动(双柱仍可分辨)
const SLOT_MIN: f32 = 14.0;
/// 最大桶宽:少量数据时封顶(内容整体居中,不无限加粗)
const SLOT_MAX: f32 = 44.0;
/// 底部标签区高度
const LABEL_H: f32 = 16.0;
/// 左侧 Y 轴刻度标签区宽度
const Y_LABEL_W: f32 = 46.0;
/// X 标签最小间距(按此推导稀疏化步长,滚动下标签固定在桶上不跳动)
const LABEL_SPACING: f32 = 90.0;
/// 粘性跟随的右端判定阈值(偏移距最右小于此值视为"停在最新")
const STICK_EPS: f32 = 8.0;
/// 悬停数据卡:内边距与行高
const CARD_PAD: f32 = 7.0;
const CARD_LINE1_H: f32 = 15.0;
const CARD_LINE2_H: f32 = 17.0;

/// 一根时间桶的展示数据
pub struct UsageBar {
    /// 桶标签(如 "10-01" / "10-01 14")
    pub label: String,
    /// 下载(入站)字节
    pub down: u64,
    /// 上传(出站)字节
    pub up: u64,
}

/// 双序列柱状图;三段式桶宽,数据多时横向滚动并粘性跟随最新
pub fn bar_chart(ui: &mut egui::Ui, bars: &[UsageBar], size: Vec2) {
    let p = theme::c();
    let n = bars.len();
    if n == 0 {
        // 占位保持调用方布局稳定
        ui.allocate_exact_size(size, Sense::hover());
        return;
    }
    let viewport_h = size.y - LABEL_H;

    // 左列固定 Y 轴刻度标签,右侧为横向滚动内容区
    let (label_rect, _) = ui.allocate_exact_size(Vec2::new(Y_LABEL_W, size.y), Sense::hover());
    let chart_w = ui.available_width();

    // 三段式桶宽:封顶居中 / 铺满 / 恒最小宽滚动
    let fit = chart_w / n as f32;
    let (slot, content_w, centered) = if fit > SLOT_MAX {
        (SLOT_MAX, chart_w, true)
    } else if fit >= SLOT_MIN {
        (fit, chart_w, false)
    } else {
        (SLOT_MIN, n as f32 * SLOT_MIN, false)
    };
    let max_offset = (content_w - chart_w).max(0.0);

    // 粘性跟随:上一帧停在右端时,本帧把视口推到最右(数据增长跟随);
    // 用户拖离右端后停止强制(show 后按实际偏移更新标志)
    let stick_id = egui::Id::new("usage-bar-stick");
    let mut stick = ui
        .ctx()
        .data_mut(|d| *d.get_temp_mut_or_insert_with(stick_id, || true));
    let mut area = egui::ScrollArea::horizontal()
        .id_salt("usage-bar-scroll")
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
        .auto_shrink(false);
    if stick && max_offset > 0.0 {
        area = area.scroll_offset(Vec2::new(max_offset, 0.0));
    }
    let out = area.show(ui, |ui| {
        let (content_rect, _) =
            ui.allocate_exact_size(Vec2::new(content_w, viewport_h + LABEL_H), Sense::hover());
        let origin_x = if centered {
            content_rect.left() + (content_w - n as f32 * slot) * 0.5
        } else {
            content_rect.left()
        };
        draw_content(ui, bars, slot, origin_x, content_rect, p);
    });
    let offset = out.state.offset.x;
    stick = offset >= max_offset - STICK_EPS;
    ui.ctx().data_mut(|d| d.insert_temp(stick_id, stick));

    // Y 轴刻度(固定列;网格线/基线在内容层随滚动绘制,横线横向滚动下
    // 视觉与固定绘制无异)
    let painter = ui.painter_at(label_rect);
    let nice_max = nice_ceil(
        bars.iter()
            .map(|b| b.down.max(b.up))
            .max()
            .unwrap_or(0)
            .max(1) as f32,
    );
    for v in [nice_max, nice_max * 0.5] {
        let y = label_rect.top() + viewport_h * (1.0 - v / nice_max);
        painter.text(
            egui::pos2(label_rect.right() - 6.0, y),
            Align2::RIGHT_CENTER,
            fmt_bytes(v as u64),
            FontId::proportional(theme::font::MICRO),
            p.text_dim,
        );
    }
    painter.text(
        egui::pos2(label_rect.right() - 6.0, label_rect.top() + viewport_h),
        Align2::RIGHT_CENTER,
        "0",
        FontId::proportional(theme::font::MICRO),
        p.text_dim,
    );
}

/// 内容层绘制:网格、柱、槽高亮、基线、X 标签与悬停卡(随内容滚动;
/// 内容区高度含底部标签带,plot 为其上部净图区)
fn draw_content(
    ui: &egui::Ui,
    bars: &[UsageBar],
    slot: f32,
    origin_x: f32,
    content_rect: Rect,
    p: &theme::Palette,
) {
    let painter = ui.painter_at(content_rect);
    let n = bars.len();
    let plot = Rect::from_min_max(
        egui::pos2(origin_x, content_rect.top()),
        egui::pos2(origin_x + n as f32 * slot, content_rect.bottom() - LABEL_H),
    );
    let nice_max = nice_ceil(
        bars.iter()
            .map(|b| b.down.max(b.up))
            .max()
            .unwrap_or(0)
            .max(1) as f32,
    );
    let bar_w = (slot * BAR_RATIO).max(1.0);
    let gap = slot * BAR_GAP;
    let center_of = |i: usize| plot.left() + slot * (i as f32 + 0.5);

    // 悬停定位桶(命中区下探到底部标签带:紧贴底缘的桶易悬停)
    let hover_idx = ui.input(|i| i.pointer.hover_pos()).and_then(|pos| {
        if pos.x < plot.left() || pos.x > plot.right() || pos.y < content_rect.top() {
            return None;
        }
        Some((((pos.x - plot.left()) / slot) as usize).min(n - 1))
    });

    if let Some(i) = hover_idx {
        let slot_rect = Rect::from_min_max(
            egui::pos2(plot.left() + slot * i as f32, plot.top()),
            egui::pos2(plot.left() + slot * (i as f32 + 1.0), plot.bottom()),
        );
        painter.rect_filled(slot_rect, CornerRadius::same(2), p.hover_bg);
    }

    // 悬停桶最高柱顶(数据卡垂直锚点)
    let hover_bar_top = hover_idx.map(|i| {
        let b = &bars[i];
        let h = (b.down.max(b.up) as f32 / nice_max * plot.height()).min(plot.height());
        plot.bottom() - h
    });

    for (i, b) in bars.iter().enumerate() {
        let cx = center_of(i);
        for (v, color, dx) in [
            (b.down, p.inbound, -(bar_w + gap) * 0.5),
            (b.up, p.outbound, (bar_w + gap) * 0.5),
        ] {
            if v == 0 {
                continue;
            }
            let h = (plot.height() * (v as f32 / nice_max))
                .min(plot.height())
                .max(1.0);
            let bar = Rect::from_min_size(
                egui::pos2(cx + dx - bar_w * 0.5, plot.bottom() - h),
                Vec2::new(bar_w, h),
            );
            painter.rect_filled(bar, CornerRadius::same(2), color.gamma_multiply(0.75));
        }
    }
    // 网格线与基线(横线在横向滚动下视觉与固定绘制无异)
    for v in [nice_max, nice_max * 0.5] {
        let y = plot.bottom() - plot.height() * (v / nice_max);
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            Stroke::new(1.0, p.faint),
        );
    }
    painter.line_segment(
        [
            egui::pos2(plot.left(), plot.bottom()),
            egui::pos2(plot.right(), plot.bottom()),
        ],
        Stroke::new(1.0, p.stroke),
    );

    // X 标签:按标签最小间距推导步长(固定索引,滚动不跳动)
    let step = ((LABEL_SPACING / slot) as usize).max(1);
    for (i, b) in bars.iter().enumerate() {
        if i % step != 0 {
            continue;
        }
        painter.text(
            egui::pos2(center_of(i), plot.bottom() + LABEL_H * 0.5),
            Align2::CENTER_CENTER,
            &b.label,
            FontId::proportional(theme::font::MICRO),
            p.text_dim,
        );
    }

    // 悬停数据卡:浮动在该桶柱顶上方(水平跟随桶、钳制在内容范围内),
    // 柱很高无上方空间时翻到柱顶下方
    if let (Some(i), Some(bar_top)) = (hover_idx, hover_bar_top) {
        let b = &bars[i];
        let font = FontId::proportional(theme::font::SM);
        let time_font = FontId::proportional(theme::font::XS);
        let down_text = format!("{} {}", icons::ARROW_DOWN, fmt_bytes(b.down));
        let up_text = format!("{} {}", icons::ARROW_UP, fmt_bytes(b.up));
        let w_time = text_w(&painter, &b.label, &time_font);
        let w_down = text_w(&painter, &down_text, &font);
        let w_up = text_w(&painter, &up_text, &font);
        let card_w = (w_time.max(w_down + 10.0 + w_up)) + CARD_PAD * 2.0;
        let card_h = CARD_LINE1_H + CARD_LINE2_H + CARD_PAD * 2.0;

        let card_top = if bar_top - 4.0 - card_h >= plot.top() {
            bar_top - 4.0 - card_h
        } else {
            (bar_top + 4.0).min(plot.bottom() - card_h)
        };
        let cx = center_of(i)
            .clamp(plot.left() + card_w * 0.5, plot.right() - card_w * 0.5)
            .max(plot.left() + card_w * 0.5);
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
}

/// 文本宽度(卡片尺寸测量)
fn text_w(painter: &egui::Painter, text: &str, font: &FontId) -> f32 {
    painter
        .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE)
        .rect
        .width()
}

/// 纵轴 nice 上取整:归一到 1/2/3/4/5/6/8/10 × 10^n,刻度值可读
fn nice_ceil(v: f32) -> f32 {
    if v <= 0.0 {
        return 1.0;
    }
    let base = 10f32.powf(v.log10().floor());
    let f = v / base;
    let nice = if f <= 1.0 {
        1.0
    } else if f <= 2.0 {
        2.0
    } else if f <= 3.0 {
        3.0
    } else if f <= 4.0 {
        4.0
    } else if f <= 5.0 {
        5.0
    } else if f <= 6.0 {
        6.0
    } else if f <= 8.0 {
        8.0
    } else {
        10.0
    };
    nice * base
}
