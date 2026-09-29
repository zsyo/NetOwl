//! 地图页右面板(Inspector):概览、端点详情与进程详情三态。
//! 无选中时显示全局概览(计数、总流量与 Top 进程/域名);选中端点
//! 显示该端点的相关进程;选中进程显示路径、签名、阻断开关与连接明细。
//! 选中态由地图端点点击与左列表进程点击驱动。
//!
//! 行内文本一律字符截断(见 [`truncate_chars`]):egui 的 truncate
//! Label/Button 会请求拉满剩余宽,放行中会把后续控件挤出面板,经
//! PanelState 逐帧反馈放大(resizable 面板宽度记忆取自内容矩形)。

use std::collections::HashMap;

use eframe::egui;
use egui::{Button, Color32, CornerRadius, Frame, Label, Margin, RichText, ScrollArea, Stroke};

use crate::model::{Connection, Place, Signing, fmt_bytes};
use crate::net::geoip;
use crate::net::rdns;
use crate::storage::config::Config;
use crate::ui::UiCtx;
use crate::ui::conn_visible;
use crate::ui::icons;
use crate::ui::map_panel::{MapPanelState, ProcGroup, collect_groups, conn_row, truncate_chars};
use crate::ui::theme;

/// 右侧 Inspector 面板:按选中对象切换视图
pub fn inspector_panel(ui: &mut egui::Ui, ctx: &mut UiCtx) {
    let panels = &mut *ctx.map_panels;
    // Esc 清除选中(端点与进程一并清除,回到概览)
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
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
    let config: &Config = ctx.config;

    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let process = panels.process.clone();
        match (panels.place, process) {
            (_, Some(name)) => process_view(
                ui, panels, rules, db, i18n, rdns, icon_tex, config, conns, &name,
            ),
            (Some(place), None) => {
                place_view(ui, panels, i18n, rdns, icon_tex, config, conns, place)
            }
            (None, None) => summary_view(ui, panels, i18n, rdns, icon_tex, config, conns),
        }
    });
}

/// 概览:进程/远端计数、总流量与 Top 进程/域名排行(无选中时);
/// 全部数据与连接列表同口径(本地/局域网远端噪音过滤)
fn summary_view(
    ui: &mut egui::Ui,
    panels: &mut MapPanelState,
    i18n: &crate::i18n::I18n,
    rdns: &rdns::Rdns,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
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
    section_title(ui, &i18n.t("map-inspector-top-proc"));
    let groups = collect_groups(conns, config, None, "", rdns);
    if groups.is_empty() {
        ui.label(theme::dim_text(&i18n.t("map-panel-empty"), 12.0));
    }
    for g in groups.iter().take(5) {
        proc_rank_row(ui, panels, i18n, icon_tex, g);
    }

    section_title(ui, &i18n.t("map-inspector-top-domain"));
    for (host, bytes_in, bytes_out) in top_domains(conns, config, rdns, 5) {
        bytes_row(ui, &host, bytes_in, bytes_out);
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
    config: &Config,
    conns: &[Connection],
    place: Place,
) {
    if title_row(
        ui,
        &truncate_chars(&geoip::place_label(place, i18n), 24),
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
    for g in groups {
        proc_rank_row(ui, panels, i18n, icon_tex, &g);
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
        .and_then(|t| t.as_ref());
    if title_row(
        ui,
        &truncate_chars(&display, 24),
        tex,
        i18n.t("map-inspector-clear"),
    ) {
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

    // 进程信息:完整路径可换行;签名取已知状态中优先的一条
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
    let sign_key = match signed {
        Signing::Signed => "proc-signed",
        Signing::Unsigned => "proc-unsigned",
        Signing::Invalid => "proc-sign-invalid",
        Signing::Unknown => "proc-sign-unknown",
    };
    ui.label(theme::dim_text(&i18n.t(sign_key), 11.0));

    // 进程级阻断开关(未知进程不可阻断,避免空进程条件生成全局规则)
    if !name.is_empty() {
        ui.add_space(2.0);
        let blocked = rules.process_block_rule(name, path.as_deref()).is_some();
        let (glyph, tip, color) = if blocked {
            (icons::BAN, i18n.t("map-unblock-process"), theme::c().danger)
        } else {
            (icons::X_LG, i18n.t("map-block-process"), theme::c().text)
        };
        let text = RichText::new(format!("{glyph} {tip}"))
            .size(12.0)
            .color(color);
        let btn = Button::new(text)
            .fill(if blocked {
                theme::c().danger.gamma_multiply(0.12)
            } else {
                theme::c().bg_card
            })
            .stroke(Stroke::new(1.0, theme::c().stroke))
            .corner_radius(CornerRadius::same(theme::RADIUS_SM));
        if ui.add(btn).clicked() {
            if blocked {
                if let Some(r) = rules.process_block_rule(name, path.as_deref()) {
                    let id = r.id;
                    let _ = rules.delete(db, id);
                }
            } else {
                let _ = rules.insert(db, crate::rules::Rule::block(name, None));
            }
        }
    }

    section_title(ui, &i18n.t("map-inspector-conns"));
    for c in &rows {
        conn_row(ui, rules, db, i18n, rdns, c);
    }
}

/// 标题行:大标题 + 右侧清除选中按钮;返回按钮是否被点击
fn title_row(
    ui: &mut egui::Ui,
    title: &str,
    tex: Option<&egui::TextureHandle>,
    clear_tip: String,
) -> bool {
    ui.horizontal(|ui| {
        if let Some(t) = tex {
            ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(20.0, 20.0)));
        }
        ui.add(
            Label::new(
                RichText::new(title.to_owned())
                    .size(16.0)
                    .strong()
                    .color(theme::c().text),
            )
            .wrap_mode(egui::TextWrapMode::Extend),
        );
        let btn = Button::new(
            RichText::new(icons::X_LG)
                .size(11.0)
                .color(theme::c().text_dim),
        )
        .frame(false);
        ui.add(btn).on_hover_text(clear_tip).clicked()
    })
    .inner
}

/// 小节标题(排行/列表段)
fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(
        RichText::new(text.to_owned())
            .size(12.0)
            .strong()
            .color(theme::c().text_dim),
    );
}

/// 下载/上传双卡片(语义色大数字,样式仿历史页卡片)
fn traffic_cards(ui: &mut egui::Ui, down: u64, up: u64, i18n: &crate::i18n::I18n) {
    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 8.0;
        // 卡宽留余量:Frame 描边等细微外扩若顶满可用宽,超出部分会经
        // resizable 面板的宽度记忆逐帧放大(见模块注释)
        let w = (ui.available_width() - 16.0) / 2.0;
        traffic_card(
            ui,
            w,
            &i18n.t("nav-rate-down"),
            &fmt_bytes(down),
            theme::c().inbound,
        );
        traffic_card(
            ui,
            w,
            &i18n.t("nav-rate-up"),
            &fmt_bytes(up),
            theme::c().outbound,
        );
    });
}

