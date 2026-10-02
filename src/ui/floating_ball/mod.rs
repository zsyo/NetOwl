//! 悬浮窗:贴边常显上/下行总速率,悬浮展开进程流量排行。
//! 条与浮窗是两个独立 viewport:条窗口恒定尺寸(贴边窄条,永不 resize),
//! 浮窗窗口以显隐切换出现/消失——窗口尺寸变化会让 DWM 合成出旧内容裁剪
//! 帧与标题栏重绘帧,显隐则是原子合成,两种切换都无脏帧。
//! 无鼠标穿透(wallwarp 方案):贴边窄条即窗口全部内容,无多屏接缝歧义;
//! 全显静默 REVEAL_GRACE 后,二次移动并停止 EXPAND_DELAY 弹出浮窗
//! (静止维持全显),离开即取消;离开球与浮窗 RETRACT_DELAY 后半隐
//! (期间重入跳过);按住拖动(系统级 StartDrag,OS 模态循环期间不产帧,
//! 恢复渲染即松手)松手按窗口中心吸附左/右屏幕边缘;位置记忆于 config。

mod view;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eframe::egui;

use super::conn_visible;
use crate::i18n::I18n;
use crate::model::Connection;
use crate::platform::monitor;
use crate::storage::config::{Config, FloatingBallConfig};

/// 贴边方向
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapEdge {
    Left,
    Right,
}

/// 悬浮球三态:贴边半隐 / 全显(hover 滑出)/ 详情浮窗
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Phase {
    Docked,
    Revealed,
    Expanded,
}

/// 全显控件尺寸(逻辑点):两行速率文字 + 内边距,容纳最长速率形态
/// (高度同时容纳半隐窄条的上下两组信号格与组间隔)
const BAR_W: f32 = 88.0;
const BAR_H: f32 = 52.0;
/// 全显态控件滑出后与屏幕边缘的间隙
const REVEAL: f32 = 10.0;
/// 半隐时露出的窄条宽度(贴边窗口比控件多出的部分;比全显间隙
/// 更宽以保证信号格识别度)
const STRIP_W: f32 = 14.0;
/// 球窗口尺寸:容纳全显控件 + 半隐窄条余量(半隐时控件大部分移出
/// 窗口被裁剪,只露贴边侧 STRIP_W 窄条)
const BALL_WIN_W: f32 = BAR_W + STRIP_W;
const BALL_WIN_H: f32 = BAR_H;
/// 浮窗尺寸(逻辑点;头部一行 + 上/下榜各 TOP_N 行 + 分隔线与按钮
/// 紧跟内容流式排布,实测满布局内容约 302,底部留少量余量)
const HOVER_W: f32 = 264.0;
const HOVER_H: f32 = 332.0;
/// 浮窗与球的间距
const BUBBLE_GAP: f32 = 8.0;
/// 全显态鼠标停止移动后弹出浮窗的静默时长(移动即重置,滑过不弹)
const EXPAND_DELAY: Duration = Duration::from_millis(500);
/// 进入全显后的判定静默窗:吸收触发全显时的反应延迟尾段与手部漂移,
/// 窗口结束后才开始二次移动采样(期间不计移动、不武装)
const REVEAL_GRACE: Duration = Duration::from_millis(500);
/// 信号格档位阈值(字节/秒,对数分档):速率达到第 i 档点亮第 i 格
/// (靠圆心的格先亮)
const RATE_STEPS: [u64; 4] = [1, 8 * 1024, 256 * 1024, 8 * 1024 * 1024];
/// 上传/下载榜单各取前 N
pub(super) const TOP_N: usize = 3;
/// 鼠标离开球与浮窗后半隐延迟(期间重入则跳过)
const RETRACT_DELAY: Duration = Duration::from_millis(500);
/// 拖动判定阈值(逻辑点)
const DRAG_THRESHOLD: f32 = 6.0;
/// 几何命令下发阈值(逻辑点;小于此差异不重发防抖动)
const GEOM_EPSILON: f32 = 0.5;
/// 两个 viewport 的唯一 id 与窗口标题(标题仅作 FindWindow 定位锚,不显示)
const BALL_VIEWPORT_ID: &str = "netowl-floating-ball";
const PANEL_VIEWPORT_ID: &str = "netowl-floating-ball-panel";
const BALL_TITLE: &str = "NetOwl Ball";
const PANEL_TITLE: &str = "NetOwl Ball Panel";

