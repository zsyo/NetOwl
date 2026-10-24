//! toast 通知浮层:右下角牌堆(新设备接入、新监听端口、配额告警等
//! 轻量提示,默认只完整显示最新一张,悬停平铺展开+滚轮回看,见 stack.rs)
//! 与页面底部居中气泡(用户操作反馈,如复制成功)两个通道。
//! 主窗口内 Foreground Area,自动过期;主窗口隐藏时不参与绘制
//! (用户不可见即无意义,不做系统级通知)。

mod stack;

use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, Margin, RichText, Stroke};

use crate::ui::{icons, theme};
pub use stack::StackState;

/// 单条展示时长(牌堆只有堆顶在倒计时,被压住的暂停;底部气泡各自倒计时)
const TOAST_TTL: Duration = Duration::from_secs(6);
/// 同文本去重窗口(同一设备/阈值在窗口内不重复弹)
const DEDUP_WINDOW: Duration = Duration::from_secs(30);
/// 牌堆保留上限:集中风暴(开机一批设备/监听端口)下丢最老,防无限堆积
const MAX_STACK: usize = 8;
/// 底部气泡堆叠步长与离窗口下缘的距离
const STACK_STEP: f32 = 48.0;
const BOTTOM_MARGIN: f32 = 48.0;

/// 通知语义(图标与强调色)
#[derive(Clone, Copy, PartialEq)]
pub enum ToastKind {
    /// 信息(新设备接入)
    Info,
    /// 警示(用量配额)
    Warn,
    /// 操作成功(复制到剪切板等)
    Success,
}

/// 卡片点击跳转动作:点击后该卡消化,frame 层执行页面跳转
#[derive(Clone, Copy)]
pub enum ToastAction {
    /// 跳连接页监听视图,端口填入搜索框筛选
    ListenPort(u16),
    /// 跳局域网设备页(该页无筛选,仅跳转)
    Lan,
    /// 跳历史页用量视图(配额告警)
    Usage,
}

/// 监听提醒副行端点(渲染时 IP 部分末端省略、端口号恒完整)
#[derive(Clone, PartialEq)]
pub struct ListenEndpoint {
    /// 协议名(TCP/UDP,语言无关)
    pub proto: String,
    /// 本地地址原文
    pub ip: String,
    pub port: u16,
}

/// 展示位置:右下角牌堆 vs 页面底部居中气泡
#[derive(Clone, Copy, PartialEq)]
enum ToastPos {
    Corner,
    Bottom,
}

pub struct Toast {
    kind: ToastKind,
    text: String,
    /// 入队时刻(去重窗口判定基准)
    at: Instant,
    /// 剩余展示时长(初始 TOAST_TTL,暂停/消化语义见 show)
    ttl_left: Duration,
    pos: ToastPos,
    /// 入队序号(单调递增):Area id 用它而非可见项下标——下标会因上一条
    /// 过期而整体前移,id 漂移让 egui 把同一条 toast 当新区块(悬停等
    /// 按 id 的状态瞬断)
    seq: u64,
    /// 点击跳转动作(None = 纯展示不可点)
    action: Option<ToastAction>,
    /// 两行卡副行端点(None = 单行卡,内容垂直居中)
    endpoint: Option<ListenEndpoint>,
    /// 两行卡主行类型前缀(如"新监听端口",小号弱色渲染)
    label: Option<String>,
}

/// 队尾序号 + 1:Toast 生命周期短(去重窗口 30s),队列很小
fn next_seq(toasts: &[Toast]) -> u64 {
    toasts.iter().map(|t| t.seq).max().unwrap_or(0) + 1
}

/// 入队(右下角牌堆,纯展示不可点);同位置同内容在去重窗口内只保留
/// 一条(判重键 = 主行文本 + 端点)。两个位置互不挤占
pub fn push(toasts: &mut Vec<Toast>, kind: ToastKind, text: String) {
    push_at(toasts, kind, text, ToastPos::Corner, None, None, None)
}

/// 入队(右下角牌堆,带点击跳转动作)
pub fn push_action(toasts: &mut Vec<Toast>, kind: ToastKind, text: String, action: ToastAction) {
    push_at(
        toasts,
        kind,
        text,
        ToastPos::Corner,
        Some(action),
        None,
        None,
    )
}

/// 入队(右下角牌堆,监听提醒两行卡:主行类型前缀+进程名,副行端点)
pub fn push_listen(
    toasts: &mut Vec<Toast>,
    kind: ToastKind,
    label: String,
    process: String,
    endpoint: ListenEndpoint,
    action: ToastAction,
) {
    push_at(
        toasts,
        kind,
        process,
        ToastPos::Corner,
        Some(action),
        Some(endpoint),
        Some(label),
    )
}

/// 入队(页面底部居中气泡):用户操作成功/失败的反馈通道,与右下角
/// 通知语义分离(互不挤占位置),去重口径同 push
pub fn push_bottom(toasts: &mut Vec<Toast>, kind: ToastKind, text: String) {
    push_at(toasts, kind, text, ToastPos::Bottom, None, None, None)
}

