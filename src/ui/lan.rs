//! 局域网设备页:本网段 ARP 发现的设备列表(IP/MAC/最近在线/首见),
//! "新"徽章标注 24 小时内首见设备;行右键复制地址。基础版不做主机名
//! 反查与 MAC 厂商识别。
//!
//! 设备量小(几十台量级),不虚拟化:表头固定在滚动区外,行整体滚动。

use std::net::Ipv4Addr;

use eframe::egui;
use egui::{Label, RichText};

use crate::net::lan::DeviceRow;
use crate::storage::history_query;
use crate::ui::UiCtx;
use crate::ui::theme;
use crate::ui::widgets;

/// 局域网页;返回恒 false(无直接配置改动)
pub(super) fn lan_ui(ui: &mut egui::Ui, ctx: &mut UiCtx) -> bool {
    let devices = ctx.lan_devices;
    let i18n = &mut ctx.i18n;
    widgets::header::page_header(ui, &i18n.t("lan-title"), &i18n.t("lan-subtitle"));
    ui.add_space(theme::sp::SM);

    // 统计行:在线数 / 总数
    let online = devices.iter().filter(|d| d.online).count();
    ui.label(theme::dim_text(
        &i18n.t_with_args(
            "lan-count",
            &[
                ("online", online.to_string()),
                ("total", devices.len().to_string()),
            ],
        ),
        theme::font::BODY,
    ));
    ui.add_space(theme::sp::SM);

    if devices.is_empty() {
        ui.add_space(theme::sp::XL);
        ui.label(theme::dim_text(&i18n.t("lan-empty"), theme::font::H3));
        return false;
    }

    const C_MAC_W: f32 = 160.0;
    const C_SEEN_W: f32 = 112.0;
    const C_STATUS_W: f32 = 120.0;
    let table_w = ui.available_width();
    let flex_w = (table_w - (C_MAC_W + C_SEEN_W * 2.0 + C_STATUS_W)).max(120.0);

    // 表头固定在滚动区外(与历史页同形态)
    widgets::table::header_grid(
        ui,
        "lan_header",
        &[
            ("lan-col-ip", flex_w, false),
            ("lan-col-mac", C_MAC_W, false),
            ("lan-col-last", C_SEEN_W, false),
            ("history-col-first", C_SEEN_W, false),
            ("lan-col-status", C_STATUS_W, false),
        ],
        &|k| i18n.t(k),
    );
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let table_left = ui.max_rect().left();
            let table_right = ui.max_rect().right();
            egui::Grid::new("lan_grid")
                .num_columns(5)
                .striped(true)
                .spacing([0.0, widgets::table::ROW_SPACING_Y])
                .show(ui, |ui| {
                    for d in devices {
                        let row_top = ui.cursor().top();
                        let row_rect = egui::Rect::from_min_max(
                            egui::pos2(table_left, row_top),
                            egui::pos2(table_right, row_top + widgets::table::ROW_H),
                        )
                        .expand2(egui::vec2(0.0, widgets::table::ROW_SPACING_Y * 0.5));
                        let row_resp = ui.interact(
                            row_rect,
                            egui::Id::new(("lan_row", &d.mac)),
                            egui::Sense::click(),
                        );
                        row_resp.context_menu(|ui| device_menu(ui, &d.mac, d.ip, i18n));
                        widgets::table::fixed_cell(ui, flex_w, 18.0, |ui| {
                            ui.add(
                                Label::new(
                                    RichText::new(d.ip.to_string())
                                        .size(theme::font::BODY)
                                        .color(theme::c().text),
                                )
                                .wrap_mode(egui::TextWrapMode::Truncate),
                            );
                        });
                        widgets::table::fixed_cell(ui, C_MAC_W, 18.0, |ui| {
                            ui.add(
                                Label::new(
                                    RichText::new(&d.mac)
                                        .size(theme::font::BODY)
                                        .color(theme::c().text_dim),
                                )
                                .wrap_mode(egui::TextWrapMode::Truncate),
                            );
                        });
                        widgets::table::fixed_cell(ui, C_SEEN_W, 18.0, |ui| {
                            ui.label(theme::dim_text(
                                &history_query::fmt_local(d.last_seen),
                                theme::font::BODY,
                            ));
                        });
                        widgets::table::fixed_cell(ui, C_SEEN_W, 18.0, |ui| {
                            ui.label(theme::dim_text(
                                &history_query::fmt_local(d.first_seen),
                                theme::font::BODY,
                            ));
                        });
                        widgets::table::fixed_cell(ui, C_STATUS_W, 22.0, |ui| {
                            ui.horizontal(|ui| {
                                ui.style_mut().spacing.item_spacing.x = 4.0;
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(10.0, 14.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter()
                                    .circle_filled(rect.center(), 3.0, status_color(d));
                                ui.label(theme::dim_text(
                                    &i18n.t(if d.online {
                                        "lan-online"
                                    } else {
                                        "lan-offline"
                                    }),
                                    theme::font::SM,
                                ));
                                if d.is_new {
                                    widgets::badge::badge(
                                        ui,
                                        &i18n.t("lan-new"),
                                        widgets::badge::BadgeKind::Accent,
                                    );
                                }
                            });
                        });
                        ui.end_row();
                    }
                });
        });
    false
}

/// 在线状态色:在线 status_ok,离线 text_dim
fn status_color(d: &DeviceRow) -> egui::Color32 {
    let p = theme::c();
    if d.online { p.status_ok } else { p.text_dim }
}

/// 设备行右键菜单:复制 IP / 复制 MAC
fn device_menu(ui: &mut egui::Ui, mac: &str, ip: Ipv4Addr, i18n: &crate::i18n::I18n) {
    if widgets::menu::menu_item(ui, i18n.t("lan-copy-ip"), true).clicked() {
        ui.ctx().copy_text(ip.to_string());
    }
    if widgets::menu::menu_item(ui, i18n.t("lan-copy-mac"), true).clicked() {
        ui.ctx().copy_text(mac.to_owned());
    }
}
