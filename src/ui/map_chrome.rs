//! 地图页标题行 chrome:页头(标题/副标题)+ 左右面板开关与流量图例。
//! 仅地图页使用;面板与 Inspector 本体在 map_panel/map_inspector。

use eframe::egui;
use egui::{Color32, Sense};

use super::{UiCtx, icons, theme, widgets};

/// 地图页标题行:标题、副标题、图例与左右面板开关(开关在图例之后,
/// 从右往左排布)
pub(super) fn map_header(ui: &mut egui::Ui, ctx: &mut UiCtx) {
    widgets::header::page_header_row(
        ui,
        &ctx.i18n.t("map-title"),
        &ctx.i18n.t("map-subtitle"),
        |ui| {
            let panels = &mut *ctx.map_panels;
            panel_toggle(
                ui,
                &mut panels.show_right,
                icons::LAYOUT_TEXT_SIDEBAR_REVERSE,
                &ctx.i18n.t("map-panel-toggle-inspector"),
            );
            panel_toggle(
                ui,
                &mut panels.show_left,
                icons::LAYOUT_SIDEBAR,
                &ctx.i18n.t("map-panel-toggle-list"),
            );
            ui.add_space(theme::sp::MD);
            legend(ui, theme::c().outbound, &ctx.i18n.t("map-legend-out"));
            ui.add_space(theme::sp::SM);
            legend(ui, theme::c().inbound, &ctx.i18n.t("map-legend-in"));
        },
    );
}

/// 面板开关小按钮(图标高亮 = 面板显示)
pub(super) fn panel_toggle(ui: &mut egui::Ui, on: &mut bool, glyph: &str, tip: &str) {
    // 全自绘(替代 frame(false) Button):垫底必须画在图标之前,否则
    // 悬停底色把图标盖住;悬停同时把非选中图标提亮,浅色主题下
    // hover_bg 近白与弱化灰的对比才够
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
    let p = theme::c();
    let hovered = resp.hovered();
    if hovered {
        ui.painter().rect_filled(rect, theme::RADIUS_SM, p.hover_bg);
    }
    let color = if *on {
        p.accent
    } else if hovered {
        p.text
    } else {
        p.text_dim
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(theme::font::H3),
        color,
    );
    let resp = resp
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tip);
    if resp.clicked() {
        *on = !*on;
    }
}

/// 图例:语义色圆点 + 文字
pub(super) fn legend(ui: &mut egui::Ui, color: Color32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
    ui.label(theme::dim_text(label, theme::font::SM));
}
