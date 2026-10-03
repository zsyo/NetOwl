//! 悬浮窗交互状态机:贴边/全显/浮窗三态流转、菜单开关与系统拖动,
//! 以及系统级光标命中查询与贴边吸附。

use std::time::Instant;

use eframe::egui;

use super::types::{
    BALL_TITLE, BALL_VIEWPORT_ID, BALL_WIN_H, BALL_WIN_W, DRAG_THRESHOLD, EXPAND_DELAY, GEOM_EPSILON,
    MENU_TITLE, MENU_VIEWPORT_ID, PANEL_TITLE, PANEL_VIEWPORT_ID, Phase, REVEAL_GRACE,
    RETRACT_DELAY, SnapEdge,
};
use crate::platform::monitor;
use super::types::BallOutcome;
use super::types::BallState;

/// 鼠标是否在可见的悬浮窗窗口(条/浮窗/菜单)矩形内(系统级光标查询:
/// 窗口矩形与光标坐标均为物理像素;egui 的 interact hover 会被上层可交互
/// widget 截停,透明像素区域也会打断事件流,均不可靠)
pub(super) fn cursor_over_windows(ctx: &egui::Context, panel_visible: bool, menu_visible: bool) -> bool {
    let Some((x, y)) = monitor::cursor_pos_physical() else {
        return false;
    };
    let cur = egui::pos2(x as f32, y as f32);
    let over = |id: &str| {
        ctx.input_for(egui::ViewportId(egui::Id::new(id)), |i| {
            i.viewport().outer_rect
        })
        .is_some_and(|r| r.contains(cur))
    };
    over(BALL_VIEWPORT_ID)
        || (panel_visible && over(PANEL_VIEWPORT_ID))
        || (menu_visible && over(MENU_VIEWPORT_ID))
}

