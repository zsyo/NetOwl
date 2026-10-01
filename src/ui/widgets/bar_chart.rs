//! 双序列柱状图:用量视图的时间桶柱(入站/出站语义色)。
//!
//! allocate + painter 模式(参照 sparkline):不参与布局流。纵轴取 nice
//! number 刻度并画网格线;顶部常驻信息行显示悬停桶(无悬停显示最新桶)
//! 的时间与收发字节——数据位置固定,视线不用追鼠标;悬停桶整槽高亮。
//! 信息行走应用字体,方向图标可用 icons glyph。

use eframe::egui;
use egui::{Align2, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use crate::model::fmt_bytes;
use crate::ui::icons;
use crate::ui::theme;

/// 柱宽占桶槽位比例(两柱 + 间隙恒小于 1,组间留白)
const BAR_RATIO: f32 = 0.3;
const BAR_GAP: f32 = 0.08;
/// 底部标签区高度
const LABEL_H: f32 = 16.0;
/// 顶部常驻信息行高度
const INFO_H: f32 = 18.0;
/// 左侧 Y 轴刻度标签区宽度
const Y_LABEL_W: f32 = 46.0;
/// X 标签最大条数(超出按整除步长隔档显示)
const MAX_LABELS: usize = 12;

/// 一根时间桶的展示数据
pub struct UsageBar {
    /// 桶标签(如 "10-01" / "10-01 14")
    pub label: String,
    /// 下载(入站)字节
    pub down: u64,
    /// 上传(出站)字节
    pub up: u64,
}

/// 双序列柱状图;悬停桶整槽高亮,数据固定展示在顶部信息行
pub fn bar_chart(ui: &mut egui::Ui, bars: &[UsageBar], size: Vec2) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    let p = theme::c();
    let n = bars.len();
    if n == 0 {
        return;
    }
    // 纵轴 nice 刻度:柱高不再顶满图顶,留出可读余量
    let raw_max = bars
        .iter()
        .map(|b| b.down.max(b.up))
        .max()
        .unwrap_or(0)
        .max(1) as f32;
    let nice_max = nice_ceil(raw_max);
    let plot = Rect::from_min_max(
        egui::pos2(rect.left() + Y_LABEL_W, rect.top() + INFO_H),
        egui::pos2(rect.right(), rect.bottom() - LABEL_H),
    );
    let slot = plot.width() / n as f32;
    let bar_w = (slot * BAR_RATIO).max(1.0);
    let gap = slot * BAR_GAP;
    // 组内两柱以槽中心对称排布
    let center_of = |i: usize| plot.left() + slot * (i as f32 + 0.5);

    // 悬停定位桶(标签区也纳入命中:紧贴底缘的桶易悬停)
    let hover_idx = resp.hover_pos().and_then(|pos| {
        if pos.x < plot.left() || pos.x > plot.right() || pos.y < rect.top() {
            return None;
        }
        Some((((pos.x - plot.left()) / slot) as usize).min(n - 1))
    });

    // 顶部常驻信息行:悬停桶优先,无悬停显示最新桶;分段着色与导航
    // 速率卡同口径(时间弱化 / 下行入站色 / 上行出站色)
    let shown = hover_idx.unwrap_or(n - 1);
    let b = &bars[shown];
    let info_y = rect.top() + INFO_H * 0.5;
    let mut x = plot.left();
    x = paint_text(
        &painter,
        egui::pos2(x, info_y),
        Align2::LEFT_CENTER,
        &b.label,
        FontId::proportional(theme::font::SM),
        p.text_dim,
    );
    x = paint_text(
        &painter,
        egui::pos2(x + theme::sp::MD, info_y),
        Align2::LEFT_CENTER,
        &format!("{} {}", icons::ARROW_DOWN, fmt_bytes(b.down)),
        FontId::proportional(theme::font::SM),
        p.inbound,
    );
    paint_text(
        &painter,
        egui::pos2(x + theme::sp::MD, info_y),
        Align2::LEFT_CENTER,
        &format!("{} {}", icons::ARROW_UP, fmt_bytes(b.up)),
        FontId::proportional(theme::font::SM),
        p.outbound,
    );

    // Y 轴网格:满值 / 半值两条弱化线 + 0 基线,左侧刻度标签
    for v in [nice_max, nice_max * 0.5] {
        let y = plot.bottom() - plot.height() * (v / nice_max);
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            Stroke::new(1.0, p.faint),
        );
        painter.text(
            egui::pos2(plot.left() - 6.0, y),
            Align2::RIGHT_CENTER,
            fmt_bytes(v as u64),
            FontId::proportional(theme::font::MICRO),
            p.text_dim,
        );
    }
    painter.text(
        egui::pos2(plot.left() - 6.0, plot.bottom()),
        Align2::RIGHT_CENTER,
        "0",
        FontId::proportional(theme::font::MICRO),
        p.text_dim,
    );

    // 悬停桶整槽高亮(垫在柱子之下)
    if let Some(i) = hover_idx {
        let slot_rect = Rect::from_min_max(
            egui::pos2(plot.left() + slot * i as f32, plot.top()),
            egui::pos2(plot.left() + slot * (i as f32 + 1.0), plot.bottom()),
        );
        painter.rect_filled(slot_rect, CornerRadius::same(2), p.hover_bg);
    }

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
    // 基线与稀疏 X 标签
    painter.line_segment(
        [
            egui::pos2(plot.left(), plot.bottom()),
            egui::pos2(plot.right(), plot.bottom()),
        ],
        Stroke::new(1.0, p.stroke),
    );
    let step = n.div_ceil(MAX_LABELS);
    for (i, b) in bars.iter().enumerate() {
        if i % step != 0 {
            continue;
        }
        painter.text(
            egui::pos2(center_of(i), rect.bottom() - LABEL_H * 0.5),
            Align2::CENTER_CENTER,
            &b.label,
            FontId::proportional(theme::font::MICRO),
            p.text_dim,
        );
    }
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

/// 单段着色文本,返回绘制后的右缘 x(信息行分段排布用)。
/// egui 锚点 x ∈ {-1, 0, 1}(左/中/右),文本中点 = pos + (1-ax)/2 × 宽
fn paint_text(
    painter: &egui::Painter,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    font: FontId,
    color: egui::Color32,
) -> f32 {
    painter.text(pos, anchor, text, font.clone(), color);
    let w = painter
        .layout_no_wrap(text.to_owned(), font, color)
        .rect
        .width();
    // 锚点系数:左对齐文本右缘 = pos + 全宽,居中半宽,右对齐不偏移
    let factor = match anchor.x() {
        egui::Align::Min => 1.0,
        egui::Align::Center => 0.5,
        egui::Align::Max => 0.0,
    };
    pos.x + factor * w
}
