//! 左侧导航栏:品牌区、页面切换(选中指示条 + 过渡动画)、底部速率卡
//! (一分钟双色走势)与监控状态行。

use eframe::egui;
use egui::{Align2, Color32, CornerRadius, FontId, Margin, RichText, Sense, Stroke};

use super::UiCtx;
use crate::collector::CollectorKind;
use crate::i18n::I18n;
use crate::model::{Connection, fmt_bytes};
use crate::ui::{icons, theme, widgets};

/// 左侧导航项:(页面, 词条键, 图标码点(ui::icons,glyph 名见常量注释))
const NAV_ITEMS: &[(super::Page, &str, &str)] = &[
    (super::Page::Map, "nav-map", icons::GLOBE),
    (super::Page::Connections, "nav-connections", icons::LIST_UL),
    (super::Page::History, "nav-history", icons::CLOCK_HISTORY),
    (super::Page::Lan, "nav-lan", icons::ROUTER),
    (super::Page::Rules, "nav-rules", icons::SHIELD),
    (super::Page::Settings, "nav-settings", icons::GEAR),
];

/// 导航项尺寸与选中指示条
const NAV_ITEM_H: f32 = 36.0;
const NAV_INDICATOR_W: f32 = 3.0;
/// 导航选中/悬停过渡动画时长(秒)
const NAV_ANIM_SECS: f32 = 0.15;

/// 导航栏入口:品牌区、页面切换与底部状态区
pub fn nav_ui(
    ui: &mut egui::Ui,
    page: &mut super::Page,
    ctx: &UiCtx,
    logo: &egui::TextureHandle,
    collector_kind: CollectorKind,
) {
    // 品牌区:应用图标圆角块 + 名称/副标题,左对齐
    ui.horizontal(|ui| {
        ui.add(
            egui::Image::new(logo)
                .fit_to_exact_size(egui::vec2(28.0, 28.0))
                .corner_radius(CornerRadius::same(theme::RADIUS_SM)),
        );
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(ctx.i18n.t("app-name"))
                        .size(theme::font::H3)
                        .strong()
                        .color(theme::c().text),
                );
                brand_light(ui);
            });
            ui.label(theme::dim_text(
                &ctx.i18n.t("app-subtitle"),
                theme::font::MICRO,
            ));
        });
    });
    ui.add_space(theme::sp::MD);

    for (target, key, icon) in NAV_ITEMS {
        nav_item(ui, page, *target, &ctx.i18n.t(key), icon);
    }

    // 底部:监控状态在其上,速率卡贴底(bottom_up 先绘制者在底部)
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        // 会话累计 = 当前活跃连接字节总和(程序启动起算,重启归零;
        // 完结连接的字节转入历史库,由历史页统计)
        let totals = (
            ctx.conns.iter().map(|c| c.bytes_in).sum::<u64>(),
            ctx.conns.iter().map(|c| c.bytes_out).sum::<u64>(),
        );
        rate_card(ui, ctx.i18n, ctx.rates, ctx.rate_hist, totals);
        ui.add_space(theme::sp::MD);
        status_rows(ui, ctx.conns, ctx.i18n, collector_kind);
    });
}

/// 导航项:图标 + 文字按钮,选中态底色与左侧强调指示条(高度随过渡动画展开)
fn nav_item(
    ui: &mut egui::Ui,
    page: &mut super::Page,
    target: super::Page,
    label: &str,
    icon: &str,
) {
    let p = theme::c();
    let selected = *page == target;
    let t = ui.ctx().animate_value_with_time(
        egui::Id::new(("nav-item", target)),
        if selected { 1.0 } else { 0.0 },
        NAV_ANIM_SECS,
    );

    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), NAV_ITEM_H), Sense::click());
    let radius = CornerRadius::same(theme::RADIUS_MD);
    if selected {
        ui.painter().rect_filled(rect, radius, p.accent_soft);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, radius, p.hover_bg);
    }
    if t > 0.01 {
        let h = (NAV_ITEM_H - 14.0) * t;
        let bar = egui::Rect::from_center_size(
            egui::pos2(rect.left() + NAV_INDICATOR_W / 2.0, rect.center().y),
            egui::vec2(NAV_INDICATOR_W, h),
        );
        // 辉光层:更宽的低透明 accent 垫底,实心条在其上
        ui.painter().rect_filled(
            bar.expand2(egui::vec2(2.5, 2.0)),
            CornerRadius::same(theme::RADIUS_PILL),
            p.accent.gamma_multiply(0.25),
        );
        ui.painter()
            .rect_filled(bar, CornerRadius::same(theme::RADIUS_PILL), p.accent);
    }
    let text_color = if selected || resp.hovered() {
        p.text
    } else {
        p.text_dim
    };
    let center = rect.center();
    ui.painter().text(
        egui::pos2(rect.left() + 18.0, center.y),
        Align2::CENTER_CENTER,
        icon,
        FontId::proportional(theme::font::BODY),
        if selected { p.accent } else { text_color },
    );
    ui.painter().text(
        egui::pos2(rect.left() + 38.0, center.y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(theme::font::H3),
        text_color,
    );
    if resp.clicked() {
        *page = target;
    }
}

/// 底部速率卡:一分钟双色走势 + 当前速率 + 会话累计
fn rate_card(
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
fn session_row(ui: &mut egui::Ui, i18n: &I18n, totals: (u64, u64)) {
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
fn rate_row(ui: &mut egui::Ui, i18n: &I18n, key: &str, rate: u64, color: Color32, icon: &str) {
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

/// 品牌指示灯:accent 发光小圆(外圈辉光 + 内芯)
fn brand_light(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
    let p = theme::c();
    let c = rect.center();
    ui.painter()
        .circle_filled(c, 5.0, p.accent.gamma_multiply(0.22));
    ui.painter().circle_filled(c, 2.5, p.accent);
}

/// 底部状态:监控中(绿点)、连接数、数据源提示
fn status_rows(ui: &mut egui::Ui, conns: &[Connection], i18n: &I18n, kind: CollectorKind) {
    let p = theme::c();
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, p.status_ok);
        ui.label(theme::dim_text(
            &i18n.t("status-monitoring"),
            theme::font::SM,
        ));
    });
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(icons::ETHERNET)
                .size(theme::font::XS)
                .color(p.text_dim),
        );
        ui.label(theme::dim_text(
            &i18n.t_with_args("status-conn-count", &[("count", conns.len().to_string())]),
            theme::font::XS,
        ));
    });
    // 模拟数据源提示(真实采集时无独立状态行,避免与绿点行重复)
    if kind == CollectorKind::Mock {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(icons::FLASK)
                    .size(theme::font::XS)
                    .color(p.status_warn),
            );
            ui.label(theme::dim_text(&i18n.t("status-mock"), theme::font::XS));
        });
    }
}
