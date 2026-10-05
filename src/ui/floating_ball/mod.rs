//! 悬浮窗:贴边常显上/下行总速率,悬浮展开进程流量排行。
//! 条与浮窗是两个独立 viewport:条窗口恒定尺寸(贴边窄条,永不 resize),
//! 浮窗窗口以显隐切换出现/消失——窗口尺寸变化会让 DWM 合成出旧内容裁剪
//! 帧与标题栏重绘帧,显隐则是原子合成,两种切换都无脏帧。
//! 无鼠标穿透(wallwarp 方案):贴边窄条即窗口全部内容,无多屏接缝歧义;
//! 全显静默 REVEAL_GRACE 后,二次移动并停止 EXPAND_DELAY 弹出浮窗
//! (静止维持全显),离开即取消;离开球与浮窗 RETRACT_DELAY 后半隐
//! (期间重入跳过);按住拖动(系统级 StartDrag,OS 模态循环期间不产帧,
//! 恢复渲染即松手)松手按窗口中心吸附左/右屏幕边缘;位置记忆于 config。
//!
//! 类型与刻度常量在 types,交互状态机在 interaction,viewport 定位在
//! layout,速率榜聚合在 data,viewport 帧体在 body。

mod body;
mod data;
mod interaction;
mod layout;
mod menu;
mod types;
mod view;

use std::time::{Duration, Instant};

use eframe::egui;

use crate::i18n::I18n;
use crate::platform::monitor;
use crate::storage::config::FloatingBallConfig;

pub use data::collect_proc_rates;
pub use types::{BallData, BallOutcome, BallState, ProcRate, SnapEdge};

use interaction::cursor_over_windows;
use layout::{dock_pos, menu_pos, panel_pos};
use menu::{MENU_H, MENU_W, MenuAction};
use types::{
    BALL_TITLE, BALL_VIEWPORT_ID, BALL_WIN_H, BALL_WIN_W, HOVER_H, HOVER_W, MENU_TITLE,
    MENU_VIEWPORT_ID, PANEL_TITLE, PANEL_VIEWPORT_ID, Phase,
};

