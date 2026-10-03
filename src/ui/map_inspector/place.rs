//! Inspector 端点详情视图:位置名、连接数、双向流量与相关进程排行。

use std::collections::HashMap;

use eframe::egui;

use super::title_row;
use crate::i18n::I18n;
use crate::model::{Connection, Place};
use crate::net::geoip;
use crate::net::rdns;
use crate::storage::config::Config;
use crate::ui::map_panel::{MapPanelState, collect_groups};
use crate::ui::map_widgets::{proc_rank_row, section_title, traffic_cards};
use crate::ui::theme;

/// 端点详情:位置名、连接数、双向流量与相关进程排行
#[allow(clippy::too_many_arguments)]
pub(super) fn view(
    ui: &mut egui::Ui,
    panels: &mut MapPanelState,
    i18n: &I18n,
    rdns: &rdns::Rdns,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    config: &Config,
    conns: &[Connection],
    place: Place,
) {
    if title_row(
        ui,
        &geoip::place_label(place, i18n),
        None,
        i18n.t("map-inspector-clear"),
    ) {
        panels.place = None;
        panels.process = None;
        return;
    }
    // 端点定位自地图聚合,远端必有归属,连接数/流量按端点收窄即无需噪音过滤
    let rows: Vec<&Connection> = conns.iter().filter(|c| c.city == Some(place)).collect();
    ui.label(theme::dim_text(
        &i18n.t_with_args("status-conn-count", &[("count", rows.len().to_string())]),
        theme::font::SM,
    ));
    traffic_cards(
        ui,
        rows.iter().map(|c| c.bytes_in).sum(),
        rows.iter().map(|c| c.bytes_out).sum(),
        i18n,
    );

    section_title(ui, &i18n.t("map-inspector-procs"));
    let groups = collect_groups(conns, config, Some(place), "", rdns);
    let max_total = groups.first().map(|g| g.total).unwrap_or(0);
    for g in &groups {
        let ratio = if max_total > 0 {
            g.total as f32 / max_total as f32
        } else {
            0.0
        };
        proc_rank_row(ui, panels, i18n, icon_tex, default_icon_tex, g, ratio);
    }
}
