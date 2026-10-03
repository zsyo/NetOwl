//! 悬浮窗三 viewport 几何定位:贴边条位置、浮窗位置(上方/下方择优)
//! 与菜单弹出位置,均按悬浮条所在显示器的工作区钳制。

use super::types::{
    BALL_WIN_H, BALL_WIN_W, BUBBLE_GAP, HOVER_H, HOVER_W, REVEAL, SnapEdge,
};
use super::menu::{MENU_H, MENU_W};
use crate::platform::monitor;

/// 浮窗窗口位置:贴边侧与球缘对齐(按悬浮条所在显示器的工作区),
/// 球顶余量够则浮窗在球上方,否则在球下方;收起与展开位置一致,
/// 浮窗显隐不伴随窗口移动
pub(super) fn panel_pos(state: &super::types::BallState) -> (f32, f32) {
    let (wl, wt, wr, wb) = monitor::workarea_of(
        state.pos.0 + BALL_WIN_W * 0.5,
        state.pos.1 + BALL_WIN_H * 0.5,
    );
    let left = matches!(state.edge, Some(SnapEdge::Left));
    let x = if left {
        wl + REVEAL
    } else {
        (wr - HOVER_W - REVEAL).max(wl)
    };
    let y = if state.pos.1 - wt >= HOVER_H + BUBBLE_GAP {
        // 浮窗在条上方:窗口底 = 条顶 - 间距
        state.pos.1 - BUBBLE_GAP - HOVER_H
    } else {
        // 浮窗在条下方:窗口顶 = 条底 + 间距
        state.pos.1 + BALL_WIN_H + BUBBLE_GAP
    };
    (x, y.clamp(wt, (wb - HOVER_H).max(wt)))
}

/// 贴边/全显态窗口位置(按悬浮条所在显示器的工作区钳制,防越界)
pub(super) fn dock_pos(state: &super::types::BallState) -> (f32, f32) {
    let (wl, wt, wr, wb) = monitor::workarea_of(
        state.pos.0 + BALL_WIN_W * 0.5,
        state.pos.1 + BALL_WIN_H * 0.5,
    );
    let x = state.pos.0.clamp(wl, (wr - BALL_WIN_W).max(wl));
    let y = state.pos.1.clamp(wt, (wb - BALL_WIN_H).max(wt));
    (x, y)
}

/// 菜单窗口位置:贴右缘时弹条左侧、贴左缘时弹条右侧,y 与条顶对齐
/// 后钳制所在显示器工作区内
pub(super) fn menu_pos(state: &super::types::BallState) -> (f32, f32) {
    let (_wl, wt, _wr, wb) = monitor::workarea_of(
        state.pos.0 + BALL_WIN_W * 0.5,
        state.pos.1 + BALL_WIN_H * 0.5,
    );
    let left = matches!(state.edge, Some(SnapEdge::Left));
    let x = if left {
        state.pos.0 + BALL_WIN_W + BUBBLE_GAP
    } else {
        state.pos.0 - MENU_W - BUBBLE_GAP
    };
    let y = state.pos.1.clamp(wt, (wb - MENU_H).max(wt));
    (x, y)
}
