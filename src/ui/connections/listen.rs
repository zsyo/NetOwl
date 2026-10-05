//! 端口监听视图:TCP LISTEN 与 UDP 绑定端点表(进程/协议/本地端点/PID)。
//! 无远端与流量语义,行右键复制本地端点;搜索词过滤进程。

use std::collections::HashMap;

use eframe::egui;
use egui::RichText;

use super::super::{UiCtx, theme, widgets};
use crate::i18n::I18n;
use crate::model::ListenEntry;

/// 监听表(独立表头 + 虚拟化行;搜索词按进程名/路径过滤)
pub(super) fn listen_table(ui: &mut egui::Ui, ctx: &mut UiCtx) {
    let UiCtx {
        listens,
        i18n,
        icon_tex,
        default_icon_tex,
        conn_search,
        ..
    } = ctx;
    let needle = conn_search.trim().to_lowercase();
    let rows: Vec<&ListenEntry> = listens
        .iter()
        .filter(|l| {
            needle.is_empty()
                || l.process.to_lowercase().contains(&needle)
                || l.proc_path
                    .as_deref()
                    .is_some_and(|p| p.to_lowercase().contains(&needle))
        })
        .collect();
    if rows.is_empty() {
        ui.add_space(theme::sp::LG);
        let key = if listens.is_empty() {
            "listen-empty"
        } else {
            "conns-no-match"
        };
        ui.label(theme::dim_text(&i18n.t(key), theme::font::H3));
        return;
    }

    const L_PROTO_W: f32 = 64.0;
    const L_PID_W: f32 = 90.0;
    let table_w = ui.available_width();
    let flex_w = ((table_w - (L_PROTO_W + L_PID_W)) / 2.0).max(220.0);

    widgets::table::header_grid(
        ui,
        "listen_header",
        &[
            ("col-process", flex_w, false),
            ("col-proto", L_PROTO_W, false),
            ("listen-col-local", flex_w, false),
            ("listen-col-pid", L_PID_W, true),
        ],
        &|k| i18n.t(k),
    );
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
        ui,
        widgets::table::ROW_H,
        rows.len(),
        |ui, row_range| {
            egui::Grid::new("listen_rows")
                .num_columns(4)
                .striped(true)
                .spacing([0.0, widgets::table::ROW_SPACING_Y])
                .show(ui, |ui| {
                    for i in row_range {
                        let l = rows[i];
                        listen_row(
                            ui,
                            l,
                            i18n,
                            icon_tex,
                            *default_icon_tex,
                            flex_w,
                            L_PROTO_W,
                            L_PID_W,
                        );
                        ui.end_row();
                    }
                });
        },
    );
}

/// 单行渲染:进程(图标+名+路径)/ 协议徽章 / 本地地址:端口 / PID
#[allow(clippy::too_many_arguments)]
fn listen_row(
    ui: &mut egui::Ui,
    l: &ListenEntry,
    i18n: &I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    flex_w: f32,
    proto_w: f32,
    pid_w: f32,
) {
    widgets::table::fixed_cell(ui, flex_w, widgets::table::ROW_H, |ui| {
        ui.horizontal(|ui| {
            let tex = l
                .proc_path
                .as_deref()
                .and_then(|p| icon_tex.get(p))
                .and_then(|t| t.as_ref());
            widgets::process::proc_icon(ui, tex, default_icon_tex, 16.0);
            ui.add(
                egui::Label::new(
                    RichText::new(&l.process)
                        .size(theme::font::BODY)
                        .color(theme::c().text),
                )
                .wrap_mode(egui::TextWrapMode::Truncate),
            );
        });
    });
    widgets::table::fixed_center_cell(ui, proto_w, widgets::table::ROW_H, |ui| {
        widgets::badge::badge(
            ui,
            l.proto.as_str(),
            if l.proto.as_str() == "TCP" {
                widgets::badge::BadgeKind::Accent
            } else {
                widgets::badge::BadgeKind::Neutral
            },
        );
    });
    widgets::table::fixed_cell(ui, flex_w, widgets::table::ROW_H, |ui| {
        let resp = ui.add(
            egui::Label::new(
                RichText::new(format!("{}:{}", l.local_addr, l.local_port))
                    .size(theme::font::BODY)
                    .color(theme::c().text),
            )
            .wrap_mode(egui::TextWrapMode::Truncate),
        );
        resp.context_menu(|ui| {
            if widgets::menu::menu_item(ui, i18n.t("menu-copy-remote"), true).clicked() {
                ui.ctx()
                    .copy_text(format!("{}:{}", l.local_addr, l.local_port));
            }
        });
    });
    widgets::table::fixed_num_cell(ui, pid_w, |ui| {
        ui.label(
            RichText::new(l.pid.to_string())
                .size(theme::font::BODY)
                .color(theme::c().text_dim),
        );
    });
}
