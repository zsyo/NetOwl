//! 胶囊徽章:语义状态标注(允许/阻断/协议/计数等)。

use eframe::egui;
use egui::{CornerRadius, Margin, RichText};

use super::super::theme;

/// 徽章语义(底色 = 语义色低透明,文字 = 语义色)
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BadgeKind {
    Ok,
    Warn,
    Danger,
    Neutral,
    Accent,
}

/// 胶囊徽章(小号文字,底色随语义)
pub fn badge(ui: &mut egui::Ui, text: &str, kind: BadgeKind) -> egui::Response {
    let p = theme::c();
    let fg = match kind {
        BadgeKind::Ok => p.status_ok,
        BadgeKind::Warn => p.status_warn,
        BadgeKind::Danger => p.danger,
        BadgeKind::Neutral => p.text_dim,
        BadgeKind::Accent => p.accent,
    };
    egui::Frame::new()
        .fill(fg.gamma_multiply(0.16))
        .corner_radius(CornerRadius::same(theme::RADIUS_PILL))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .size(theme::font::MICRO)
                    .strong()
                    .color(fg),
            )
        })
        .response
}
