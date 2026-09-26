//! 主窗口布局:导航侧栏与各页面(地图 / 连接 / 规则占位 / 设置占位)。

use eframe::egui;
use egui::{Button, Color32, CornerRadius, Frame, Margin, RichText, Stroke};

use crate::map;
use crate::model::{Connection, fmt_bytes};
use crate::text;
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
    (Page::Map, text::NAV_MAP),
    (Page::Connections, text::NAV_CONNECTIONS),
    (Page::Rules, text::NAV_RULES),
    (Page::Settings, text::NAV_SETTINGS),
];

/// 左侧导航栏
pub fn nav_ui(ui: &mut egui::Ui, page: &mut Page, conns: &[Connection]) {
    ui.add_space(4.0);
    ui.label(RichText::new(text::APP_NAME).size(22.0).strong().color(theme::ACCENT));
    ui.label(theme::dim_text(text::APP_SUBTITLE, 10.0));
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(6.0);

    for (target, label) in NAV_ITEMS {
        let selected = page == target;
        let label_text = RichText::new(*label).size(15.0).color(if selected {
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
            ui.label(theme::dim_text(text::STATUS_MONITORING, 13.0));
        });
        ui.label(
            RichText::new(format!("{} · {} 条连接", text::STATUS_MOCK, conns.len()))
                .size(11.0)
                .color(theme::TEXT_DIM),
        );
    });
}

/// 中央区域按页面分发
pub fn central_ui(ui: &mut egui::Ui, page: &Page, conns: &[Connection]) {
    match page {
        Page::Map => map::draw(ui, conns),
        Page::Connections => connections_ui(ui, conns),
        Page::Rules => placeholder_ui(ui, text::RULES_TITLE, text::RULES_PLACEHOLDER),
        Page::Settings => placeholder_ui(ui, text::SETTINGS_TITLE, text::SETTINGS_PLACEHOLDER),
    }
}

/// 连接列表页
fn connections_ui(ui: &mut egui::Ui, conns: &[Connection]) {
    ui.heading(theme::accent_text(text::CONNS_TITLE, 20.0));
    ui.label(theme::dim_text(text::CONNS_SUBTITLE, 13.0));
    ui.add_space(10.0);

    if conns.is_empty() {
        ui.label(theme::dim_text(text::CONNS_EMPTY, 14.0));
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
                    let header = |label: &str, ui: &mut egui::Ui| {
                        ui.label(RichText::new(label).size(12.0).strong().color(theme::TEXT_DIM));
                    };
                    header(text::COL_PROCESS, ui);
                    header(text::COL_PROTO, ui);
                    header(text::COL_REMOTE, ui);
                    header(text::COL_LOCATION, ui);
                    header(text::COL_DOWN, ui);
                    header(text::COL_UP, ui);
                    ui.end_row();

                    for conn in conns {
                        ui.label(RichText::new(&conn.process).size(13.0).color(theme::TEXT));
                        ui.label(theme::dim_text(conn.proto.as_str(), 13.0));
                        ui.label(
                            RichText::new(format!("{}:{}", conn.remote_ip, conn.remote_port))
                                .size(13.0)
                                .color(theme::TEXT),
                        );
                        ui.label(theme::dim_text(crate::collector::city_name(conn.city), 13.0));
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
