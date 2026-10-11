//! 地图页标题行 chrome:页头(标题/副标题)+ 左右面板开关与流量图例。
//! 仅地图页使用;面板与 Inspector 本体在 map_panel/map_inspector。

use eframe::egui;
use egui::{Align2, Color32, FontId, RichText, Sense};

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
            // 图例收纳为一行提示 + 帮助气泡:窄窗口(左右面板全开)下
            // 不与标题块叠印,行高由标题块决定不被撑大;气泡内用正常
            // 字号与彩色圆点展开
            legend_help(ui, ctx);
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

/// 图例入口:一行提示文字 + 帮助图标,悬停弹出气泡分节展开全部图例项
/// (连线方向 / 节点延迟)。延迟节仅采样开启时展示(关闭时节点不着色)。
/// 与 panel_toggle 同构的显式占位自绘:horizontal 容器在 RTL 行内会占满
/// 剩余宽,response 与右侧开关重叠导致 hit-test 失效
fn legend_help(ui: &mut egui::Ui, ctx: &mut UiCtx) {
    let i18n = &*ctx.i18n;
    let quality_on = ctx.config.general.quality_monitor;
    let text = i18n.t("map-legend-help");
    let font = FontId::proportional(theme::font::SM);
    let text_w = ui
        .painter()
        .layout_no_wrap(text.clone(), font.clone(), Color32::WHITE)
        .rect
        .width();
    const ICON_W: f32 = 14.0;
    const GAP: f32 = 5.0;
    let size = egui::vec2(text_w + GAP + ICON_W + 2.0, 20.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let p = theme::c();
    let color = if resp.hovered() { p.text } else { p.text_dim };
    let painter = ui.painter();
    painter.text(
        egui::pos2(rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        &text,
        font.clone(),
        color,
    );
    painter.text(
        egui::pos2(rect.right() - ICON_W * 0.5 - 1.0, rect.center().y),
        Align2::CENTER_CENTER,
        icons::QUESTION_CIRCLE,
        font,
        color,
    );
    resp.on_hover_ui(move |ui| {
        ui.set_max_width(210.0);
        ui.spacing_mut().item_spacing.y = 4.0;
        section_title(ui, &i18n.t("map-legend-dir"));
        ui.horizontal(|ui| {
            legend_row(ui, p.outbound, &i18n.t("map-legend-out"));
            ui.add_space(theme::sp::MD);
            legend_row(ui, p.inbound, &i18n.t("map-legend-in"));
        });
        if quality_on {
            section_title(ui, &i18n.t("map-legend-latency"));
            legend_row(ui, p.status_ok, &i18n.t("map-legend-good"));
            legend_row(ui, p.status_warn, &i18n.t("map-legend-warn"));
            legend_row(ui, p.danger, &i18n.t("map-legend-bad"));
        }
    });
}

/// 气泡内小节标题
fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(theme::font::XS)
            .strong()
            .color(theme::c().text_dim),
    );
}

/// 气泡内单行图例:语义色圆点 + 文字(正常字号)
fn legend_row(ui: &mut egui::Ui, color: Color32, text: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, color);
        ui.label(theme::dim_text(text, theme::font::BODY));
    });
}