/// 单进程实时速率行(浮窗榜单;icon 为图标纹理快照,绘制不回查 App)
#[derive(Clone)]
pub struct ProcRate {
    pub name: String,
    pub icon: Option<egui::TextureHandle>,
    pub down: u64,
    pub up: u64,
}

/// 悬浮球数据快照(app 层节流聚合,绘制只读;速率均为字节/秒)
#[derive(Default)]
pub struct BallData {
    pub logo: Option<egui::TextureHandle>,
    pub default_icon: Option<egui::TextureHandle>,
    /// 总速率 (下行, 上行)
    pub rates: (u64, u64),
    pub up_top: Vec<ProcRate>,
    pub down_top: Vec<ProcRate>,
    /// 提权可用;否则 ETW 关闭,进程速率不可统计(浮窗显示提示)
    pub elevated: bool,
}

/// show 的交互结果(帧内聚合,由 app 层消费)
#[derive(Default)]
pub struct BallOutcome {
    /// 位置已变化,需回写 config
    pub pos_dirty: bool,
    /// 请求唤出主窗口(点击收起条 / 浮窗"查看详情")
    pub show_main: bool,
}

/// 悬浮球交互状态;持久化位置以 config.floating_ball 为唯一来源,
/// 变更经 BallOutcome::pos_dirty 回写
pub struct BallState {
    /// 当前贴边方向(None = 尚未完成首次贴边)
    edge: Option<SnapEdge>,
    /// 球窗口位置(外框左上角,逻辑点)
    pos: (f32, f32),
    /// 当前交互阶段(贴边/全显/浮窗)
    phase: Phase,
    /// 进入全显的时刻(静默窗 REVEAL_GRACE 计时起点,窗口内不采样)
    revealed_at: Option<Instant>,
    /// 全显内二次移动的最后时刻(停止 EXPAND_DELAY 后弹出浮窗;
    /// None = 未二次移动,静止维持全显不弹)
    last_move_at: Option<Instant>,
    /// 全显后鼠标是否已静止过一帧(二次移动 = 从静止发起的移动;
    /// 进入全显的滑行尾段不算,不武装弹出)
    stationary_seen: bool,
    /// 鼠标离开时刻(None = 指针在球或浮窗上)
    left_at: Option<Instant>,
    /// 按下位置(None = 未按下;超阈值成拖动,原位释放为点击)
    press_pos: Option<egui::Pos2>,
    /// 系统拖动中(StartDrag 已发出)
    dragging: bool,
    /// DWM 去边框下次检查时刻(None = 立即;viewport 重建后随节流重设)
    dwm_retry_at: Option<Instant>,
}

impl BallState {
    pub fn new(cfg: &FloatingBallConfig) -> Self {
        let (wa_w, wa_h) = monitor::workarea_logical();
        let (x, y) = if cfg.x != i32::MIN && cfg.y != i32::MIN {
            (cfg.x as f32, cfg.y as f32)
        } else {
            // 无记忆位置:主屏右缘垂直居中
            (wa_w - BALL_WIN_W, (wa_h - BALL_WIN_H) * 0.5)
        };
        // 贴边方向按记忆位置所在显示器判断(多屏下副屏坐标可为负)
        let (wl, _, wr, _) = monitor::workarea_of(x + BALL_WIN_W * 0.5, y);
        BallState {
            edge: Some(if x + BALL_WIN_W * 0.5 <= (wl + wr) * 0.5 {
                SnapEdge::Left
            } else {
                SnapEdge::Right
            }),
            pos: (x, y),
            phase: Phase::Docked,
            revealed_at: None,
            last_move_at: None,
            stationary_seen: false,
            left_at: None,
            press_pos: None,
            dragging: false,
            dwm_retry_at: None,
        }
    }

    /// 当前收起条位置(逻辑点,回写 config 用)
    pub fn pos(&self) -> (f32, f32) {
        self.pos
    }
}

