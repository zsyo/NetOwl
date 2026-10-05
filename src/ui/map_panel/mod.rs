//! 地图页左面板:按进程分组的连接列表,支持端点过滤、搜索与一键阻断。
//! 阻断 = 创建永久 Block 规则(求值未命中默认放行,故无"允许"按钮),
//! 再次点击删除对应规则撤销;WFP 由 App 层轮询自动同步生效。
//!
//! 行内可变长文本统一"固定宽度容器 + Truncate"(宽度扣除同行其余控件):
//! 拉满剩余宽的控件若不设容器会把后续控件挤出面板,经 resizable 面板的
//! 宽度记忆逐帧放大(map_inspector 模块注释)。

mod group_row;

use std::collections::HashSet;

use eframe::egui;
use egui::{Button, Label, RichText, ScrollArea};

use crate::model::{Connection, Place};
use crate::net::geoip;
use crate::net::rdns;
use crate::rules::wfp;
use crate::storage::config::Config;
use crate::ui::UiCtx;
use crate::ui::conn_visible;
use crate::ui::icons;
use crate::ui::map_conn::conn_row;
use crate::ui::theme;

/// 地图页左右面板与选中状态(App 持有,会话态不入 config)
pub struct MapPanelState {
    /// 左侧连接列表面板显隐
    pub show_left: bool,
    /// 右侧详情面板显隐
    pub show_right: bool,
    /// 选中端点:地图点击节点收窄列表与详情
    pub place: Option<Place>,
    /// 选中进程(映像名;右侧详情联动)
    pub process: Option<String>,
    /// 左列表展开的进程组(键 = 映像名;未知进程组为空串)
    pub expanded: HashSet<String>,
    /// 左列表搜索词(过滤进程名/远端 IP/域名)
    pub search: String,
    /// 概览进程排行排序键(与域名排行相互独立)
    pub proc_rank_sort: RankSort,
    /// 概览域名排行排序键
    pub domain_rank_sort: RankSort,
}

impl Default for MapPanelState {
    fn default() -> Self {
        Self {
            show_left: true,
            show_right: true,
            place: None,
            process: None,
            expanded: HashSet::new(),
            search: String::new(),
            proc_rank_sort: RankSort::Total,
            domain_rank_sort: RankSort::Total,
        }
    }
}

/// 概览排行排序键
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RankSort {
    Total,
    Out,
    In,
}

/// 按映像名分组的进程连接(组名空串 = 未知进程);左列表与右侧
/// Inspector 的进程排行共用
pub(crate) struct ProcGroup<'a> {
    pub(crate) name: String,
    pub(crate) conns: Vec<&'a Connection>,
    /// 组内累计流量(排序键)
    pub(crate) total: u64,
}

/// 当前过滤口径下的进程分组:本地/局域网远端噪音过滤(config 持久化,
/// 与连接列表同口径)、端点选中收窄,搜索词命中组名时保留组内全部连接,
/// 否则只保留远端(IP/域名)命中的连接;组按累计流量降序
pub(crate) fn collect_groups<'a>(
    conns: &'a [Connection],
    config: &Config,
    place: Option<Place>,
    search: &str,
    rdns: &rdns::Rdns,
) -> Vec<ProcGroup<'a>> {
    let needle = search.trim().to_lowercase();
    let mut map = std::collections::HashMap::<&str, ProcGroup<'a>>::new();
    for c in conns {
        if !conn_visible(config, c) {
            continue;
        }
        if place.is_some_and(|p| c.city != Some(p)) {
            continue;
        }
        let name = c.process.as_str();
        let keep = needle.is_empty()
            || name.to_lowercase().contains(&needle)
            || rdns
                .lookup(c.remote_ip)
                .is_some_and(|h| h.to_lowercase().contains(&needle))
            || c.remote_ip.to_string().contains(&needle);
        if !keep {
            continue;
        }
        let entry = map.entry(name).or_insert_with(|| ProcGroup {
            name: name.to_owned(),
            conns: Vec::new(),
            total: 0,
        });
        entry.conns.push(c);
        entry.total += c.total_bytes();
    }
    let mut groups: Vec<ProcGroup> = map.into_values().collect();
    // 次级键按名称:HashMap 迭代序随机,total 相同(如未提权时字节恒 0)
    // 的组若不加稳定键,列表顺序每帧跳动
    groups.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.name.cmp(&b.name)));
    // 组内连接按远端四元组排序:表快照顺序不稳定,展开的子行会每秒跳动
    for g in &mut groups {
        g.conns.sort_by(|a, b| {
            (a.remote_ip, a.remote_port, a.local_port).cmp(&(
                b.remote_ip,
                b.remote_port,
                b.local_port,
            ))
        });
    }
    groups
}

