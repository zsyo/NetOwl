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
        .inner_margin(Margin::symmetric(8, 3))
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

/// 定宽格内居中徽章:徽章为自适应宽 Frame,egui 的格内居中布局对其
/// 不生效(Frame 实测贴格左),先测文字宽再补前导间距
/// (16.0 = badge 水平内边距 Margin::symmetric(8, ..) 两侧之和)
pub fn badge_centered(ui: &mut egui::Ui, w: f32, h: f32, text: &str, kind: BadgeKind) {
    let badge_w = crate::ui::text_width(ui, text, theme::font::MICRO) + 16.0;
    super::table::fixed_cell(ui, w, h, |ui| {
        let content_w = w - 2.0 * super::table::CELL_PAD_X;
        ui.add_space(((content_w - badge_w) / 2.0).max(0.0));
        badge(ui, text, kind);
    });
}