/// 悬浮窗帧入口(主窗口 ui() 末尾调用;主窗口隐藏时仍按低频帧执行)
pub fn show(
    ctx: &egui::Context,
    state: &mut BallState,
    data: &BallData,
    i18n: &I18n,
    cfg: &mut FloatingBallConfig,
) -> BallOutcome {
    // 窗口样式节流检查:Windows 11 给透明圆角窗口画系统边框(方框残边),
    // 需逐 HWND 禁用;同时修正任务栏样式(egui-winit 的 taskbar(false)
    // 未生效)、接管 NC 计算消除 winit undecorated shadow 的顶边 1px
    // 边框;viewport 重建后 HWND 变化,按 1s 节流重设(幂等)
    if state
        .dwm_retry_at
        .is_none_or(|t| t.elapsed() >= Duration::from_secs(1))
    {
        state.dwm_retry_at = Some(Instant::now());
        for title in [BALL_TITLE, PANEL_TITLE, MENU_TITLE] {
            if let Some(hwnd) = monitor::find_window_by_title(title) {
                monitor::remove_dwm_frame(hwnd);
                monitor::fix_toolwindow_style(hwnd);
                monitor::align_floating_style(hwnd);
                monitor::subclass_full_client(hwnd);
            }
        }
    }

    // 几何:球窗口恒为贴边窄条(dock_pos),浮窗窗口尺寸恒定、显隐切换
    // (panel_pos),菜单窗口按贴边方向弹出(menu_pos);均按悬浮条所在
    // 显示器的工作区计算,展开/收起只切换可见性,不发生任何 resize
    let (ball_x, ball_y) = dock_pos(state);
    let (panel_x, panel_y) = panel_pos(state);
    let (menu_x, menu_y) = menu_pos(state);
    let panel_visible = state.phase == Phase::Expanded;
    let menu_visible = state.menu_open;

    // 显示上升沿提升 z 序:自启动实例无前台激活权限,egui-winit 显隐
    // 切换的 ShowWindow 激活失败时窗口只显示不提升 z 序,被激活窗口
    // 压住(手动启动的实例带前台权,激活成功,所以无法复现);HWND_TOP
    // 不激活不抢焦点,与 with_active(false) 的设计一致。窗口延迟创建
    // 时首帧找不到 HWND,保持未置位下一帧重试(悬停期间 30fps)
    if panel_visible
        && !state.panel_raised
        && let Some(hwnd) = monitor::find_window_by_title(PANEL_TITLE)
    {
        monitor::raise_no_activate(hwnd);
        state.panel_raised = true;
    } else if !panel_visible {
        state.panel_raised = false;
    }
    if menu_visible
        && !state.menu_raised
        && let Some(hwnd) = monitor::find_window_by_title(MENU_TITLE)
    {
        monitor::raise_no_activate(hwnd);
        state.menu_raised = true;
    } else if !menu_visible {
        state.menu_raised = false;
    }

    let mut outcome = BallOutcome::default();
    let mut menu_action: Option<MenuAction> = None;

    // 置顶由 builder 条件设置(窗口新建时生效);运行中切换靠菜单动作
    // 里的 WindowLevel 命令对三个窗口即时生效
    let mut panel_builder = egui::ViewportBuilder::default()
        .with_title(PANEL_TITLE)
        .with_decorations(false)
        .with_taskbar(false)
        .with_resizable(false)
        .with_close_button(false)
        .with_active(false)
        .with_transparent(true)
        .with_visible(panel_visible)
        .with_inner_size([HOVER_W, HOVER_H])
        .with_position([panel_x, panel_y]);
    if cfg.always_on_top {
        panel_builder = panel_builder.with_always_on_top();
    }
    ctx.show_viewport_immediate(
        egui::ViewportId(egui::Id::new(PANEL_VIEWPORT_ID)),
        panel_builder,
        |ui, _class| body::panel_body(ui, data, i18n, &mut outcome.show_main),
    );

    // 菜单窗口:自绘右键菜单,不抢焦点(点击菜单项不需要激活)
    let mut menu_builder = egui::ViewportBuilder::default()
        .with_title(MENU_TITLE)
        .with_decorations(false)
        .with_taskbar(false)
        .with_resizable(false)
        .with_close_button(false)
        .with_active(false)
        .with_transparent(true)
        .with_visible(menu_visible)
        .with_inner_size([MENU_W, MENU_H])
        .with_position([menu_x, menu_y]);
    if cfg.always_on_top {
        menu_builder = menu_builder.with_always_on_top();
    }
    ctx.show_viewport_immediate(
        egui::ViewportId(egui::Id::new(MENU_VIEWPORT_ID)),
        menu_builder,
        |ui, _class| menu::menu_body(ui, cfg, i18n, &mut menu_action),
    );

    // 球窗口:三态球面绘制与交互状态机。with_active(false):点击悬浮
    // 条不抢前台(全屏游戏时点击条不会引发全屏切换黑屏);系统拖动走
    // SC_MOUSEMOVE 不依赖窗口焦点
    let mut ball_builder = egui::ViewportBuilder::default()
        .with_title(BALL_TITLE)
        .with_decorations(false)
        .with_taskbar(false)
        .with_resizable(false)
        .with_close_button(false)
        .with_active(false)
        .with_transparent(true)
        .with_inner_size([BALL_WIN_W, BALL_WIN_H])
        .with_position([ball_x, ball_y]);
    if cfg.always_on_top {
        ball_builder = ball_builder.with_always_on_top();
    }
    ctx.show_viewport_immediate(
        egui::ViewportId(egui::Id::new(BALL_VIEWPORT_ID)),
        ball_builder,
        |ui, _class| {
            body::ball_body(
                ui,
                state,
                data,
                panel_visible,
                cfg.auto_hide_edge,
                &mut outcome,
            )
        },
    );

    // 应用菜单动作(在 viewport 闭包之外,借用已释放)
    if let Some(action) = menu_action {
        match action {
            MenuAction::Show(page) => {
                outcome.show_main = Some(page);
                state.menu_open = false;
            }
            MenuAction::ToggleTopmost => {
                cfg.always_on_top = !cfg.always_on_top;
                outcome.config_touched = true;
                // builder 只在窗口新建时生效,运行中切换用命令对三个窗口
                // 即时生效
                let level = if cfg.always_on_top {
                    egui::WindowLevel::AlwaysOnTop
                } else {
                    egui::WindowLevel::Normal
                };
                for id in [BALL_VIEWPORT_ID, PANEL_VIEWPORT_ID, MENU_VIEWPORT_ID] {
                    ctx.send_viewport_cmd_to(
                        egui::ViewportId(egui::Id::new(id)),
                        egui::ViewportCommand::WindowLevel(level),
                    );
                }
            }
            MenuAction::ToggleAutoHide => {
                cfg.auto_hide_edge = !cfg.auto_hide_edge;
                outcome.config_touched = true;
            }
            MenuAction::Close => {
                outcome.close = true;
                state.menu_open = false;
            }
        }
    }

    // 收回倒计时驱动:离开期间维持心跳帧保证按时半隐
    if let Some(left) = state.left_at {
        let remain = types::RETRACT_DELAY.saturating_sub(left.elapsed());
        ctx.request_repaint_after(remain + Duration::from_millis(10));
    }
    // 悬停期间维持约 30fps:静态低帧率(约 500ms)下"按下-移动-释放"
    // 可能整段落在两帧之间,egui 同帧收到按下与释放,拖动判定所需的
    // down 状态失效,窗口便不跟随鼠标
    if cursor_over_windows(ctx, panel_visible, state.menu_open) {
        ctx.request_repaint_after(Duration::from_millis(33));
    }
    outcome
}