fn push_at(
    toasts: &mut Vec<Toast>,
    kind: ToastKind,
    text: String,
    pos: ToastPos,
    action: Option<ToastAction>,
    endpoint: Option<ListenEndpoint>,
    label: Option<String>,
) {
    // 判重键含端点:监听卡主行只有进程名,同进程不同端口须各自入队
    if toasts.iter().any(|t| {
        t.pos == pos
            && t.text == text
            && t.endpoint.as_ref() == endpoint.as_ref()
            && t.at.elapsed() < DEDUP_WINDOW
    }) {
        return;
    }
    match &endpoint {
        Some(e) => tracing::info!("[Toast] {text} {} {}:{}", e.proto, e.ip, e.port),
        None => tracing::info!("[Toast] {text}"),
    }
    toasts.push(Toast {
        kind,
        text,
        at: Instant::now(),
        ttl_left: TOAST_TTL,
        pos,
        seq: next_seq(toasts),
        action,
        endpoint,
        label,
    });
    // 牌堆上限:仅统计 Corner 条目(bottom 不占额),超限移除最老
    let corner_n = toasts.iter().filter(|t| t.pos == ToastPos::Corner).count();
    if corner_n > MAX_STACK
        && let Some(i) = toasts.iter().position(|t| t.pos == ToastPos::Corner)
    {
        toasts.remove(i);
    }
}

/// 驱动 toast 生命周期并绘制(每帧调用,空队列零开销)。
/// TTL 消化:底部气泡各自倒计时;牌堆悬停时全部暂停,否则只扣堆顶
/// (最新一条),过期后次新顶上继续计时,被压住的一直保留。
/// 条目在展示寿命耗尽后仍保留至去重窗口结束(供 push 判重)。
/// 返回值 = 本帧被点击卡片的跳转动作(被点卡即时消化)
pub fn show(
    ctx: &egui::Context,
    toasts: &mut Vec<Toast>,
    stack: &mut StackState,
) -> Option<ToastAction> {
    let dt = stack.tick();
    for t in toasts.iter_mut() {
        if matches!(t.pos, ToastPos::Bottom) {
            t.ttl_left = t.ttl_left.saturating_sub(dt);
        }
    }
    // 牌堆:上一帧悬停判定暂停计时,只消化堆顶(最后一个可见条目)
    if !stack.hover {
        let mut top: Option<&mut Toast> = None;
        for t in toasts.iter_mut() {
            if matches!(t.pos, ToastPos::Corner) && t.ttl_left > Duration::ZERO {
                top = Some(t);
            }
        }
        if let Some(t) = top {
            t.ttl_left = t.ttl_left.saturating_sub(dt);
        }
    }
    toasts.retain(|t| t.ttl_left > Duration::ZERO || t.at.elapsed() < DEDUP_WINDOW);

    let p = theme::c();
    let corner: Vec<&Toast> = toasts
        .iter()
        .filter(|t| matches!(t.pos, ToastPos::Corner) && t.ttl_left > Duration::ZERO)
        .collect();
    let clicked = if corner.is_empty() {
        stack.reset();
        None
    } else {
        stack::draw(ctx, p, &corner, stack, dt)
    };
    // 被点卡片即时消化(已处理;判重状态仍保留至去重窗口结束)
    if let Some(c) = &clicked {
        for t in toasts.iter_mut() {
            if t.seq == c.seq {
                t.ttl_left = Duration::ZERO;
            }
        }
    }
    for (i, t) in toasts
        .iter()
        .filter(|t| matches!(t.pos, ToastPos::Bottom) && t.ttl_left > Duration::ZERO)
        .enumerate()
    {
        draw_bottom(ctx, p, t, i);
    }
    clicked.map(|c| c.action)
}

/// 底部居中气泡(逐条独立 Area,向上堆叠)
fn draw_bottom(ctx: &egui::Context, p: &theme::Palette, t: &Toast, i: usize) {
    let (glyph, accent) = match t.kind {
        ToastKind::Info => (icons::ETHERNET, p.accent),
        ToastKind::Warn => (icons::EXCLAMATION_CIRCLE, p.status_warn),
        ToastKind::Success => (icons::CHECK_CIRCLE, p.status_ok),
    };
    let text = t.text.clone();
    egui::Area::new(egui::Id::unique(("toast-b", t.seq)))
        .order(egui::Order::Foreground)
        .anchor(
            Align2::CENTER_BOTTOM,
            egui::vec2(0.0, -BOTTOM_MARGIN - i as f32 * STACK_STEP),
        )
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(p.bg_elevated)
                .stroke(Stroke::new(1.0, p.stroke))
                .corner_radius(egui::CornerRadius::same(theme::RADIUS_MD))
                .inner_margin(Margin::same(theme::sp::MD as i8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(glyph).size(theme::font::H3).color(accent));
                        ui.add(
                            egui::Label::new(
                                RichText::new(text).size(theme::font::BODY).color(p.text),
                            )
                            .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                    });
                });
        });
}
