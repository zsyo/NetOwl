//! 滑动开关(iOS 式):替代 checkbox 的启停控件。
//!
//! [`toggle_switch`] 为标准尺寸/强调色的默认形态;自定义颜色、尺寸与
//! 禁用态(阻断开关的警示红、迷你尺寸、置灰)走 [`toggle_switch_styled`]。

use eframe::egui;
use egui::{Color32, CornerRadius, Sense};

use super::super::theme;

/// 标准开关尺寸
const TRACK_W: f32 = 36.0;
const TRACK_H: f32 = 20.0;
/// 滑块与轨道边缘的间隙(滑块直径 = 轨道高 - 2*间隙)
const KNOB_MARGIN: f32 = 3.0;
/// 状态切换动画时长(秒)
const ANIM_SECS: f32 = 0.12;

/// 滑动开关(标准尺寸,开启态为强调色);点击切换 `on`,带平滑过渡动画。
/// `anim_id` 须每实例唯一且跨帧稳定(动画状态按 id 存取;用布局位置
/// 派生的自动 id 会在列表重排/滚动时漂移,动画永不收敛导致高频重绘)
pub fn toggle_switch(ui: &mut egui::Ui, on: &mut bool, anim_id: egui::Id) -> egui::Response {
    toggle_switch_styled(ui, on, theme::c().accent, true, TRACK_W, TRACK_H, anim_id)
}

/// 自定义形态滑动开关:`on_color` 为开启态轨道色,`enabled = false` 时
/// 置灰且不可点击(禁用态语义由调用方用 tooltip 说明)
#[allow(clippy::too_many_arguments)]
pub fn toggle_switch_styled(
    ui: &mut egui::Ui,
    on: &mut bool,
    on_color: Color32,
    enabled: bool,
    track_w: f32,
    track_h: f32,
    anim_id: egui::Id,
) -> egui::Response {
    let p = theme::c();
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(track_w, track_h), Sense::click());
    if enabled {
        if resp.clicked() {
            *on = !*on;
            resp.mark_changed();
        }
    } else {
        resp = resp.on_hover_cursor(egui::CursorIcon::NotAllowed);
    }
    // 进度 0..1 驱动滑块横移与配色过渡
    let t = ui
        .ctx()
        .animate_value_with_time(anim_id, if *on { 1.0 } else { 0.0 }, ANIM_SECS);
    let knob = track_h - 2.0 * KNOB_MARGIN;
    let bg = if !enabled {
        p.faint
    } else if t > 0.5 {
        on_color
    } else {
        p.stroke_strong
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(theme::RADIUS_PILL), bg);

    let travel = track_w - 2.0 * KNOB_MARGIN - knob;
    let knob_x = rect.left() + KNOB_MARGIN + t * travel;
    let knob_center = egui::pos2(knob_x + knob / 2.0, rect.center().y);
    ui.painter().circle_filled(
        knob_center,
        knob / 2.0,
        if enabled { p.on_accent } else { p.text_dim },
    );

    // 关闭态在滑块右侧显示细横线,强化"停用"语义
    if t < 0.5 {
        ui.painter().line_segment(
            [
                egui::pos2(knob_center.x + knob / 2.0 + 3.0, knob_center.y),
                egui::pos2(rect.right() - KNOB_MARGIN - 3.0, knob_center.y),
            ],
            egui::Stroke::new(1.5, p.on_accent.gamma_multiply(0.9)),
        );
    }
    resp
}
