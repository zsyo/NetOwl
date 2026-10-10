//! 悬浮窗三个 viewport 的帧体:浮窗与球窗口的内容绘制与几何对账;
//! 显隐、定位与交互编排在 mod,长条与气泡绘制在 view/bar。

use eframe::egui;

use super::layout::dock_pos;
use super::types::{
    BALL_WIN_H, BALL_WIN_W, GEOM_EPSILON, HOVER_H, HOVER_W, Phase, REVEAL, STRIP_W,
};
use super::{BallData, BallOutcome, BallState};
use super::{bar, interaction, view};
use crate::i18n::I18n;

/// 浮窗窗口帧体:气泡浮层占满窗口;窗口显隐由 show 按 phase 驱动,
/// 尺寸恒定无 resize,隐藏期间照常绘制保证显示瞬间内容就绪
pub(super) fn panel_body(
    ui: &mut egui::Ui,
    data: &BallData,
    i18n: &I18n,
    show_main: &mut Option<crate::ui::Page>,
) {
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::TRANSPARENT)
                .inner_margin(0.0),
        )
        .show(ui, |ui| {
            let rect = ui.max_rect();
            view::hover_panel(ui, rect, data, i18n, show_main);
        });

    // 几何对账:与期望差异超阈值才重发命令(防每帧 SetWindowPos 抖动)
    let ctx = ui.ctx();
    if let Some(r) = ctx.input(|i| i.viewport().outer_rect)
        && ((r.width() - HOVER_W).abs() > GEOM_EPSILON
            || (r.height() - HOVER_H).abs() > GEOM_EPSILON)
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            HOVER_W, HOVER_H,
        )));
    }
}

/// 球窗口帧体:三态球面绘制 + 交互状态机 + 几何对账
pub(super) fn ball_body(
    ui: &mut egui::Ui,
    state: &mut BallState,
    data: &BallData,
    panel_visible: bool,
    auto_hide: bool,
    outcome: &mut BallOutcome,
) {
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::TRANSPARENT)
                .inner_margin(0.0),
        )
        .show(ui, |ui| {
            // 贴边态控件移向贴边侧,只露 STRIP_W 窄条(其余被窗口裁剪),
            // 全显态滑向屏内侧成完整长条(距屏缘 REVEAL)
            let left = matches!(state.edge, Some(super::types::SnapEdge::Left));
            let (bar_cx, mode) = match state.phase {
                Phase::Docked => (
                    if left {
                        STRIP_W - super::types::BAR_W * 0.5
                    } else {
                        BALL_WIN_W - STRIP_W + super::types::BAR_W * 0.5
                    },
                    bar::BallMode::Docked { bars_left: !left },
                ),
                _ => (
                    if left {
                        REVEAL + super::types::BAR_W * 0.5
                    } else {
                        BALL_WIN_W - REVEAL - super::types::BAR_W * 0.5
                    },
                    bar::BallMode::Full,
                ),
            };
            bar::ball(ui, bar_cx, BALL_WIN_H * 0.5, data, mode);
            interaction::handle_input(ui, state, panel_visible, auto_hide, outcome);
        });

    // 几何对账:与期望差异超阈值才重发命令(防每帧 SetWindowPos 抖动)
    let ctx = ui.ctx();
    if let Some(r) = ctx.input(|i| i.viewport().outer_rect) {
        if (r.width() - BALL_WIN_W).abs() > GEOM_EPSILON
            || (r.height() - BALL_WIN_H).abs() > GEOM_EPSILON
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                BALL_WIN_W, BALL_WIN_H,
            )));
        }
        let want = dock_pos(state);
        if (r.left() - want.0).abs() > GEOM_EPSILON || (r.top() - want.1).abs() > GEOM_EPSILON {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                want.0, want.1,
            )));
        }
    }
}
