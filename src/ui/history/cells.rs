//! 历史页共享单元格与行渲染辅助:虚拟化表格的手动斑马行底色、进程/
//! 位置/字节单元格、截断提示与空态。

use std::collections::HashMap;

use eframe::egui;
use egui::{Label, RichText};

use crate::i18n::I18n;
use crate::model::{Place, fmt_bytes};
use crate::net::geoip;
use crate::storage::history_query;
use crate::ui::{theme, widgets};

/// 行底色(虚拟化表格手动斑马):行首调用,悬停高亮优先于奇数行条纹;
/// 色块上下各含半个行距,与 Grid striped 的观感一致,判定区同样含行距;
/// 返回判定区矩形供调用方做整行交互(如汇总行点击下钻)
pub(super) fn row_background(
    ui: &egui::Ui,
    left: f32,
    right: f32,
    top: f32,
    idx: usize,
) -> egui::Rect {
    let rect = egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, top + 22.0))
        .expand2(egui::vec2(0.0, widgets::table::ROW_SPACING_Y * 0.5));
    let bg = if ui.rect_contains_pointer(rect) {
        theme::c().hover_bg
    } else if idx % 2 == 1 {
        theme::c().faint
    } else {
        return rect;
    };
    ui.painter().rect_filled(rect, 0.0, bg);
    rect
}

pub(super) fn truncated_hint(ui: &mut egui::Ui, len: usize, i18n: &I18n) {
    if len >= history_query::QUERY_LIMIT {
        ui.add_space(theme::sp::SM);
        ui.label(theme::dim_text(
            &i18n.t_with_args(
                "history-truncated",
                &[("n", history_query::QUERY_LIMIT.to_string())],
            ),
            theme::font::SM,
        ));
    }
}

pub(super) fn empty_hint(ui: &mut egui::Ui, i18n: &I18n) {
    ui.add_space(theme::sp::XL);
    ui.label(theme::dim_text(&i18n.t("history-empty"), theme::font::H3));
}

/// 字节总量单元格:0 弱化为灰(空载噪音),有值按方向语义色
pub(super) fn bytes_cell(ui: &mut egui::Ui, bytes: u64, outbound: bool) {
    let c = theme::c();
    let color = if bytes == 0 {
        c.text_dim
    } else if outbound {
        c.outbound
    } else {
        c.inbound
    };
    widgets::table::num_cell(ui, fmt_bytes(bytes), color);
}

/// 进程单元格:图标 + 名称(未知进程占位),聚合行无 PID
pub(super) fn proc_cell(
    ui: &mut egui::Ui,
    name: &str,
    pid: Option<u32>,
    path: Option<&str>,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    i18n: &I18n,
) {
    ui.horizontal(|ui| {
        let tex = path.and_then(|p| icon_tex.get(p)).and_then(|t| t.as_ref());
        widgets::process::proc_icon(ui, tex, default_icon_tex, 16.0);
        let text = match (name.is_empty(), pid) {
            (true, Some(pid)) => format!(
                "{} {}",
                i18n.t("conn-proc-unknown"),
                i18n.t_with_args("pid-suffix", &[("pid", pid.to_string())])
            ),
            (true, None) => i18n.t("conn-proc-unknown"),
            (false, Some(pid)) => format!(
                "{name} {}",
                i18n.t_with_args("pid-suffix", &[("pid", pid.to_string())])
            ),
            (false, None) => name.to_owned(),
        };
        ui.add(
            Label::new(
                RichText::new(text)
                    .size(theme::font::BODY)
                    .color(theme::c().text),
            )
            .wrap_mode(egui::TextWrapMode::Truncate),
        );
    });
}

/// 位置单元格:归属地实时反查(geoip 数据重建不影响历史行),未知占位
pub(super) fn location_cell(ui: &mut egui::Ui, i18n: &I18n, ip: std::net::Ipv4Addr) {
    let text = match geoip::locate(ip).map(Place::Geo) {
        Some(p) => geoip::place_label(p, i18n),
        None => i18n.t("conn-loc-unknown"),
    };
    ui.add(
        Label::new(theme::dim_text(&text, theme::font::BODY))
            .wrap_mode(egui::TextWrapMode::Truncate),
    );
}
