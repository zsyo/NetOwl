//! 地图页右面板(Inspector):概览、端点详情与进程详情三态。
//! 无选中时显示全局概览(计数、总流量与 Top 进程/域名);选中端点
//! 显示该端点的相关进程;选中进程显示路径、签名、阻断开关与连接明细。
//! 选中态由地图端点点击与左列表进程点击驱动。
//!
//! 行内可变长文本统一"固定宽度容器 + Truncate":拉满剩余宽的控件若
//! 不设容器会把后续控件挤出面板,经 resizable 面板的宽度记忆逐帧放大
//! (面板宽度记忆取自内容矩形)。

use std::collections::HashMap;

use eframe::egui;
use egui::{Button, CornerRadius, Label, RichText, ScrollArea, Stroke};

use crate::model::{Connection, Place, Signing};
use crate::net::geoip;
use crate::net::rdns;
use crate::storage::config::Config;
use crate::ui::UiCtx;
use crate::ui::conn_visible;
use crate::ui::icons;
use crate::ui::map_conn::conn_row;
use crate::ui::map_panel::MapPanelState;
use crate::ui::map_panel::ProcGroup;
use crate::ui::map_panel::RankSort;
use crate::ui::map_panel::collect_groups;
use crate::ui::map_widgets::{bytes_row, proc_rank_row, rank_header, section_title, traffic_cards};
use crate::ui::theme;
use crate::ui::widgets;

/// 右侧 Inspector 面板:按选中对象切换视图
pub fn inspector_panel(ui: &mut egui::Ui, ctx: &mut UiCtx) {
    let panels = &mut *ctx.map_panels;
    // Esc 清除选中(端点与进程一并清除,回到概览);焦点在输入框
    // (左列表搜索词)时跳过,Esc 留给文本框,不连带清除选中
    let editing = ui.memory(|m| m.focused().is_some());
    if !editing && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        panels.place = None;
        panels.process = None;
        return;
    }
    let rules = &mut *ctx.rules;
    let i18n: &crate::i18n::I18n = ctx.i18n;
    let conns: &[Connection] = ctx.conns;
    let db = ctx.history_db;
    let rdns = ctx.rdns;
    let icon_tex = ctx.icon_tex;
    let default_icon_tex = ctx.default_icon_tex;
    let config: &Config = ctx.config;

    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let process = panels.process.clone();
        match (panels.place, process) {
            (_, Some(name)) => process_view(
                ui,
                panels,
                rules,
                db,
                i18n,
                rdns,
                icon_tex,
                default_icon_tex,
                config,
                conns,
                &name,
            ),
            (Some(place), None) => place_view(
                ui,
                panels,
                i18n,
                rdns,
                icon_tex,
                default_icon_tex,
                config,
                conns,
                place,
            ),
            (None, None) => summary_view(
                ui,
                panels,
                i18n,
                rdns,
                icon_tex,
                default_icon_tex,
                config,
                conns,
            ),
        }
    });
}

