//! Inspector 概览视图:进程/远端计数、总流量卡与 Top 进程/域名排行,
//! 数据口径与连接列表一致(本地/局域网远端噪音过滤)。

use std::collections::HashMap;

use eframe::egui;

use crate::i18n::I18n;
use crate::model::Connection;
use crate::net::rdns;
use crate::storage::config::Config;
use crate::ui::conn_visible;
use crate::ui::map_panel::{MapPanelState, ProcGroup, RankSort, collect_groups};
use crate::ui::map_widgets::{bytes_row, proc_rank_row, rank_header, traffic_cards};
use crate::ui::theme;

/// 概览:进程/远端计数、总流量与 Top 进程/域名排行(无选中时);
/// 全部数据与连接列表同口径(本地/局域网远端噪音过滤)
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
) {
    ui.heading(theme::accent_text(
        &i18n.t("map-inspector-summary"),
        theme::font::H2,
    ));
    let visible: Vec<&Connection> = conns.iter().filter(|c| conn_visible(config, c)).collect();
    let procs: std::collections::HashSet<&str> =
        visible.iter().map(|c| c.process.as_str()).collect();
    let remotes: std::collections::HashSet<std::net::Ipv4Addr> =
        visible.iter().map(|c| c.remote_ip).collect();
    ui.label(theme::dim_text(
        &i18n.t_with_args(
            "map-inspector-processes",
            &[
                ("count", procs.len().to_string()),
                ("remotes", remotes.len().to_string()),
            ],
        ),
        12.0,
    ));
    traffic_cards(
        ui,
        visible.iter().map(|c| c.bytes_in).sum(),
        visible.iter().map(|c| c.bytes_out).sum(),
        i18n,
    );
    if let Some(s) = rank_header(
        ui,
        &i18n.t("map-inspector-top-proc"),
        panels.proc_rank_sort,
        i18n,
    ) {
        panels.proc_rank_sort = s;
    }
    let mut groups = collect_groups(conns, config, None, "", rdns);
    if groups.is_empty() {
        ui.label(theme::dim_text(&i18n.t("map-panel-empty"), theme::font::SM));
    }
    // 排行按用户选择的排序键重排(累计总量/上传/下载),
    // 占比条与名次同键;次级键按名称稳定序(未提权时字节恒 0)
    let sort = panels.proc_rank_sort;
    let rank_value = |g: &ProcGroup| match sort {
        RankSort::Total => g.total,
        RankSort::Out => g.conns.iter().map(|c| c.bytes_out).sum::<u64>(),
        RankSort::In => g.conns.iter().map(|c| c.bytes_in).sum::<u64>(),
    };
    groups.sort_by(|a, b| {
        rank_value(b)
            .cmp(&rank_value(a))
            .then_with(|| a.name.cmp(&b.name))
    });
    let max_rank = groups.first().map(&rank_value).unwrap_or(0);
    for g in groups.iter().take(5) {
        let ratio = if max_rank > 0 {
            rank_value(g) as f32 / max_rank as f32
        } else {
            0.0
        };
        proc_rank_row(ui, panels, i18n, icon_tex, default_icon_tex, g, ratio);
    }

    if let Some(s) = rank_header(
        ui,
        &i18n.t("map-inspector-top-domain"),
        panels.domain_rank_sort,
        i18n,
    ) {
        panels.domain_rank_sort = s;
    }
    let domain_sort = panels.domain_rank_sort;
    let domains = top_domains(conns, config, rdns, domain_sort, 5);
    let max_rank = domains
        .first()
        .map(|(_, i, o)| rank_bytes(*i, *o, domain_sort))
        .unwrap_or(0);
    for (host, bytes_in, bytes_out) in &domains {
        let ratio = if max_rank > 0 {
            rank_bytes(*bytes_in, *bytes_out, domain_sort) as f32 / max_rank as f32
        } else {
            0.0
        };
        bytes_row(ui, host, *bytes_in, *bytes_out, ratio);
    }
}

/// 排行排序键的字节值(下载/上传二元组按当前排序键取值)
fn rank_bytes(bytes_in: u64, bytes_out: u64, sort: RankSort) -> u64 {
    match sort {
        RankSort::Total => bytes_in + bytes_out,
        RankSort::Out => bytes_out,
        RankSort::In => bytes_in,
    }
}

/// 域名/IP 聚合的流量排行(前 n,按排序键降序);UDP 无远端不计
fn top_domains(
    conns: &[Connection],
    config: &Config,
    rdns: &rdns::Rdns,
    sort: RankSort,
    n: usize,
) -> Vec<(String, u64, u64)> {
    let mut map: HashMap<String, (u64, u64)> = HashMap::new();
    for c in conns.iter().filter(|c| conn_visible(config, c)) {
        if c.remote_ip.is_unspecified() {
            continue;
        }
        let key = match rdns.lookup(c.remote_ip) {
            Some(h) => h.to_owned(),
            None => c.remote_ip.to_string(),
        };
        let entry = map.entry(key).or_insert((0, 0));
        entry.0 += c.bytes_in;
        entry.1 += c.bytes_out;
    }
    let mut rows: Vec<(String, u64, u64)> = map.into_iter().map(|(k, (i, o))| (k, i, o)).collect();
    // 次级键按名称:未提权时字节恒 0,排序键相同的行若不加稳定键,
    // HashMap 迭代序随机导致排行每帧跳动
    rows.sort_by(|a, b| {
        rank_bytes(b.1, b.2, sort)
            .cmp(&rank_bytes(a.1, a.2, sort))
            .then_with(|| a.0.cmp(&b.0))
    });
    rows.truncate(n);
    rows
}
