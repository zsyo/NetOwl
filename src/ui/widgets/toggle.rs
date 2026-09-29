//! 滑动开关(iOS 式):替代 checkbox 的启停控件。
//!
//! [`toggle_switch`] 为通用启停形态(off 灰底带停用横线 / on 强调色);
//! [`block_switch`] 为阻断语义形态(off 绿 = 放行 / on 红 = 阻断,无停用
//! 横线,置灰 = 被更高层级规则接管)。绘制核心在 [`paint_switch`]。

use eframe::egui;
use egui::{Color32, CornerRadius, Id, Sense};

use super::super::theme;

/// 标准开关尺寸
const TRACK_W: f32 = 36.0;
const TRACK_H: f32 = 20.0;
/// 滑块与轨道边缘的间隙(滑块直径 = 轨道高 - 2*间隙)
const KNOB_MARGIN: f32 = 3.0;
/// 状态切换动画时长(秒)
const ANIM_SECS: f32 = 0.12;

/// 通用启停开关(规则页/设置页):off 灰底横线表示停用,on 强调色
pub fn toggle_switch(ui: &mut egui::Ui, on: &mut bool, anim_id: Id) -> egui::Response {
    paint_switch(
        ui,
        on,
        theme::c().accent,
        theme::c().stroke_strong,
        true,
        true,
        TRACK_W,
        TRACK_H,
        anim_id,
    )
}

/// 阻断开关(地图面板):off 绿色 = 放行中,on 红色 = 阻断中;
/// `enabled = false` 置灰不可点(被更高层级规则接管,语义用 tooltip 说明)
pub fn block_switch(
    ui: &mut egui::Ui,
    on: &mut bool,
    enabled: bool,
    track_w: f32,
    track_h: f32,
    anim_id: Id,
) -> egui::Response {
    paint_switch(
        ui,
        on,
        theme::c().danger,
        theme::c().status_ok,
        false,
        enabled,
        track_w,
        track_h,
        anim_id,
    )
}

/// 开关绘制核心:`off_line` 控制 off 态的停用横线
#[allow(clippy::too_many_arguments)]
fn paint_switch(
    ui: &mut egui::Ui,
    on: &mut bool,
    on_color: Color32,
    off_color: Color32,
    off_line: bool,
    enabled: bool,
    track_w: f32,
    track_h: f32,
    anim_id: Id,
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
    // 进度 0..1 驱动滑块横移与配色过渡;anim_id 须每实例唯一且跨帧稳定
    // (见调用方,布局位置派生的自动 id 在动态列表中漂移会导致高频重绘)
    let t = ui
        .ctx()
        .animate_value_with_time(anim_id, if *on { 1.0 } else { 0.0 }, ANIM_SECS);
    let knob = track_h - 2.0 * KNOB_MARGIN;
    let bg = if !enabled {
        p.faint
    } else if t > 0.5 {
        on_color
    } else {
        off_color
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

    // 关闭态在滑块右侧显示细横线,强化"停用"语义(仅通用启停形态)
    if off_line && t < 0.5 {
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