/// 概览:进程/远端计数、总流量与 Top 进程/域名排行(无选中时);
/// 全部数据与连接列表同口径(本地/局域网远端噪音过滤)
#[allow(clippy::too_many_arguments)]
fn summary_view(
    ui: &mut egui::Ui,
    panels: &mut MapPanelState,
    i18n: &crate::i18n::I18n,
    rdns: &rdns::Rdns,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    config: &Config,
    conns: &[Connection],
) {
    ui.heading(theme::accent_text(&i18n.t("map-inspector-summary"), 18.0));
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
        ui.label(theme::dim_text(&i18n.t("map-panel-empty"), 12.0));
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

/// 端点详情:位置名、连接数、双向流量与相关进程排行
#[allow(clippy::too_many_arguments)]
fn place_view(
    ui: &mut egui::Ui,
    panels: &mut MapPanelState,
    i18n: &crate::i18n::I18n,
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
        12.0,
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

/// 进程详情:图标与名称、双向流量、路径、签名、阻断开关与连接明细。
/// 连接范围跟随当前端点选中(从端点进入时仅显示该端点下的连接)
#[allow(clippy::too_many_arguments)]
fn process_view(
    ui: &mut egui::Ui,
    panels: &mut MapPanelState,
    rules: &mut crate::rules::RuleSet,
    db: &crate::storage::history::Db,
    i18n: &crate::i18n::I18n,
    rdns: &rdns::Rdns,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    config: &Config,
    conns: &[Connection],
    name: &str,
) {
    let rows: Vec<&Connection> = conns
        .iter()
        .filter(|c| c.process == name)
        .filter(|c| conn_visible(config, c))
        .filter(|c| panels.place.is_none_or(|p| c.city == Some(p)))
        .collect();
    let path = rows.iter().find_map(|c| c.proc_path.clone());
    let display = if name.is_empty() {
        i18n.t("conn-proc-unknown")
    } else {
        name.to_owned()
    };
    let tex = path
        .as_deref()
        .and_then(|p| icon_tex.get(p))
        .and_then(|t| t.as_ref())
        .or(default_icon_tex);
    if title_row(ui, &display, tex, i18n.t("map-inspector-clear")) {
        panels.process = None;
        return;
    }
    ui.label(theme::dim_text(
        &i18n.t_with_args("status-conn-count", &[("count", rows.len().to_string())]),
        12.0,
    ));
    traffic_cards(
        ui,
        rows.iter().map(|c| c.bytes_in).sum(),
        rows.iter().map(|c| c.bytes_out).sum(),
        i18n,
    );

    // 进程信息:完整路径可换行;签名取已知状态中优先的一条(语义色圆点)
    if let Some(p) = &path {
        ui.add(
            Label::new(
                RichText::new(format!("{}: {}", i18n.t("map-inspector-path"), p))
                    .size(11.0)
                    .color(theme::c().text_dim),
            )
            .wrap(),
        );
    }
    let signed = rows
        .iter()
        .map(|c| c.signed)
        .find(|s| *s != Signing::Unknown)
        .unwrap_or(Signing::Unknown);
    let (dot, key) = match signed {
        Signing::Signed => (theme::c().status_ok, "proc-signed"),
        Signing::Unsigned => (theme::c().status_warn, "proc-unsigned"),
        Signing::Invalid => (theme::c().danger, "proc-sign-invalid"),
        Signing::Unknown => (theme::c().text_dim, "proc-sign-unknown"),
    };
    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 5.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 3.0, dot);
        ui.label(theme::dim_text(&i18n.t(key), 11.0));
    });

    // 进程级阻断开关(off 绿 = 放行 / on 红 = 阻断,未知进程不可阻断,
    // 避免空进程条件生成全局规则)
    if !name.is_empty() {
        ui.add_space(2.0);
        let existing = rules
            .process_block_rule(name, path.as_deref())
            .map(|r| r.id);
        let mut on = existing.is_some();
        let resp = widgets::toggle::block_switch(
            ui,
            &mut on,
            true,
            36.0,
            20.0,
            egui::Id::new(("inspector-block-toggle", name.to_owned())),
        );
        if resp.changed() {
            match (on, existing) {
                (true, None) => {
                    if let Err(e) = rules.insert(db, crate::rules::Rule::block(name, None)) {
                        tracing::warn!("[Map] 进程 {name} 阻断规则写入失败: {e}");
                    }
                }
                (false, Some(id)) => {
                    if let Err(e) = rules.delete(db, id) {
                        tracing::warn!("[Map] 进程 {name} 阻断规则(id {id})删除失败: {e}");
                    }
                }
                _ => {}
            }
        }
        ui.label(theme::dim_text(
            &i18n.t(if on {
                "map-unblock-process"
            } else {
                "map-block-process"
            }),
            theme::font::SM,
        ));
    }

    section_title(ui, &i18n.t("map-inspector-conns"));
    for c in &rows {
        conn_row(ui, rules, db, i18n, rdns, c);
    }
}

/// 标题行:图标 + 大标题(Truncate 自适应)+ 右侧清除选中按钮;
/// 返回按钮是否被点击
fn title_row(
    ui: &mut egui::Ui,
    title: &str,
    tex: Option<&egui::TextureHandle>,
    clear_tip: String,
) -> bool {
    let clear_w = 26.0;
    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 6.0;
        // 清除按钮紧凑 padding(style.button_padding 默认 10x5 太宽)
        ui.style_mut().spacing.button_padding = egui::vec2(4.0, 2.0);
        if let Some(t) = tex {
            ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(20.0, 20.0)));
        }
        let title_w = (ui.available_width() - clear_w - 6.0).max(60.0);
        ui.allocate_ui(egui::vec2(title_w, 22.0), |ui| {
            ui.add(
                Label::new(
                    RichText::new(title.to_owned())
                        .size(16.0)
                        .strong()
                        .color(theme::c().text),
                )
                .truncate(),
            );
        });
        let btn = Button::new(
            RichText::new(icons::X_LG)
                .size(11.0)
                .color(theme::c().text_dim),
        )
        .stroke(Stroke::new(1.0, theme::c().stroke))
        .corner_radius(CornerRadius::same(theme::RADIUS_SM));
        ui.add(btn).on_hover_text(clear_tip).clicked()
    })
    .inner
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
