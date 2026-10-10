//! 悬浮窗绘制:详情浮窗气泡(品牌头 + 总速率 + 进程榜 + 查看详情)
//! 与浮层面板底(菜单窗口共用)。长条速率条在 bar。

use eframe::egui;
use egui::{Align, Label, Layout, RichText, Stroke};

use super::bar::fmt_rate;
use super::types::{BallData, ProcRate};
use crate::i18n::I18n;
use crate::model::fmt_bytes;
use crate::ui::Page;
use crate::ui::{icons, theme, widgets};

/// 圆角浮层面板底(半透明 bg_float + 细描边;菜单窗口共用)
pub(super) fn panel_bg(ui: &mut egui::Ui, rect: egui::Rect) {
    let radius = egui::CornerRadius::same(theme::RADIUS_LG);
    ui.painter().rect_filled(rect, radius, theme::c().bg_float);
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, theme::c().stroke),
        egui::StrokeKind::Inside,
    );
}

/// 详情浮窗(气泡):品牌头 + 总速率 + 实时上传/下载 Top-N 榜单 + 查看详情
pub(super) fn hover_panel(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    data: &BallData,
    i18n: &I18n,
    show_main: &mut Option<Page>,
) {
    panel_bg(ui, rect);
    // 布局约束在气泡 rect 内:弹性贴底与按钮都以气泡底边为界,
    // 不再溢出到窗口底部与球重叠
    let mut panel_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: theme::sp::LG as i8,
            right: theme::sp::LG as i8,
            top: theme::sp::MD as i8,
            bottom: theme::sp::MD as i8,
        })
        .show(&mut panel_ui, |ui| {
            // 品牌头一行:logo + 程序名 + 右侧总上传/下载速率
            ui.horizontal(|ui| {
                if let Some(logo) = &data.logo {
                    ui.add(egui::Image::new(logo).fit_to_exact_size(egui::vec2(18.0, 18.0)));
                }
                ui.label(
                    RichText::new(crate::APP_NAME)
                        .size(theme::font::H3)
                        .strong()
                        .color(theme::c().text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{}{}", icons::ARROW_DOWN, fmt_rate(data.rates.0)))
                            .size(theme::font::XS)
                            .color(theme::c().inbound),
                    );
                    ui.add_space(theme::sp::SM);
                    ui.label(
                        RichText::new(format!("{}{}", icons::ARROW_UP, fmt_rate(data.rates.1)))
                            .size(theme::font::XS)
                            .color(theme::c().outbound),
                    );
                });
            });
            ui.add_space(theme::sp::SM);
            ui.separator();
            ui.add_space(theme::sp::XS);

            if data.elevated {
                section(
                    ui,
                    &i18n.t("ball-realtime-up"),
                    &data.up_top,
                    icons::ARROW_UP,
                    theme::c().outbound,
                    data.default_icon.as_ref(),
                    data.today.1,
                    i18n,
                );
                ui.add_space(theme::sp::SM);
                section(
                    ui,
                    &i18n.t("ball-realtime-down"),
                    &data.down_top,
                    icons::ARROW_DOWN,
                    theme::c().inbound,
                    data.default_icon.as_ref(),
                    data.today.0,
                    i18n,
                );
            } else {
                hint_line(ui, &i18n.t("ball-no-etw"));
            }

            // 按钮区:分隔线 + 按钮,紧跟榜单流式布局(不弹性贴底,
            // 气泡高度以内容校准,多余留白只落在气泡底部)
            ui.add_space(theme::sp::XS);
            ui.separator();
            ui.add_space(theme::sp::SM);
            if ui
                .add_sized(
                    [ui.available_width(), 26.0],
                    egui::Button::new(
                        RichText::new(i18n.t("ball-detail"))
                            .size(theme::font::BODY)
                            .strong()
                            .color(theme::c().on_accent),
                    )
                    .fill(theme::c().accent)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_MD)),
                )
                .clicked()
            {
                *show_main = Some(Page::Connections);
            }
        });
}

/// 榜单分组:标题 + 行(空榜显示占位提示);`today` = 该方向今日
/// 累计总量(本地时区自然日),显示在标题行右端
#[allow(clippy::too_many_arguments)]
fn section(
    ui: &mut egui::Ui,
    title: &str,
    rows: &[ProcRate],
    glyph: &str,
    color: egui::Color32,
    default_icon: Option<&egui::TextureHandle>,
    today: u64,
    i18n: &I18n,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title)
                .size(theme::font::XS)
                .color(theme::c().text_dim),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    i18n.t("ball-total-today"),
                    fmt_bytes(today)
                ))
                .size(theme::font::XS)
                .color(theme::c().text_dim),
            );
        });
    });
    ui.add_space(theme::sp::XS);
    if rows.is_empty() {
        hint_line(ui, &i18n.t("ball-empty"));
        return;
    }
    for r in rows {
        proc_row(ui, r, default_icon, glyph, color);
    }
}

/// 提示行(极小字号,弱化色)
fn hint_line(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(theme::font::XS)
            .color(theme::c().text_dim),
    );
}

/// 榜单行:图标 + 名称(截断)+ 方向速率(右对齐);上传占优行名称
/// 用警示色(与地图页 Inspector 同规则)
fn proc_row(
    ui: &mut egui::Ui,
    r: &ProcRate,
    default_icon: Option<&egui::TextureHandle>,
    glyph: &str,
    color: egui::Color32,
) {
    // 速率列预留宽(下行 "↓123MB/s" 也不截断)
    const RATE_COL_W: f32 = 84.0;
    // 本行展示的速率:上传榜取 up,下载榜取 down(由方向字形色区分)
    let rate = if color == theme::c().outbound {
        r.up
    } else {
        r.down
    };
    ui.horizontal(|ui| {
        let name_w = ui.available_width() - RATE_COL_W;
        ui.allocate_ui(egui::vec2(name_w, 20.0), |ui| {
            ui.set_min_width(name_w);
            ui.horizontal(|ui| {
                widgets::process::proc_icon(ui, r.icon.as_ref(), default_icon, 16.0);
                // 上传占优视为可疑(可能是未经预期的外发流量)
                let warn = r.up > r.down && r.up > 0;
                ui.add(
                    Label::new(RichText::new(&r.name).size(theme::font::SM).color(if warn {
                        theme::c().status_warn
                    } else {
                        theme::c().text
                    }))
                    .wrap_mode(egui::TextWrapMode::Truncate),
                );
            });
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.set_min_width(RATE_COL_W);
            ui.label(
                RichText::new(format!("{glyph}{}", fmt_rate(rate)))
                    .size(theme::font::XS)
                    .color(color),
            );
        });
    });
}
