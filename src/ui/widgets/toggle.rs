//! 滑动开关(iOS 式):替代 checkbox 的启停控件。

use eframe::egui;
use egui::{CornerRadius, Sense};

use super::super::theme;

/// 开关尺寸与滑块
const TRACK_W: f32 = 36.0;
const TRACK_H: f32 = 20.0;
const KNOB: f32 = 14.0;
const KNOB_MARGIN: f32 = 3.0;
/// 状态切换动画时长(秒)
const ANIM_SECS: f32 = 0.12;

/// 滑动开关;点击切换 `on`,带平滑过渡动画
pub fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let p = theme::c();
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(TRACK_W, TRACK_H), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    // 进度 0..1 驱动滑块横移与配色过渡
    let t = ui
        .ctx()
        .animate_value_with_time(resp.id, if *on { 1.0 } else { 0.0 }, ANIM_SECS);
    let bg = if t > 0.5 { p.accent } else { p.stroke_strong };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(theme::RADIUS_PILL), bg);

    let travel = TRACK_W - 2.0 * KNOB_MARGIN - KNOB;
    let knob_x = rect.left() + KNOB_MARGIN + t * travel;
    let knob_center = egui::pos2(knob_x + KNOB / 2.0, rect.center().y);
    ui.painter()
        .circle_filled(knob_center, KNOB / 2.0, p.on_accent);

    // 关闭态在滑块右侧显示细横线,强化"停用"语义
    if t < 0.5 {
        ui.painter().line_segment(
            [
                egui::pos2(knob_center.x + KNOB / 2.0 + 4.0, knob_center.y),
                egui::pos2(rect.right() - KNOB_MARGIN - 4.0, knob_center.y),
            ],
            egui::Stroke::new(1.5, p.on_accent.gamma_multiply(0.9)),
        );
    }
    resp
}
