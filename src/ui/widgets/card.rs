//! 卡片容器:标准浮起卡片(卡片底 + 描边 + 圆角)。

use eframe::egui;
use egui::{CornerRadius, Margin};

use super::super::theme;

/// 标准卡片:背景卡片色 + 1px 描边 + 中号圆角 + 中号内边距
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(theme::c().bg_card)
        .stroke(egui::Stroke::new(1.0, theme::c().stroke))
        .corner_radius(CornerRadius::same(theme::RADIUS_MD))
        .inner_margin(Margin::same(theme::sp::MD as i8))
        .show(ui, add)
}
