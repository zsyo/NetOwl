//! 悬浮窗长条速率条:圆角矩形底 + 细描边 + 按模式的贴边半隐信号格
//! 窄条或全显速率两行。详情浮窗在 view。

use eframe::egui;
use egui::Stroke;

use super::types::{BAR_H, BAR_W, BallData, RATE_STEPS, STRIP_W};
use crate::model::fmt_bytes;
use crate::ui::{icons, theme};

/// 控件绘制模式:贴边半隐(窄条信号格)或全显(长条速率两行)
pub(super) enum BallMode {
    /// 贴边半隐:控件大部分移出窗口被裁剪,只露贴边侧窄条;
    /// bars_left = 条在控件左端(即控件贴右屏缘)
    Docked { bars_left: bool },
    /// 全显:长条两行 上传/下载速率
    Full,
}

/// 速率紧凑格式:去单位空格,球面与榜单共用("1.5MB/s")
pub(super) fn fmt_rate(n: u64) -> String {
    format!("{}/s", fmt_bytes(n).replace(' ', ""))
}

/// 长条控件:圆角矩形底 + 细描边 + 按模式的窄条信号格或速率两行
pub(super) fn ball(ui: &mut egui::Ui, cx: f32, cy: f32, data: &BallData, mode: BallMode) {
    let painter = ui.painter();
    let rect = egui::Rect::from_center_size(egui::pos2(cx, cy), egui::vec2(BAR_W, BAR_H));
    let radius = egui::CornerRadius::same(theme::RADIUS_SM);
    painter.rect_filled(rect, radius, theme::c().bg_float);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, theme::c().stroke),
        egui::StrokeKind::Inside,
    );

    match mode {
        BallMode::Docked { bars_left } => signal_grid(painter, cx, data, bars_left),
        BallMode::Full => rate_stack(painter, cx, cy, data),
    }
}

/// 贴边半隐窄条信号格:上下两组各 4 格、组内从下往上堆叠——上组 =
/// 上传(贴条顶)、下组 = 下载(贴条底),组间留明显间隔;速率达到
/// 档位阈值时从各自组的最低格向上点亮
fn signal_grid(painter: &egui::Painter, cx: f32, data: &BallData, bars_left: bool) {
    let (bar_w, bar_h, gap) = (8.0_f32, 3.0_f32, 2.0_f32);
    // 窄条位于控件贴屏侧端部:由控件中心与端部方向推出条中心 x
    let strip_cx = if bars_left {
        cx - BAR_W * 0.5 + STRIP_W * 0.5
    } else {
        cx + BAR_W * 0.5 - STRIP_W * 0.5
    };
    // (速率, 点亮色, 组底 y):下载组贴条底、上传组贴中线,组间间隔 =
    // 中线 - 条底 - 组高,随 BAR_H 自适应
    let groups = [
        (data.rates.0, theme::c().inbound, BAR_H - 4.0),
        (data.rates.1, theme::c().outbound, BAR_H * 0.5 - 4.0),
    ];
    for (rate, color, group_bottom) in groups {
        for (g, step) in RATE_STEPS.iter().enumerate() {
            let lit = rate >= *step;
            let color = if lit { color } else { theme::c().faint };
            let y = group_bottom - bar_h - g as f32 * (bar_h + gap);
            let rect = egui::Rect::from_min_size(
                egui::pos2(strip_cx - bar_w * 0.5, y),
                egui::vec2(bar_w, bar_h),
            );
            // 点亮格辉光垫底(扩大 1.5px 的低透明层)
            if lit {
                painter.rect_filled(
                    rect.expand2(egui::vec2(1.5, 1.5)),
                    egui::CornerRadius::same(2),
                    color.gamma_multiply(0.28),
                );
            }
            painter.rect_filled(rect, egui::CornerRadius::same(1), color);
        }
    }
}

/// 全显长条:两行 上传速率 / 下载速率(各占半高,水平居中)
fn rate_stack(painter: &egui::Painter, cx: f32, cy: f32, data: &BallData) {
    draw_centered(
        painter,
        cx,
        cy - BAR_H * 0.5 + 6.0,
        format!("{}{}", icons::ARROW_UP, fmt_rate(data.rates.1)),
        theme::c().outbound,
    );
    draw_centered(
        painter,
        cx,
        cy + 4.0,
        format!("{}{}", icons::ARROW_DOWN, fmt_rate(data.rates.0)),
        theme::c().inbound,
    );
}

/// 长条内居中单行文字(galley 测宽后水平居中,top 为行顶)
fn draw_centered(painter: &egui::Painter, cx: f32, top: f32, text: String, color: egui::Color32) {
    let galley = painter.layout_no_wrap(text, egui::FontId::proportional(theme::font::SM), color);
    let w = galley.rect.width();
    painter.galley(egui::pos2(cx - w * 0.5, top), galley, color);
}
