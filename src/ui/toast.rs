//! 右下角 toast 通知浮层:新设备接入、用量配额告警等轻量提示。
//! 主窗口内 Foreground Area,自动过期,多条向上堆叠;主窗口隐藏时
//! 不参与绘制(用户不可见即无意义,不做系统级通知)。

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

/// 通知语义(图标与强调色)
#[derive(Clone, Copy, PartialEq)]
pub enum ToastKind {
    /// 信息(新设备接入)
    Info,
    /// 警示(用量配额)
    Warn,
}

pub struct Toast {
    kind: ToastKind,
    text: String,
    at: Instant,
}

/// 入队;同文本在去重窗口内只保留一条
pub fn push(toasts: &mut Vec<Toast>, kind: ToastKind, text: String) {
    if toasts
        .iter()
        .any(|t| t.text == text && t.at.elapsed() < DEDUP_WINDOW)
    {
        return;
    }
    tracing::info!("[Toast] {text}");
    toasts.push(Toast {
        kind,
        text,
        at: Instant::now(),
    });
}

/// 绘制右下角堆叠 toast 并清理过期项(每帧调用,空队列零开销)
pub fn show(ctx: &egui::Context, toasts: &mut Vec<Toast>) {
    toasts.retain(|t| t.at.elapsed() < TOAST_TTL);
    if toasts.is_empty() {
        return;
    }
    let p = theme::c();
    for (i, t) in toasts.iter().enumerate() {
        let (glyph, accent) = match t.kind {
            ToastKind::Info => (icons::ETHERNET, p.accent),
            ToastKind::Warn => (icons::EXCLAMATION_CIRCLE, p.status_warn),
        };
        let text = t.text.clone();
        egui::Area::new(egui::Id::new(("toast", i)))
            .order(egui::Order::Foreground)
            .anchor(
                Align2::RIGHT_BOTTOM,
                egui::vec2(-16.0, -16.0 - i as f32 * STACK_STEP),
            )
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
}
