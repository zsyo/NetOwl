//! 导航底部速率卡:网格纹理底面板 + 双色一分钟走势 + 大数字速率行
//! 与会话累计;布局编排在 nav。

use eframe::egui;
use egui::{Color32, CornerRadius, FontId, Margin, RichText, Stroke};

use crate::i18n::I18n;
use crate::model::fmt_bytes;
use crate::ui::{icons, theme, widgets};

/// 底部速率卡:一分钟双色走势 + 当前速率 + 会话累计
pub(super) fn rate_card(
    ui: &mut egui::Ui,
    i18n: &I18n,
    rates: (u64, u64),
    hist: &[(u64, u64)],
    totals: (u64, u64),
) {
    let p = theme::c();
    egui::Frame::new()
        .fill(p.bg_card)
        .stroke(Stroke::new(1.0, p.stroke))
        .corner_radius(CornerRadius::same(theme::RADIUS_MD))
        .inner_margin(Margin::same(theme::sp::MD as i8))
        .show(ui, |ui| {
            // HUD 网格纹理底:内容垫底的细网格(走势图与文字之下)
            let rect = ui.max_rect();
            let grid = p.stroke.gamma_multiply(0.35);
            let step = 12.0;
            let mut x = rect.left() + step;
            while x < rect.right() {
                ui.painter().line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    Stroke::new(0.5, grid),
                );
                x += step;
            }
            let mut y = rect.top() + step;
            while y < rect.bottom() {
                ui.painter().line_segment(
                    [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                    Stroke::new(0.5, grid),
                );
                y += step;
            }
            widgets::sparkline::sparklines(
                ui,
                hist,
                (p.inbound, p.outbound),
                egui::vec2(ui.available_width(), 34.0),
            );
            ui.add_space(theme::sp::XS);
            rate_row(
                ui,
                i18n,
                "nav-rate-down",
                rates.0,
                p.inbound,
                icons::ARROW_DOWN,
            );
            ui.add_space(theme::sp::XS);
            rate_row(
                ui,
                i18n,
                "nav-rate-up",
                rates.1,
                p.outbound,
                icons::ARROW_UP,
            );
            ui.add_space(theme::sp::XS);
            session_row(ui, i18n, totals);
        });
}

/// 会话累计行:标签 + 双向字节小字
pub(super) fn session_row(ui: &mut egui::Ui, i18n: &I18n, totals: (u64, u64)) {
    let p = theme::c();
    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 3.0;
        ui.label(theme::dim_text(
            &i18n.t("nav-session-total"),
            theme::font::XS,
        ));
        ui.label(
            RichText::new(icons::ARROW_DOWN)
                .size(theme::font::XS)
                .color(p.inbound),
        );
        ui.label(
            RichText::new(fmt_bytes(totals.0))
                .size(theme::font::XS)
                .color(p.text_dim),
        );
        ui.label(
            RichText::new(icons::ARROW_UP)
                .size(theme::font::XS)
                .color(p.outbound),
        );
        ui.label(
            RichText::new(fmt_bytes(totals.1))
                .size(theme::font::XS)
                .color(p.text_dim),
        );
    });
}

/// 一行速率:方向图标 + 标签 + 数值
pub(super) fn rate_row(
    ui: &mut egui::Ui,
    i18n: &I18n,
    key: &str,
    rate: u64,
    color: Color32,
    icon: &str,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).size(theme::font::SM).color(color));
        ui.label(theme::dim_text(&i18n.t(key), theme::font::XS));
        // 面板化大数字:等宽字体保证逐帧刷新不跳宽
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{}/s", fmt_bytes(rate)))
                    .size(theme::font::PANEL_TITLE)
                    .font(FontId::monospace(theme::font::PANEL_TITLE))
                    .strong()
                    .color(color),
            );
        });
    });
}