/// 悬浮球帧入口(主窗口 ui() 末尾调用;主窗口隐藏时仍按低频帧执行)
pub fn show(
    ctx: &egui::Context,
    state: &mut BallState,
    data: &BallData,
    i18n: &I18n,
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
        for title in [BALL_TITLE, PANEL_TITLE] {
            if let Some(hwnd) = monitor::find_window_by_title(title) {
                monitor::remove_dwm_frame(hwnd);
                monitor::fix_toolwindow_style(hwnd);
                monitor::subclass_full_client(hwnd);
            }
        }
    }

    // 几何:球窗口恒为贴边窄条(dock_pos),浮窗窗口尺寸恒定、显隐切换
    // (panel_pos);均按悬浮条所在显示器的工作区计算,展开/收起只切换
    // 浮窗可见性,不发生任何 resize
    let (ball_x, ball_y) = dock_pos(state);
    let (panel_x, panel_y) = panel_pos(state);
    let panel_visible = state.phase == Phase::Expanded;

    let mut outcome = BallOutcome::default();

    // 浮窗窗口:纯展示与查看详情按钮,不抢焦点
    let panel_builder = egui::ViewportBuilder::default()
        .with_title(PANEL_TITLE)
        .with_decorations(false)
        .with_taskbar(false)
        .with_always_on_top()
        .with_resizable(false)
        .with_close_button(false)
        .with_active(false)
        .with_transparent(true)
        .with_visible(panel_visible)
        .with_inner_size([HOVER_W, HOVER_H])
        .with_position([panel_x, panel_y]);
    ctx.show_viewport_immediate(
        egui::ViewportId(egui::Id::new(PANEL_VIEWPORT_ID)),
        panel_builder,
        |ui, _class| panel_body(ui, data, i18n, &mut outcome.show_main),
    );

    // 球窗口:三态球面绘制与交互状态机
    let ball_builder = egui::ViewportBuilder::default()
        .with_title(BALL_TITLE)
        .with_decorations(false)
        .with_taskbar(false)
        .with_always_on_top()
        .with_resizable(false)
        .with_close_button(false)
        // 不设 with_active(false):egui-winit 的 StartDrag 有 has_focus 门控,
        // 不激活的窗口(WS_EX_NOACTIVATE)永远拿不到焦点,系统拖动不会执行
        .with_transparent(true)
        .with_inner_size([BALL_WIN_W, BALL_WIN_H])
        .with_position([ball_x, ball_y]);
    ctx.show_viewport_immediate(
        egui::ViewportId(egui::Id::new(BALL_VIEWPORT_ID)),
        ball_builder,
        |ui, _class| ball_body(ui, state, data, panel_visible, &mut outcome),
    );

    // 收回倒计时驱动:离开期间维持心跳帧保证按时半隐
    if let Some(left) = state.left_at {
        let remain = RETRACT_DELAY.saturating_sub(left.elapsed());
        ctx.request_repaint_after(remain + Duration::from_millis(10));
    }
    outcome
}

