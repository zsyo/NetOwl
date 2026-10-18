//! toast 通知浮层:右下角堆叠(新设备接入、用量配额告警等轻量提示)
//! 与页面底部居中气泡(用户操作反馈,如复制成功)两个通道。
//! 主窗口内 Foreground Area,自动过期;主窗口隐藏时不参与绘制
//! (用户不可见即无意义,不做系统级通知)。

use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, CornerRadius, Margin, RichText, Stroke};

use crate::ui::{icons, theme};

/// 单条展示时长
const TOAST_TTL: Duration = Duration::from_secs(6);
/// 同文本去重窗口(同一设备/阈值在窗口内不重复弹)
const DEDUP_WINDOW: Duration = Duration::from_secs(30);
/// 单条宽度与堆叠步长
const TOAST_W: f32 = 320.0;
const STACK_STEP: f32 = 48.0;
/// 底部气泡离窗口下缘的距离
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

/// 展示位置:右下角堆叠 vs 页面底部居中气泡
#[derive(Clone, Copy, PartialEq)]
enum ToastPos {
    Corner,
    Bottom,
}

pub struct Toast {
    kind: ToastKind,
    text: String,
    at: Instant,
    pos: ToastPos,
    /// 入队序号(单调递增):Area id 用它而非可见项下标——下标会因上一条
    /// 过期而整体前移,id 漂移让 egui 把同一条 toast 当新区块(悬停等
    /// 按 id 的状态瞬断)
    seq: u64,
}

/// 队尾序号 + 1:Toast 生命周期短(去重窗口 30s),队列很小
fn next_seq(toasts: &[Toast]) -> u64 {
    toasts.iter().map(|t| t.seq).max().unwrap_or(0) + 1
}

/// 入队(右下角);同文本同位置在去重窗口内只保留一条。
/// 去重状态的存活期是 DEDUP_WINDOW(30s),长于展示时长 TOAST_TTL(6s):
/// show() 的清理若按 TTL 执行,上一条消失后同文本立刻又能入队,去重
/// 窗口形同虚设。两个位置互不挤占,同文案可各存一条
pub fn push(toasts: &mut Vec<Toast>, kind: ToastKind, text: String) {
    push_at(toasts, kind, text, ToastPos::Corner)
}

/// 入队(页面底部居中气泡):用户操作成功/失败的反馈通道,与右下角
/// 通知语义分离(互不挤占位置),去重口径同 push
pub fn push_bottom(toasts: &mut Vec<Toast>, kind: ToastKind, text: String) {
    push_at(toasts, kind, text, ToastPos::Bottom)
}

fn push_at(toasts: &mut Vec<Toast>, kind: ToastKind, text: String, pos: ToastPos) {
    if toasts
        .iter()
        .any(|t| t.pos == pos && t.text == text && t.at.elapsed() < DEDUP_WINDOW)
    {
        return;
    }
    tracing::info!("[Toast] {text}");
    toasts.push(Toast {
        kind,
        text,
        at: Instant::now(),
        pos,
        seq: next_seq(toasts),
    });
}

/// 绘制 toast 并清理过期项(每帧调用,空队列零开销)。
/// 清理按去重窗口保留(供 push 判重),绘制只画展示时长内的条目;
/// 两个位置各自独立计数堆叠(索引只数本位置的可见项)
pub fn show(ctx: &egui::Context, toasts: &mut Vec<Toast>) {
    toasts.retain(|t| t.at.elapsed() < DEDUP_WINDOW);
    let visible: Vec<&Toast> = toasts
        .iter()
        .filter(|t| t.at.elapsed() < TOAST_TTL)
        .collect();
    if visible.is_empty() {
        return;
    }
    let p = theme::c();
    let corner: Vec<&&Toast> = visible
        .iter()
        .filter(|t| t.pos == ToastPos::Corner)
        .collect();
    let bottom: Vec<&&Toast> = visible
        .iter()
        .filter(|t| t.pos == ToastPos::Bottom)
        .collect();
    for (i, t) in corner.iter().enumerate() {
        draw_one(
            ctx,
            p,
            t,
            "toast",
            i,
            Align2::RIGHT_BOTTOM,
            egui::vec2(-16.0, -16.0),
        );
    }
    for (i, t) in bottom.iter().enumerate() {
        draw_one(
            ctx,
            p,
            t,
            "toast-b",
            i,
            Align2::CENTER_BOTTOM,
            egui::vec2(0.0, -BOTTOM_MARGIN),
        );
    }
}

/// 单条 toast:锚点 + 向上堆叠偏移;两位置仅锚点与 Id 前缀不同
fn draw_one(
    ctx: &egui::Context,
    p: &theme::Palette,
    t: &Toast,
    id_prefix: &str,
    i: usize,
    anchor: Align2,
    base: egui::Vec2,
) {
    let (glyph, accent) = match t.kind {
        ToastKind::Info => (icons::ETHERNET, p.accent),
        ToastKind::Warn => (icons::EXCLAMATION_CIRCLE, p.status_warn),
        ToastKind::Success => (icons::CHECK_CIRCLE, p.status_ok),
    };
    let text = t.text.clone();
    egui::Area::new(egui::Id::unique((id_prefix, t.seq)))
        .order(egui::Order::Foreground)
        .anchor(anchor, base - egui::vec2(0.0, i as f32 * STACK_STEP))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(p.bg_elevated)
                .stroke(Stroke::new(1.0, p.stroke))
                .corner_radius(CornerRadius::same(theme::RADIUS_MD))
                .inner_margin(Margin::same(theme::sp::MD as i8))
                .show(ui, |ui| {
                    ui.set_min_width(TOAST_W);
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