/// 交互状态机:贴边 → hover 滑出全显(静止即保持全显,不弹浮窗)→
/// 全显内二次移动后武装弹出:停止 EXPAND_DELAY 弹出浮窗,移出则取消并
/// 在 RETRACT_DELAY 后回到收起态(auto_hide_edge 关闭时收起态 = 常显,
/// 即不收缩);左键原位释放与右键按下 toggle 菜单,菜单外点击关闭菜单;
/// 拖动交给系统模态循环
pub(super) fn handle_input(
    ui: &mut egui::Ui,
    state: &mut BallState,
    panel_visible: bool,
    auto_hide: bool,
    outcome: &mut BallOutcome,
) {
    let ctx = ui.ctx();
    let hovered = cursor_over_windows(ctx, panel_visible, state.menu_open);
    // 收起态:开启"贴边自动隐藏"时收缩为半隐窄条,否则保持全显常驻
    let rest_phase = if auto_hide {
        Phase::Docked
    } else {
        Phase::Revealed
    };
    let pointer = ctx.pointer_latest_pos();
    let pressed = ui.input(|i| i.pointer.primary_pressed());
    let down = ui.input(|i| i.pointer.primary_down());
    let released = ui.input(|i| i.pointer.primary_released());
    // 鼠标帧间位移:全显态"二次移动并停止"弹出浮窗的移动依据
    // (2px/帧;静默窗期间不采样,见 REVEAL_GRACE)
    let moved = ui.input(|i| i.pointer.delta()).length() > 2.0;

    // 系统拖动结束:StartDrag 的 OS 模态循环期间不产帧,恢复渲染即松手;
    // 按当前窗口实际位置重新贴边并回到收起态
    if state.dragging {
        let cur = ctx
            .input(|i| i.viewport().outer_rect)
            .map(|r| (r.left(), r.top()));
        state.dragging = false;
        state.press_pos = None;
        state.phase = rest_phase;
        state.revealed_at = None;
        state.last_move_at = None;
        state.stationary_seen = false;
        state.left_at = None;
        snap_to_edge(state, cur, outcome);
        return;
    }

    if hovered {
        state.left_at = None;
        match state.phase {
            // 第一段:贴边滑出全显;静止即维持全显,不弹浮窗
            Phase::Docked => {
                state.phase = Phase::Revealed;
                state.revealed_at = Some(Instant::now());
                state.last_move_at = None;
                state.stationary_seen = false;
            }
            // 菜单打开期间冻结浮窗弹出逻辑:二次移动不武装、不弹浮窗
            Phase::Revealed if state.menu_open => {
                state.last_move_at = None;
                state.stationary_seen = false;
            }
            // 第二段:静默窗过后,二次移动 = 从静止发起的移动(进入
            // 滑行尾段不武装);移动中不断重置计时,停止 EXPAND_DELAY
            // 弹出浮窗
            Phase::Revealed => {
                let in_grace = state
                    .revealed_at
                    .is_some_and(|t| t.elapsed() < REVEAL_GRACE);
                if in_grace {
                    state.last_move_at = None;
                    state.stationary_seen = false;
                } else if moved {
                    if state.stationary_seen {
                        state.last_move_at = Some(Instant::now());
                    }
                    state.stationary_seen = false;
                } else {
                    state.stationary_seen = true;
                    if state
                        .last_move_at
                        .is_some_and(|t| t.elapsed() >= EXPAND_DELAY)
                    {
                        state.phase = Phase::Expanded;
                    }
                }
            }
            Phase::Expanded => {}
        }
    } else if state.phase != rest_phase {
        // 离开:取消弹出意图,延迟回到收起态,期间重入(重设 left_at
        // = None)则跳过;auto_hide_edge 关闭时收起态 = 全显,视觉不变
        state.last_move_at = None;
        let left_at = state.left_at.get_or_insert(Instant::now());
        if left_at.elapsed() >= RETRACT_DELAY && state.press_pos.is_none() {
            state.phase = rest_phase;
            state.revealed_at = None;
            state.last_move_at = None;
            state.stationary_seen = false;
            state.left_at = None;
        }
    }

    // 菜单随悬浮条:鼠标离开条与菜单 RETRACT_DELAY 后一并关闭
    // (auto_hide_edge 关闭时常显不收回,菜单仍随离开关闭)
    if state.menu_open && !hovered {
        let left_at = state.left_at.get_or_insert(Instant::now());
        if left_at.elapsed() >= RETRACT_DELAY && state.press_pos.is_none() {
            state.menu_open = false;
        }
    }

    // 菜单交互:条上左键原位释放 / 右键按下均 toggle;菜单外按下关闭
    // (球窗口收不到窗口外的鼠标消息,用系统按键状态检测)
    if state.menu_open
        && (monitor::primary_button_down() || monitor::secondary_button_down())
        && !cursor_over_windows(ctx, panel_visible, true)
    {
        state.menu_open = false;
    }
    if ui.input(|i| i.pointer.secondary_pressed()) {
        toggle_menu(state);
    }

    // 拖动判定:按下后位移超阈值交给系统拖动(浮窗与菜单若在展示则
    // 立即隐藏,拖的始终是悬浮条窗口,拖动结束回收起态);原位释放为
    // 弹出菜单(左键与右键同路径)
    if pressed && let Some(p) = pointer {
        state.press_pos = Some(p);
    }
    if down
        && let (Some(pp), Some(p)) = (state.press_pos, pointer)
        && (p - pp).length() > DRAG_THRESHOLD
    {
        state.press_pos = None;
        state.left_at = None;
        for title in [PANEL_TITLE, MENU_TITLE] {
            if let Some(hwnd) = monitor::find_window_by_title(title) {
                monitor::hide_window(hwnd);
            }
        }
        state.menu_open = false;
        state.dragging = true;
        state.phase = rest_phase;
        state.revealed_at = None;
        state.last_move_at = None;
        state.stationary_seen = false;
        // 系统模态拖动循环在 SendMessage 内运行,松手后返回,
        // 下一帧 dragging 分支按新窗口位置重新贴边
        if let Some(hwnd) = monitor::find_window_by_title(BALL_TITLE) {
            monitor::begin_system_drag(hwnd);
        }
    } else if released && let Some(pp) = state.press_pos.take() {
        // 同帧收到按下与释放(快速动作落在两帧之间)时,位移超阈值是
        // 被丢失的拖动,不当点击弹菜单
        if pointer.is_none_or(|p| (p - pp).length() <= DRAG_THRESHOLD) {
            toggle_menu(state);
        }
    }
}

/// 打开/关闭右键菜单:打开时若浮窗正在展示则先收起(落回全显,
/// 避免浮窗干扰菜单操作),并清空弹出武装
fn toggle_menu(state: &mut BallState) {
    state.menu_open = !state.menu_open;
    if state.menu_open && state.phase == Phase::Expanded {
        state.phase = Phase::Revealed;
        state.revealed_at = None;
        state.last_move_at = None;
        state.stationary_seen = false;
    }
}

/// 按窗口中心所在显示器选边吸附(中心在屏左半贴左缘、右半贴右缘),
/// y 钳制该屏工作区内;吸附位置与记忆位置不同时置 pos_dirty
fn snap_to_edge(state: &mut BallState, cur: Option<(f32, f32)>, outcome: &mut BallOutcome) {
    let (_, y) = cur.unwrap_or(state.pos);
    let center_x = cur.map_or(state.pos.0 + BALL_WIN_W * 0.5, |(x, _)| {
        x + BALL_WIN_W * 0.5
    });
    let (wl, wt, wr, wb) = monitor::workarea_of(center_x, y + BALL_WIN_H * 0.5);
    let edge = if center_x <= (wl + wr) * 0.5 {
        SnapEdge::Left
    } else {
        SnapEdge::Right
    };
    let x = match edge {
        SnapEdge::Left => wl,
        SnapEdge::Right => (wr - BALL_WIN_W).max(wl),
    };
    let y = y.clamp(wt, (wb - BALL_WIN_H).max(wt));
    if (x - state.pos.0).abs() > GEOM_EPSILON || (y - state.pos.1).abs() > GEOM_EPSILON {
        outcome.pos_dirty = true;
    }
    state.pos = (x, y);
    state.edge = Some(edge);
}
