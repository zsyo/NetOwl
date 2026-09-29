//! 地图页左面板:按进程分组的连接列表,支持端点过滤、搜索与一键阻断。
//! 阻断 = 创建永久 Block 规则(求值未命中默认放行,故无"允许"按钮),
//! 再次点击删除对应规则撤销;WFP 由 App 层轮询自动同步生效。

use std::collections::{HashMap, HashSet};

use eframe::egui;
use egui::{Button, Color32, CornerRadius, Label, RichText, ScrollArea, Stroke, TextEdit, Vec2};

use crate::model::{Connection, Place, fmt_bytes};
use crate::net::geoip;
use crate::net::rdns;
use crate::rules::wfp;
use crate::rules::{RemoteKind, Rule};
use crate::ui::icons;
use crate::ui::theme;
use crate::ui::{UiCtx, text_width};

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
        }
    }
}

/// 按映像名分组的进程连接(组名空串 = 未知进程)
struct ProcGroup<'a> {
    name: String,
    conns: Vec<&'a Connection>,
    /// 组内累计流量(排序键)
    total: u64,
}

/// 当前过滤口径下的进程分组:端点选中收窄,搜索词命中组名时保留组内
/// 全部连接,否则只保留远端(IP/域名)命中的连接;组按累计流量降序
fn collect_groups<'a>(
    conns: &'a [Connection],
    place: Option<Place>,
    search: &str,
    rdns: &rdns::Rdns,
) -> Vec<ProcGroup<'a>> {
    let needle = search.trim().to_lowercase();
    let mut map = std::collections::HashMap::<&str, ProcGroup<'a>>::new();
    for c in conns {
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
    groups.sort_by_key(|g| std::cmp::Reverse(g.total));
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
    let wfp_status = &ctx.wfp_status;

    // 端点过滤提示条(地图点选的联动来源,可就地解除)
    if let Some(place) = panels.place {
        ui.horizontal(|ui| {
            let text = i18n.t_with_args(
                "map-panel-filter",
                &[("place", geoip::place_label(place, i18n))],
            );
            ui.add(Label::new(RichText::new(text).size(11.0).color(theme::c().accent)).truncate());
            let clear = Button::new(
                RichText::new(icons::X_LG)
                    .size(10.0)
                    .color(theme::c().text_dim),
            )
            .frame(false);
            if ui.add(clear).clicked() {
                panels.place = None;
                panels.process = None;
            }
        });
    }

    ui.add(
        TextEdit::singleline(&mut panels.search)
            .hint_text(RichText::new(i18n.t("map-panel-search")).size(12.0))
            .desired_width(ui.available_width()),
    );

    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let groups = collect_groups(conns, panels.place, &panels.search, rdns);
        if groups.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.label(theme::dim_text(&i18n.t("map-panel-empty"), 12.0));
            });
            return;
        }
        for g in &groups {
            let path = g.conns.iter().find_map(|c| c.proc_path.as_deref());
            group_row(ui, panels, rules, db, i18n, icon_tex, g, path);
            if panels.expanded.contains(&g.name) {
                for c in &g.conns {
                    conn_row(ui, rules, db, i18n, rdns, c);
                }
            }
            ui.add_space(2.0);
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
        ui.add(Label::new(RichText::new(text).size(11.0).color(color)).wrap());
    }
}

/// 进程组头行:展开箭头、图标、名称(点击选中联动右侧详情)与
/// 进程级阻断开关(未知进程不可阻断,避免空进程条件生成全局规则)
#[allow(clippy::too_many_arguments)]
fn group_row(
    ui: &mut egui::Ui,
    panels: &mut MapPanelState,
    rules: &mut crate::rules::RuleSet,
    db: &crate::storage::history::Db,
    i18n: &crate::i18n::I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    g: &ProcGroup,
    path: Option<&str>,
) {
    let unknown = g.name.is_empty();
    let display = if unknown {
        i18n.t("conn-proc-unknown")
    } else {
        g.name.clone()
    };
    let selected = panels.process.as_deref() == Some(g.name.as_str());
    let expanded = panels.expanded.contains(&g.name);

    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 2.0;
        let arrow = if expanded {
            icons::CHEVRON_DOWN
        } else {
            icons::CHEVRON_RIGHT
        };
        let fold = Button::new(RichText::new(arrow).size(10.0).color(theme::c().text_dim))
            .frame(false)
            .min_size(Vec2::new(16.0, 20.0));
        let fold_clicked = ui.add(fold).clicked();
        // 未展开(remove 失败)则展开,已展开则收起
        if fold_clicked && !panels.expanded.remove(&g.name) {
            panels.expanded.insert(g.name.clone());
        }
        let tex = path.and_then(|p| icon_tex.get(p)).and_then(|t| t.as_ref());
        match tex {
            Some(t) => {
                ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(16.0, 16.0)));
            }
            None => {
                ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            }
        }
        // 名称行:导航栏同款的受控选中样式(SelectableLabel 为内部
        // Toggle 状态,跨行单选不受控,不用)
        let label = format!("{display} ({})", g.conns.len());
        let text = RichText::new(label).size(13.0).color(if selected {
            theme::c().text
        } else {
            theme::c().text_dim
        });
        let btn = Button::new(text)
            .fill(if selected {
                theme::c().accent_soft
            } else {
                Color32::TRANSPARENT
            })
            .stroke(if selected {
                Stroke::new(1.0, theme::c().accent.gamma_multiply(0.4))
            } else {
                Stroke::NONE
            })
            .corner_radius(CornerRadius::same(theme::RADIUS_SM));
        let block_w = if unknown { 0.0 } else { 24.0 };
        let label_w = (ui.available_width() - block_w - 4.0).max(60.0);
        if ui.add_sized([label_w, 22.0], btn).clicked() {
            if selected {
                panels.process = None;
            } else {
                panels.process = Some(g.name.clone());
            }
        }
        if unknown {
            return;
        }
        let blocked = rules.process_block_rule(&g.name, path).is_some();
        let (glyph, tip) = if blocked {
            (icons::BAN, i18n.t("map-unblock-process"))
        } else {
            (icons::X_LG, i18n.t("map-block-process"))
        };
        let resp = ui
            .add(
                Button::new(RichText::new(glyph).size(12.0).color(if blocked {
                    theme::c().danger
                } else {
                    theme::c().text_dim
                }))
                .frame(false)
                .min_size(Vec2::new(20.0, 20.0)),
            )
            .on_hover_text(tip);
        if resp.clicked() {
            if blocked {
                if let Some(r) = rules.process_block_rule(&g.name, path) {
                    let id = r.id;
                    let _ = rules.delete(db, id);
                }
            } else {
                let _ = rules.insert(db, Rule::block(&g.name, None));
            }
        }
    });
}