/// 浮窗窗口帧体:气泡浮层占满窗口;窗口显隐由 show 按 phase 驱动,
/// 尺寸恒定无 resize,隐藏期间照常绘制保证显示瞬间内容就绪
fn panel_body(ui: &mut egui::Ui, data: &BallData, i18n: &I18n, show_main: &mut bool) {
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
fn ball_body(
    ui: &mut egui::Ui,
    state: &mut BallState,
    data: &BallData,
    panel_visible: bool,
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
            let left = matches!(state.edge, Some(SnapEdge::Left));
            let (bar_cx, mode) = match state.phase {
                Phase::Docked => (
                    if left {
                        STRIP_W - BAR_W * 0.5
                    } else {
                        BALL_WIN_W - STRIP_W + BAR_W * 0.5
                    },
                    view::BallMode::Docked { bars_left: !left },
                ),
                _ => (
                    if left {
                        REVEAL + BAR_W * 0.5
                    } else {
                        BALL_WIN_W - REVEAL - BAR_W * 0.5
                    },
                    view::BallMode::Full,
                ),
            };
            view::ball(ui, bar_cx, BALL_WIN_H * 0.5, data, mode);
            handle_input(ui, state, panel_visible, outcome);
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

/// 鼠标是否在球窗口或可见浮窗窗口的矩形内(系统级光标查询:窗口矩形
/// 与光标坐标均为物理像素;egui 的 interact hover 会被上层可交互 widget
/// 截停,透明像素区域也会打断事件流,均不可靠)
fn cursor_over_windows(ctx: &egui::Context, panel_visible: bool) -> bool {
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
    over(BALL_VIEWPORT_ID) || (panel_visible && over(PANEL_VIEWPORT_ID))
}

/// 交互状态机:贴边 → hover 滑出全显(静止即保持全显,不弹浮窗)→
/// 全显内二次移动后武装弹出:停止 EXPAND_DELAY 弹出浮窗,移出则取消并
/// 在 RETRACT_DELAY 后半隐(持续移动不断重置,滑过不弹;期间重入跳过);
/// 拖动(浮窗态先收浮窗)与点击唤出主窗口
fn handle_input(
    ui: &mut egui::Ui,
    state: &mut BallState,
    panel_visible: bool,
    outcome: &mut BallOutcome,
) {
    let ctx = ui.ctx();
    let hovered = cursor_over_windows(ctx, panel_visible);
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
        state.phase = Phase::Docked;
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
    } else if state.phase != Phase::Docked {
        // 离开:取消弹出意图,延迟半隐,期间重入(重设 left_at = None)则跳过
        state.last_move_at = None;
        let left_at = state.left_at.get_or_insert(Instant::now());
        if left_at.elapsed() >= RETRACT_DELAY && state.press_pos.is_none() {
            state.phase = Phase::Docked;
            state.revealed_at = None;
            state.last_move_at = None;
            state.stationary_seen = false;
            state.left_at = None;
        }
    }

    // 拖动判定:按下后位移超阈值交给系统拖动(浮窗若在展示则立即
    // 隐藏,拖的始终是悬浮条窗口,拖动结束回贴边收起态);原位释放为
    // 点击(收起条 = 主窗口快速入口,浮窗态由面板按钮自理)
    if pressed && let Some(p) = pointer {
        state.press_pos = Some(p);
    }
    if down
        && let (Some(pp), Some(p)) = (state.press_pos, pointer)
        && (p - pp).length() > DRAG_THRESHOLD
    {
        state.press_pos = None;
        state.left_at = None;
        if let Some(hwnd) = monitor::find_window_by_title(PANEL_TITLE) {
            monitor::hide_window(hwnd);
        }
        state.dragging = true;
        state.phase = Phase::Docked;
        state.revealed_at = None;
        state.last_move_at = None;
        state.stationary_seen = false;
        // 系统模态拖动循环在 SendMessage 内运行,松手后返回,
        // 下一帧 dragging 分支按新窗口位置重新贴边
        if let Some(hwnd) = monitor::find_window_by_title(BALL_TITLE) {
            monitor::begin_system_drag(hwnd);
        }
    } else if released && state.press_pos.take().is_some() && state.phase != Phase::Expanded {
        outcome.show_main = true;
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

/// 浮窗窗口位置:贴边侧与球缘对齐(按悬浮条所在显示器的工作区),
/// 球顶余量够则浮窗在球上方,否则在球下方;收起与展开位置一致,
/// 浮窗显隐不伴随窗口移动
fn panel_pos(state: &BallState) -> (f32, f32) {
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
fn dock_pos(state: &BallState) -> (f32, f32) {
    let (wl, wt, wr, wb) = monitor::workarea_of(
        state.pos.0 + BALL_WIN_W * 0.5,
        state.pos.1 + BALL_WIN_H * 0.5,
    );
    let x = state.pos.0.clamp(wl, (wr - BALL_WIN_W).max(wl));
    let y = state.pos.1.clamp(wt, (wb - BALL_WIN_H).max(wt));
    (x, y)
}

/// 每进程实时速率聚合:按映像名汇总 ETW 每连接速率,生成上传/下载
/// Top-N 两榜;过滤口径与连接列表一致(hide_local/hide_lan)。
/// 未提权时 conn_rates 为空,两榜为空——由绘制层提示
pub fn collect_proc_rates(
    conns: &[Connection],
    conn_rates: &HashMap<u64, (u64, u64)>,
    config: &Config,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
) -> (Vec<ProcRate>, Vec<ProcRate>) {
    // 按映像名聚合 (down, up, 首个可用的进程路径[图标查找键])
    let mut acc: HashMap<&str, (u64, u64, Option<String>)> = HashMap::new();
    for c in conns {
        if !conn_visible(config, c) {
            continue;
        }
        let Some((down, up)) = conn_rates.get(&c.id) else {
            continue;
        };
        let e = acc.entry(c.process.as_str()).or_default();
        e.0 += down;
        e.1 += up;
        if e.2.is_none() {
            e.2 = c.proc_path.clone();
        }
    }

    let mut rows: Vec<ProcRate> = acc
        .into_iter()
        .map(|(name, (down, up, path))| ProcRate {
            name: name.to_owned(),
            icon: path
                .as_deref()
                .and_then(|p| icon_tex.get(p))
                .cloned()
                .flatten(),
            down,
            up,
        })
        .collect();
    let mut up_rows = rows.clone();
    up_rows.sort_by(|a, b| b.up.cmp(&a.up).then_with(|| a.name.cmp(&b.name)));
    up_rows.truncate(TOP_N);
    rows.sort_by(|a, b| b.down.cmp(&a.down).then_with(|| a.name.cmp(&b.name)));
    rows.truncate(TOP_N);
    (up_rows, rows)
}