/// 左侧连接列表面板(选中端点时仅显示该端点的连接)
pub fn list_panel(ui: &mut egui::Ui, ctx: &mut UiCtx) {
    let panels = &mut *ctx.map_panels;
    let rules = &mut *ctx.rules;
    let i18n: &crate::i18n::I18n = ctx.i18n;
    let conns: &[Connection] = ctx.conns;
    let db = ctx.history_db;
    let rdns = ctx.rdns;
    let icon_tex = ctx.icon_tex;
    let default_icon_tex = ctx.default_icon_tex;
    let config: &Config = ctx.config;
    let wfp_status = &ctx.wfp_status;

    // 端点过滤提示条(地图点选的联动来源,可就地解除)
    if let Some(place) = panels.place {
        ui.horizontal(|ui| {
            ui.style_mut().spacing.item_spacing.x = 4.0;
            ui.style_mut().spacing.button_padding = egui::vec2(2.0, 0.0);
            let text = i18n.t_with_args(
                "map-panel-filter",
                &[("place", geoip::place_label(place, i18n))],
            );
            let clear_w = 16.0;
            let text_w = (ui.available_width() - clear_w - 4.0).max(40.0);
            ui.allocate_ui(egui::vec2(text_w, 14.0), |ui| {
                ui.add(
                    Label::new(
                        RichText::new(text)
                            .size(theme::font::XS)
                            .color(theme::c().accent),
                    )
                    .truncate(),
                );
            });
            let clear = Button::new(
                RichText::new(icons::X_LG)
                    .size(theme::font::MICRO)
                    .color(theme::c().text_dim),
            )
            .frame(false);
            if ui.add(clear).clicked() {
                panels.place = None;
                panels.process = None;
            }
        });
    }

    crate::ui::widgets::search_box::search_box(
        ui,
        &mut panels.search,
        i18n.t("map-panel-search"),
        ui.available_width(),
    );

    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let groups = collect_groups(conns, config, panels.place, &panels.search, rdns);
        if groups.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.label(theme::dim_text(&i18n.t("map-panel-empty"), theme::font::SM));
            });
            return;
        }
        for g in &groups {
            let path = g.conns.iter().find_map(|c| c.proc_path.as_deref());
            group_row::group_row(
                ui,
                panels,
                rules,
                db,
                i18n,
                icon_tex,
                default_icon_tex,
                g,
                path,
            );
            if panels.expanded.contains(&g.name) {
                for (i, c) in g.conns.iter().enumerate() {
                    conn_row(ui, rules, db, i18n, rdns, c, Some(i));
                }
            }
            // 组间分隔线(一级列表行界,子行不画)
            let bottom = ui.cursor().top();
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(ui.max_rect().left(), bottom),
                    egui::pos2(ui.max_rect().right(), bottom + 1.0),
                ),
                0.0,
                theme::c().stroke,
            );
            ui.add_space(theme::sp::SM);
        }
    });

    // 底部固定 WFP 状态提示(阻断生效的前提;生效中不打扰)
    let tip = match wfp_status {
        wfp::Status::Active(_) => None,
        wfp::Status::NoAdmin => Some((i18n.t("wfp-status-noadmin"), theme::c().status_warn)),
        wfp::Status::Failed(e) => Some((
            format!("{}: {e}", i18n.t("wfp-status-failed")),
            theme::c().danger,
        )),
        wfp::Status::Off => Some((i18n.t("wfp-status-off"), theme::c().text_dim)),
    };
    if let Some((text, color)) = tip {
        ui.separator();
        ui.add(Label::new(RichText::new(text).size(theme::font::XS).color(color)).wrap());
    }
}