/// 子模块间复用的阻断动作(连接行的目标级阻断;进程级在组头处理)
enum BlockAct {
    /// 已由进程级规则阻断,不可在目标级撤销
    None,
    /// 删除命中的目标级阻断规则
    Delete(i64),
    /// 新建进程 + 目标 IP 规则
    Add(std::net::Ipv4Addr),
}

/// 连接明细行:远端(域名/地址):端口 + 协议、累计字节与目标级阻断开关。
/// 已被进程级规则阻断的连接按钮置灰(撤销进程规则会放大放行范围,不做)
fn conn_row(
    ui: &mut egui::Ui,
    rules: &mut crate::rules::RuleSet,
    db: &crate::storage::history::Db,
    i18n: &crate::i18n::I18n,
    rdns: &rdns::Rdns,
    c: &Connection,
) {
    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 4.0;
        ui.add_space(20.0);
        let remote = match rdns.lookup(c.remote_ip) {
            Some(host) => format!("{host}:{}", c.remote_port),
            None => c.remote_display(),
        };
        let text = format!("{} {}", remote, c.proto.as_str());
        let bytes_text = format!("{} / {}", fmt_bytes(c.bytes_in), fmt_bytes(c.bytes_out));
        let bytes_w = text_width(ui, &bytes_text, 11.0) + 4.0;
        let left_w = (ui.available_width() - bytes_w - 24.0).max(60.0);
        ui.add_sized(
            [left_w, 18.0],
            Label::new(RichText::new(text).size(12.0).color(theme::c().text)).truncate(),
        );
        ui.label(
            RichText::new(bytes_text)
                .size(11.0)
                .color(theme::c().text_dim),
        );

        // 未知进程:无法限定进程条件,不做目标级阻断(会生成全局 IP 规则)
        if c.process.is_empty() {
            return;
        }
        let act = match rules.blocking_rule(c, rdns.lookup(c.remote_ip)) {
            Some(r) if r.remote_kind == RemoteKind::Ip => {
                let id = r.id;
                if block_button(ui, icons::BAN, &i18n.t("map-unblock-target"), true, true).clicked()
                {
                    BlockAct::Delete(id)
                } else {
                    BlockAct::None
                }
            }
            // 命中的是进程级规则:目标级按钮置灰,撤销交给组头的进程开关
            // (在这里删除进程规则会连带放行该进程的其他目标,超出预期)
            Some(_) => {
                block_button(
                    ui,
                    icons::BAN,
                    &i18n.t("map-blocked-by-process"),
                    true,
                    false,
                )
                .on_disabled_hover_text(i18n.t("map-blocked-by-process"));
                BlockAct::None
            }
            None => {
                if block_button(ui, icons::X_LG, &i18n.t("map-block-target"), false, true).clicked()
                {
                    BlockAct::Add(c.remote_ip)
                } else {
                    BlockAct::None
                }
            }
        };
        match act {
            BlockAct::None => {}
            BlockAct::Delete(id) => {
                let _ = rules.delete(db, id);
            }
            BlockAct::Add(ip) => {
                let _ = rules.insert(db, Rule::block(&c.process, Some(ip)));
            }
        }
    });
}

/// 小型图标操作按钮(阻断/撤销;danger = 已阻断语义色)
fn block_button(
    ui: &mut egui::Ui,
    glyph: &str,
    tip: &str,
    danger: bool,
    enabled: bool,
) -> egui::Response {
    let color = if !enabled {
        theme::c().text_dim
    } else if danger {
        theme::c().danger
    } else {
        theme::c().text_dim
    };
    let btn = Button::new(RichText::new(glyph).size(12.0).color(color))
        .frame(false)
        .min_size(Vec2::new(20.0, 18.0));
    ui.add_enabled(enabled, btn).on_hover_text(tip)
}