fn traffic_card(ui: &mut egui::Ui, w: f32, label: &str, value: &str, color: Color32) {
    // Frame 的响应宽 = 内容宽 + 内边距;内容需减去两侧内边距,
    // 否则两卡合计超出面板可用宽,经面板宽度记忆逐帧膨胀
    Frame::new()
        .fill(theme::c().bg_card)
        .stroke(Stroke::new(1.0, theme::c().stroke))
        .corner_radius(CornerRadius::same(theme::RADIUS_MD))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(w - 20.0);
            ui.set_min_height(44.0);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(value.to_owned())
                        .size(16.0)
                        .strong()
                        .color(color),
                );
                ui.label(theme::dim_text(label, 10.0));
            });
        });
}

/// Top 进程行:图标 + 名称(点击选中联动进程详情)+ 双向字节
fn proc_rank_row(
    ui: &mut egui::Ui,
    panels: &mut MapPanelState,
    i18n: &crate::i18n::I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    g: &ProcGroup,
) {
    let selected = panels.process.as_deref() == Some(g.name.as_str());
    let display = if g.name.is_empty() {
        i18n.t("conn-proc-unknown")
    } else {
        g.name.clone()
    };
    let path = g.conns.iter().find_map(|c| c.proc_path.as_deref());
    let bytes_text = format!(
        "{} / {}",
        fmt_bytes(g.conns.iter().map(|c| c.bytes_in).sum()),
        fmt_bytes(g.conns.iter().map(|c| c.bytes_out).sum())
    );
    ui.horizontal(|ui| {
        let tex = path.and_then(|p| icon_tex.get(p)).and_then(|t| t.as_ref());
        match tex {
            Some(t) => {
                ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(16.0, 16.0)));
            }
            None => {
                ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            }
        }
        let text = RichText::new(truncate_chars(
            &format!("{display} ({})", g.conns.len()),
            20,
        ))
        .size(12.0)
        .color(if selected {
            theme::c().accent
        } else {
            theme::c().text
        });
        let btn = Button::new(text)
            .fill(if selected {
                theme::c().accent_soft
            } else {
                Color32::TRANSPARENT
            })
            .frame(false)
            .corner_radius(CornerRadius::same(theme::RADIUS_SM));
        if ui.add(btn).clicked() {
            panels.process = Some(g.name.clone());
        }
        ui.label(
            RichText::new(bytes_text)
                .size(11.0)
                .color(theme::c().text_dim),
        );
    });
}

/// 域名/IP 聚合的流量排行(前 n,双向字节降序);UDP 无远端不计
fn top_domains(
    conns: &[Connection],
    config: &Config,
    rdns: &rdns::Rdns,
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
    // 次级键按名称:未提权时字节恒 0,total 相同的行若不加稳定键,
    // HashMap 迭代序随机导致排行每帧跳动
    rows.sort_by(|a, b| (b.1 + b.2).cmp(&(a.1 + a.2)).then_with(|| a.0.cmp(&b.0)));
    rows.truncate(n);
    rows
}

/// 排行的域名行:名称 + 双向字节(不可点击)
fn bytes_row(ui: &mut egui::Ui, title: &str, bytes_in: u64, bytes_out: u64) {
    let bytes_text = format!("{} / {}", fmt_bytes(bytes_in), fmt_bytes(bytes_out));
    ui.horizontal(|ui| {
        ui.add(
            Label::new(
                RichText::new(truncate_chars(title, 22).to_owned())
                    .size(12.0)
                    .color(theme::c().text),
            )
            .wrap_mode(egui::TextWrapMode::Extend),
        );
        ui.label(
            RichText::new(bytes_text)
                .size(11.0)
                .color(theme::c().text_dim),
        );
    });
}
