//! 连接列表行渲染:进程两行列(图标 + 签名/路径)、协议徽章、rDNS
//! 域名两行列、归属、速率与累计字节、规则求值动作徽章,以及行右键
//! 菜单挂接与行悬停垫底。

use std::collections::HashMap;

use eframe::egui;
use egui::{Label, RichText};

use super::menu;
use crate::i18n::I18n;
use crate::model::{Connection, Signing, fmt_bytes};
use crate::net::rdns;
use crate::rules as rules_engine;
use crate::ui::{theme, widgets};

/// 单行渲染(行悬停垫底 + 右键菜单 + 九列内容)
#[allow(clippy::too_many_arguments)]
pub(super) fn conn_row(
    ui: &mut egui::Ui,
    conn: &Connection,
    i18n: &I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    rdns: &rdns::Rdns,
    conn_rates: &HashMap<u64, (u64, u64)>,
    rules: &mut rules_engine::RuleSet,
    conn_row_hover: &mut widgets::table::RowHover,
    table_left: f32,
    table_right: f32,
    flex_w: f32,
    elevated: bool,
) {
    const C_PROTO_W: f32 = 64.0;
    const C_LOC_W: f32 = 112.0;
    const C_RATE_W: f32 = 90.0;
    const C_TOTAL_W: f32 = 90.0;
    const C_ACTION_W: f32 = 70.0;
    let row_top = ui.cursor().top();
    conn_row_hover.begin(ui, table_left, table_right, row_top);
    // 行整格右键菜单(Sense::click 使行级 hit-test 胜过
    // 行内仅 hover 的文本控件);左键无动作
    let row_rect = egui::Rect::from_min_max(
        egui::pos2(table_left, row_top),
        egui::pos2(table_right, row_top + 36.0),
    )
    .expand2(egui::vec2(0.0, widgets::table::ROW_SPACING_Y * 0.5));
    let row_resp = ui.interact(
        row_rect,
        egui::Id::new(("conn_row", conn.id)),
        egui::Sense::click(),
    );
    row_resp.context_menu(|ui| menu::conn_menu(ui, conn, elevated, i18n));
    let process = if conn.process.is_empty() {
        format!("{} (PID {})", i18n.t("conn-proc-unknown"), conn.pid)
    } else {
        conn.process.clone()
    };
    // 进程列两行:映像名(带图标)+ 弱化的签名状态与路径。
    // 无图标的进程也占位,保证各行文字起点对齐不跳动;
    // 格高 36 与两行内容同高(顶对齐无缝),长文本 Truncate
    widgets::table::fixed_cell(ui, flex_w, 36.0, |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                let tex = conn
                    .proc_path
                    .as_deref()
                    .and_then(|p| icon_tex.get(p))
                    .and_then(|t| t.as_ref());
                widgets::process::proc_icon(ui, tex, default_icon_tex, 16.0);
                ui.add(
                    Label::new(
                        RichText::new(process)
                            .size(theme::font::BODY)
                            .color(theme::c().text),
                    )
                    .wrap_mode(egui::TextWrapMode::Truncate),
                );
            });
            ui.add(
                Label::new(theme::dim_text(&proc_detail(conn, i18n), theme::font::XS))
                    .wrap_mode(egui::TextWrapMode::Truncate),
            );
        });
    });
    // 协议徽章:TCP 强调 / UDP 弱化
    let proto_kind = if conn.proto.as_str() == "TCP" {
        widgets::badge::BadgeKind::Accent
    } else {
        widgets::badge::BadgeKind::Neutral
    };
    widgets::table::fixed_cell(ui, C_PROTO_W, 22.0, |ui| {
        widgets::badge::badge(ui, conn.proto.as_str(), proto_kind);
    });
    // rDNS 域名优先:两行(域名 + 弱化的裸 IP),格高 36
    // 与两行内容同高;无 PTR 时内容本就是 ip:port,单行
    // 直接由格的垂直居中承载,不再套两行结构
    widgets::table::fixed_cell(ui, flex_w, 36.0, |ui| match rdns.lookup(conn.remote_ip) {
        Some(host) => {
            ui.vertical(|ui| {
                ui.add(
                    Label::new(
                        RichText::new(rdns::display(host, conn.remote_port, 36))
                            .size(theme::font::BODY)
                            .color(theme::c().text),
                    )
                    .wrap_mode(egui::TextWrapMode::Truncate),
                );
                ui.add(
                    Label::new(theme::dim_text(
                        &conn.remote_ip.to_string(),
                        theme::font::XS,
                    ))
                    .wrap_mode(egui::TextWrapMode::Truncate),
                );
            });
        }
        None => {
            ui.add(
                Label::new(
                    RichText::new(conn.remote_display())
                        .size(theme::font::BODY)
                        .color(theme::c().text),
                )
                .wrap_mode(egui::TextWrapMode::Truncate),
            );
        }
    });
    let location = match conn.city {
        Some(place) => crate::net::geoip::place_label(place, i18n),
        None => i18n.t("conn-loc-unknown"),
    };
    widgets::table::fixed_cell(ui, C_LOC_W, 18.0, |ui| {
        ui.add(
            Label::new(theme::dim_text(&location, theme::font::BODY))
                .wrap_mode(egui::TextWrapMode::Truncate),
        );
    });
    // 下载/上传列显示实时速率(ETW 字节差值),悬停显示累计字节;
    // 未提权时 ETW 未启动,速率恒 0
    let (rin, rout) = conn_rates.get(&conn.id).copied().unwrap_or((0, 0));
    widgets::table::fixed_num_cell(ui, C_RATE_W, |ui| {
        widgets::table::num_cell(ui, format!("{}/s", fmt_bytes(rin)), theme::c().inbound)
            .on_hover_text(format!(
                "{} {}",
                i18n.t("conn-total-bytes"),
                fmt_bytes(conn.bytes_in)
            ));
    });
    widgets::table::fixed_num_cell(ui, C_RATE_W, |ui| {
        widgets::table::num_cell(ui, format!("{}/s", fmt_bytes(rout)), theme::c().outbound)
            .on_hover_text(format!(
                "{} {}",
                i18n.t("conn-total-bytes"),
                fmt_bytes(conn.bytes_out)
            ));
    });
    // 累计字节列(速率列的悬停信息在此显式展示)
    widgets::table::fixed_num_cell(ui, C_TOTAL_W, |ui| {
        widgets::table::num_cell(ui, fmt_bytes(conn.bytes_in), theme::c().inbound);
    });
    widgets::table::fixed_num_cell(ui, C_TOTAL_W, |ui| {
        widgets::table::num_cell(ui, fmt_bytes(conn.bytes_out), theme::c().outbound);
    });
    // 规则求值:命中规则的连接标注动作徽章,未命中默认放行不标注
    widgets::table::fixed_cell(ui, C_ACTION_W, 22.0, |ui| {
        match rules.evaluate(&rules_engine::MatchReq::from_conn(
            conn,
            rdns.lookup(conn.remote_ip),
        )) {
            Some(hit) => {
                let (key, kind) = match hit.action {
                    rules_engine::Action::Allow => {
                        ("conn-action-allow", widgets::badge::BadgeKind::Ok)
                    }
                    rules_engine::Action::Block => {
                        ("conn-action-block", widgets::badge::BadgeKind::Danger)
                    }
                };
                widgets::badge::badge(ui, &i18n.t(key), kind);
            }
            None => {
                ui.label("");
            }
        }
    });
    ui.end_row();
    conn_row_hover.end(ui, row_top);
}

/// 进程列第二行文本:签名状态 + 映像路径(超长取尾部保留文件名)
fn proc_detail(conn: &Connection, i18n: &I18n) -> String {
    let sign = match conn.signed {
        Signing::Signed => i18n.t("proc-signed"),
        Signing::Unsigned => i18n.t("proc-unsigned"),
        Signing::Invalid => i18n.t("proc-sign-invalid"),
        Signing::Unknown => i18n.t("proc-sign-unknown"),
    };
    match &conn.proc_path {
        Some(path) => format!("{sign} · {}", tail_path(path, 52)),
        None => format!("{sign} · {}", i18n.t("proc-path-unknown")),
    }
}

/// 路径超长时截取尾部("…" 前缀),保住文件名部分
fn tail_path(path: &str, max: usize) -> String {
    let chars: Vec<char> = path.chars().collect();
    if chars.len() <= max {
        return path.to_owned();
    }
    let tail: String = chars[chars.len() - (max - 1)..].iter().collect();
    format!("…{tail}")
}
