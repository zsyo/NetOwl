//! 主窗口布局:导航侧栏与各页面(地图 / 连接 / 规则占位 / 设置)。
//! 全部界面文本经 I18n 词条获取(AGENTS.md 规范 4)。

use eframe::egui;
use egui::{Button, Color32, CornerRadius, Frame, Margin, RichText, Stroke};

use crate::i18n::I18n;
use crate::map;
use crate::model::{Connection, fmt_bytes};
use crate::theme;

/// 主窗口页面
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Map,
    Connections,
    Rules,
    Settings,
}

const NAV_ITEMS: &[(Page, &str)] = &[
    (Page::Map, "nav-map"),
    (Page::Connections, "nav-connections"),
    (Page::Rules, "nav-rules"),
    (Page::Settings, "nav-settings"),
];

/// 左侧导航栏
pub fn nav_ui(ui: &mut egui::Ui, page: &mut Page, conns: &[Connection], i18n: &I18n) {
    ui.add_space(4.0);
    ui.label(RichText::new(i18n.t("app-name")).size(22.0).strong().color(theme::ACCENT));
    ui.label(theme::dim_text(&i18n.t("app-subtitle"), 10.0));
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(6.0);

    for (target, key) in NAV_ITEMS {
        let selected = page == target;
        let label_text = RichText::new(i18n.t(key)).size(15.0).color(if selected {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        });
        let response = ui.add_sized(
            [ui.available_width(), 34.0],
            Button::new(label_text)
                .fill(if selected { theme::ACCENT_SOFT } else { Color32::TRANSPARENT })
                .stroke(if selected {
                    Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.4))
                } else {
                    Stroke::NONE
                })
                .corner_radius(CornerRadius::same(theme::RADIUS_MD)),
        );
        if response.clicked() {
            *page = *target;
        }
    }

    // 底部状态区
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 4.0, theme::STATUS_OK);
            ui.label(theme::dim_text(&i18n.t("status-monitoring"), 13.0));
        });
        ui.label(
            RichText::new(i18n.t_with_args("status-conn-count", &[("count", conns.len().to_string())]))
                .size(11.0)
                .color(theme::TEXT_DIM),
        );
        ui.label(
            RichText::new(i18n.t("status-mock"))
                .size(11.0)
                .color(theme::TEXT_DIM),
        );
    });
}

/// 中央区域按页面分发
pub fn central_ui(ui: &mut egui::Ui, page: &Page, conns: &[Connection], i18n: &mut I18n) {
    match page {
        Page::Map => map::draw(ui, conns, i18n),
        Page::Connections => connections_ui(ui, conns, i18n),
        Page::Rules => placeholder_ui(
            ui,
            &i18n.t("rules-title"),
            &i18n.t("rules-placeholder"),
        ),
        Page::Settings => settings_ui(ui, i18n),
    }
}

/// 连接列表页
fn connections_ui(ui: &mut egui::Ui, conns: &[Connection], i18n: &I18n) {
    ui.heading(theme::accent_text(&i18n.t("conns-title"), 20.0));
    ui.label(theme::dim_text(&i18n.t("conns-subtitle"), 13.0));
    ui.add_space(10.0);

    if conns.is_empty() {
        ui.label(theme::dim_text(&i18n.t("conns-empty"), 14.0));
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            egui::Grid::new("connections_grid")
                .num_columns(6)
                .spacing([24.0, 9.0])
                .striped(true)
                .show(ui, |ui| {
                    for key in ["col-process", "col-proto", "col-remote", "col-location", "col-down", "col-up"] {
                        ui.label(RichText::new(i18n.t(key)).size(12.0).strong().color(theme::TEXT_DIM));
                    }
                    ui.end_row();

                    for conn in conns {
                        ui.label(RichText::new(&conn.process).size(13.0).color(theme::TEXT));
                        ui.label(theme::dim_text(conn.proto.as_str(), 13.0));
                        ui.label(
                            RichText::new(format!("{}:{}", conn.remote_ip, conn.remote_port))
                                .size(13.0)
                                .color(theme::TEXT),
                        );
                        ui.label(theme::dim_text(&i18n.t(&format!("city-{}", conn.city)), 13.0));
                        ui.label(
                            RichText::new(fmt_bytes(conn.bytes_in)).size(13.0).color(theme::INBOUND),
                        );
                        ui.label(
                            RichText::new(fmt_bytes(conn.bytes_out)).size(13.0).color(theme::OUTBOUND),
                        );
                        ui.end_row();
                    }
                });
        });
}

/// 设置页:语言切换(词条即时生效)
fn settings_ui(ui: &mut egui::Ui, i18n: &mut I18n) {
    ui.heading(theme::accent_text(&i18n.t("settings-title"), 20.0));
    ui.add_space(16.0);
    ui.label(RichText::new(i18n.t("settings-language")).size(14.0).strong().color(theme::TEXT));
    ui.add_space(4.0);

    let current_name = i18n
        .available_langs
        .iter()
        .find(|info| info.code == i18n.current_lang)
        .map(|info| info.name.clone())
        .unwrap_or_else(|| i18n.current_lang.clone());
    egui::ComboBox::from_id_salt("settings-language-select")
        .width(180.0)
        .selected_text(current_name)
        .show_ui(ui, |ui| {
            for (code, name) in i18n.lang_codes_and_names() {
                let selected = code == i18n.current_lang;
                let label = RichText::new(name).size(14.0).color(if selected {
                    theme::ACCENT
                } else {
                    theme::TEXT
                });
                if ui.selectable_label(selected, label).clicked() {
                    i18n.set_language(code);
                }
            }
        });
    ui.add_space(6.0);
    ui.label(theme::dim_text(&i18n.t("settings-language-hint"), 12.0));
}

/// 占位页统一卡片
fn placeholder_ui(ui: &mut egui::Ui, title: &str, body: &str) {
    ui.heading(theme::accent_text(title, 20.0));
    ui.add_space(20.0);
    Frame::new()
        .fill(theme::BG_CARD)
        .stroke(Stroke::new(1.0, theme::STROKE))
        .corner_radius(CornerRadius::same(theme::RADIUS_LG))
        .inner_margin(Margin::same(20))
        .show(ui, |ui| {
            ui.set_max_width(440.0);
            ui.label(RichText::new(body).size(13.0).color(theme::TEXT_DIM));
        });
}
