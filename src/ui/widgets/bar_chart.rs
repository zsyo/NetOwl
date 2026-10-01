//! 双序列柱状图:用量视图的时间桶柱(入站/出站语义色)。
//!
//! allocate + painter 模式(参照 sparkline):不参与布局流,Y 轴按全序列
//! 最大值归一;每桶两根并排柱,X 标签稀疏化(桶多时隔档显示),悬停
//! 显示桶时间与精确字节。tooltip 走应用字体,方向图标可用 icons glyph。

use eframe::egui;
use egui::{Align2, CornerRadius, FontId, Rect, Sense, Stroke, Vec2};

use crate::model::fmt_bytes;
use crate::ui::icons;
use crate::ui::theme;

/// 柱宽占桶槽位比例(两柱 + 间隙恒小于 1,组间留白)
const BAR_RATIO: f32 = 0.3;
const BAR_GAP: f32 = 0.08;
/// 底部标签区高度
const LABEL_H: f32 = 16.0;
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

/// 双序列柱状图;悬停桶时在原位显示 tooltip(时间 + 下载/上传精确值)
pub fn bar_chart(ui: &mut egui::Ui, bars: &[UsageBar], size: Vec2) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    let p = theme::c();
    let max = bars
        .iter()
        .map(|b| b.down.max(b.up))
        .max()
        .unwrap_or(0)
        .max(1) as f32;
    let plot = Rect::from_min_max(
        rect.left_top(),
        egui::pos2(rect.right(), rect.bottom() - LABEL_H),
    );
    let n = bars.len();
    if n == 0 {
        return;
    }
    let slot = plot.width() / n as f32;
    let bar_w = (slot * BAR_RATIO).max(1.0);
    let gap = slot * BAR_GAP;
    // 组内两柱以槽中心对称排布
    let center_of = |i: usize| plot.left() + slot * (i as f32 + 0.5);

    for (i, b) in bars.iter().enumerate() {
        let cx = center_of(i);
        for (v, color, dx) in [
            (b.down, p.inbound, -(bar_w + gap) * 0.5),
            (b.up, p.outbound, (bar_w + gap) * 0.5),
        ] {
            if v == 0 {
                continue;
            }
            let h = (plot.height() * (v as f32 / max))
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

    // 悬停定位桶:标签区也纳入命中(紧贴底缘的桶易悬停)
    if let Some(pos) = resp.hover_pos()
        && pos.x >= plot.left()
        && pos.x <= plot.right()
    {
        let idx = (((pos.x - plot.left()) / slot) as usize).min(n - 1);
        let b = &bars[idx];
        resp.on_hover_text(format!(
            "{}\n{} {}\n{} {}",
            b.label,
            icons::ARROW_DOWN,
            fmt_bytes(b.down),
            icons::ARROW_UP,
            fmt_bytes(b.up)
        ));
    }
}
