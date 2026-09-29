//! 主窗口布局:导航侧栏与各页面(地图 / 连接 / 规则占位 / 设置)。
//! 全部界面文本经 I18n 词条获取(AGENTS.md 规范 4)。

pub mod ask;
pub mod history;
pub mod icons;
pub mod log_window;
pub mod map_conn;
pub mod map_inspector;
pub mod map_panel;
pub mod map_widgets;
pub mod rules;
pub mod theme;

use std::collections::HashMap;

use eframe::egui;
use egui::{Button, Color32, CornerRadius, Label, RichText, Stroke};

use crate::collector::CollectorKind;
use crate::i18n::I18n;
use crate::logging::LogLevel;
use crate::map;
use crate::map::basemap;
use crate::model::{Connection, Signing, fmt_bytes};
use crate::net::geoip;
use crate::net::rdns;
use crate::rules as rules_engine;
use crate::rules::wfp;
use crate::storage::config::Config;
use crate::storage::history as history_store;
use crate::storage::history_query;

/// 连接列表排序键(表头点击切换;None = 表快照原序)
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConnSort {
    Process,
    Location,
    RateDown,
    RateUp,
    TotalDown,
    TotalUp,
}

/// 连接列表排序状态:(键, 是否正序);点击已激活表头反转方向
pub type ConnSortState = Option<(ConnSort, bool)>;

/// 主窗口页面
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Map,
    Connections,
    History,
    Rules,
    Settings,
}

/// 左侧导航项:(页面, 词条键, 图标码点(ui::icons,glyph 名见常量注释))
const NAV_ITEMS: &[(Page, &str, &str)] = &[
    (Page::Map, "nav-map", icons::GLOBE),
    (Page::Connections, "nav-connections", icons::LIST_UL),
    (Page::History, "nav-history", icons::CLOCK_HISTORY),
    (Page::Rules, "nav-rules", icons::SHIELD),
    (Page::Settings, "nav-settings", icons::GEAR),
];

/// 左侧导航栏
pub fn nav_ui(
    ui: &mut egui::Ui,
    page: &mut Page,
    conns: &[Connection],
    i18n: &I18n,
    rates: (u64, u64),
    collector_kind: CollectorKind,
) {
    ui.add_space(4.0);
    // 品牌名与副标题居中
    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
        ui.label(
            RichText::new(i18n.t("app-name"))
                .size(22.0)
                .strong()
                .color(theme::c().accent),
        );
        ui.label(theme::dim_text(&i18n.t("app-subtitle"), 10.0));
    });
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(6.0);

    for (target, key, icon) in NAV_ITEMS {
        let selected = page == target;
        let label_text = RichText::new(format!("{icon}  {}", i18n.t(key)))
            .size(15.0)
            .color(if selected {
                theme::c().text
            } else {
                theme::c().text_dim
            });
        let response = ui.add_sized(
            [ui.available_width(), 34.0],
            Button::new(label_text)
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
                .corner_radius(CornerRadius::same(theme::RADIUS_MD)),
        );
        if response.clicked() {
            *page = *target;
        }
    }

    // 底部状态区(bottom_up:先绘制的贴底)。速率行给固定宽度起点:
    // "下载/上传"标签不随速率长短移动,速率只向右延伸,整体近似居中
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        ui.add_space(8.0);
        let x = ((ui.available_width() - RATE_ROW_WIDTH) / 2.0).max(0.0) + 40.0;
        rate_row(ui, i18n, "nav-rate-up", rates.1, theme::c().outbound, x);
        rate_row(ui, i18n, "nav-rate-down", rates.0, theme::c().inbound, x);
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);

        // 状态行/条数水平居中(按文本实测宽度计算偏移;
        // bottom_up 内嵌套占满式布局会把内容顶到菜单下方,故逐行偏移)
        let text = i18n.t("status-monitoring");
        let w = 14.0 + 8.0 + text_width(ui, &text, 13.0);
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(rect.center(), 4.0, theme::c().status_ok);
            ui.label(theme::dim_text(&text, 13.0));
        });
        let text = i18n.t_with_args("status-conn-count", &[("count", conns.len().to_string())]);
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - text_width(ui, &text, 11.0)) / 2.0).max(0.0));
            ui.label(RichText::new(text).size(11.0).color(theme::c().text_dim));
        });
        // 模拟数据源提示(真实采集时无独立状态行,避免与绿点行重复)
        if collector_kind == CollectorKind::Mock {
            let text = i18n.t("status-mock");
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - text_width(ui, &text, 11.0)) / 2.0).max(0.0));
                ui.label(RichText::new(text).size(11.0).color(theme::c().text_dim));
            });
        }
    });
}

/// 速率行固定宽度:标签起点固定,速率在行内向右延伸,整体近似居中
const RATE_ROW_WIDTH: f32 = 150.0;

/// 侧栏字体下文本宽度(居中偏移计算用;地图面板行宽计算共用)
pub(crate) fn text_width(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(size),
            egui::Color32::WHITE,
        )
        .rect
        .width()
}

/// 一行速率:"下载/上传"标签 + 速率,经固定偏移实现近似居中
fn rate_row(ui: &mut egui::Ui, i18n: &I18n, key: &str, rate: u64, color: Color32, x: f32) {
    ui.horizontal(|ui| {
        ui.add_space(x);
        ui.label(RichText::new(i18n.t(key)).size(12.0).color(color));
        ui.label(
            RichText::new(format!("{}/s", fmt_bytes(rate)))
                .size(12.0)
                .color(color),
        );
    });
}

/// 各页面共用的绘制上下文(集中可变状态引用,避免签名持续膨胀)
pub struct UiCtx<'a> {
    pub conns: &'a [Connection],
    pub i18n: &'a mut I18n,
    pub map_view: &'a mut basemap::View,
    pub config: &'a mut Config,
    pub rdns: &'a rdns::Rdns,
    /// 总速率(字节/秒):(下行, 上行)
    pub rates: (u64, u64),
    /// 每连接实时速率(键 = 连接 id;ETW 字节差值/秒,未提权恒 0)
    pub conn_rates: &'a HashMap<u64, (u64, u64)>,
    /// 连接列表表头排序状态(表头点击切换)
    pub conn_sort: &'a mut ConnSortState,
    /// 进程图标纹理(键 = 映像路径);None 表示已提取且无图标
    pub icon_tex: &'a HashMap<String, Option<egui::TextureHandle>>,
    /// 历史页状态
    pub history: &'a mut history_query::PageState,
    /// 历史查询只读连接
    pub history_db: &'a history_store::Db,
    /// 规则集(连接页求值与规则页编辑)
    pub rules: &'a mut rules_engine::RuleSet,
    /// 地图页左右面板与选中状态(端点点击联动)
    pub map_panels: &'a mut map_panel::MapPanelState,
    /// 规则页状态
    pub rules_page: &'a mut rules::PageState,
    /// 日志浏览窗口状态
    pub log_window: &'a mut log_window::PageState,
    /// 拦截引擎状态(WFP 管理线程回报)
    pub wfp_status: wfp::Status,
    /// 历史写线程句柄(手动清空)
    pub writer: &'a history_store::Writer,
    pub local_pos: (f32, f32),
}

/// 中央区域按页面分发;返回本轮是否直接改动了配置(由 App 层标脏落盘)
pub fn central_ui(ui: &mut egui::Ui, page: &Page, ctx: &mut UiCtx) -> bool {
    match page {
        Page::Map => {
            map_header(ui, ctx);
            ui.add_space(6.0);
            let click = map::draw(
                ui,
                ctx.conns,
                ctx.i18n,
                ctx.map_view,
                ctx.rdns,
                ctx.icon_tex,
                ctx.local_pos,
                ctx.map_panels.place,
            );
            match click {
                Some(map::MapClick::Place(place)) => {
                    ctx.map_panels.place = Some(place);
                    ctx.map_panels.process = None;
                }
                Some(map::MapClick::Background) => {
                    ctx.map_panels.place = None;
                    ctx.map_panels.process = None;
                }
                None => {}
            }
            false
        }
        Page::Connections => connections_ui(ui, ctx),
        Page::History => history::show(
            ui,
            ctx.history,
            ctx.i18n,
            ctx.icon_tex,
            ctx.history_db,
            ctx.writer,
            ctx.config,
        ),
        Page::Rules => {
            rules::show(
                ui,
                ctx.rules_page,
                ctx.i18n,
                ctx.history_db,
                ctx.rules,
                &ctx.wfp_status,
            );
            false
        }
        Page::Settings => settings_ui(ui, ctx.config, ctx.i18n, ctx.log_window),
    }
}

/// 地图页标题行:标题、副标题、图例与左右面板开关(开关在图例之后,
/// 从右往左排布)
fn map_header(ui: &mut egui::Ui, ctx: &mut UiCtx) {
    ui.horizontal(|ui| {
        ui.heading(theme::accent_text(&ctx.i18n.t("map-title"), 20.0));
        ui.label(theme::dim_text(&ctx.i18n.t("map-subtitle"), 13.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let panels = &mut *ctx.map_panels;
            panel_toggle(
                ui,
                &mut panels.show_right,
                icons::LAYOUT_TEXT_SIDEBAR_REVERSE,
                &ctx.i18n.t("map-panel-toggle-inspector"),
            );
            panel_toggle(
                ui,
                &mut panels.show_left,
                icons::LAYOUT_SIDEBAR,
                &ctx.i18n.t("map-panel-toggle-list"),
            );
            ui.add_space(10.0);
            legend(ui, theme::c().outbound, &ctx.i18n.t("map-legend-out"));
            ui.add_space(10.0);
            legend(ui, theme::c().inbound, &ctx.i18n.t("map-legend-in"));
        });
    });
}

/// 面板开关小按钮(图标高亮 = 面板显示)
fn panel_toggle(ui: &mut egui::Ui, on: &mut bool, glyph: &str, tip: &str) {
    let text = RichText::new(glyph).size(15.0).color(if *on {
        theme::c().accent
    } else {
        theme::c().text_dim
    });
    let resp = ui
        .add(
            Button::new(text)
                .frame(false)
                .min_size(egui::vec2(24.0, 24.0)),
        )
        .on_hover_text(tip);
    if resp.clicked() {
        *on = !*on;
    }
}

/// 图例:语义色圆点 + 文字
fn legend(ui: &mut egui::Ui, color: Color32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
    ui.label(theme::dim_text(label, 12.0));
}

/// 连接显示过滤:本地/局域网远端噪音(config 持久化;连接列表与
/// 地图页左右面板共用同一口径)
pub(crate) fn conn_visible(config: &Config, c: &Connection) -> bool {
    !(config.general.hide_local && c.remote_ip.is_loopback())
        && !(config.general.hide_lan && c.remote_ip.is_private())
}

/// 连接列表页;返回是否直接改动了配置(隐藏本地/局域网开关)。
/// 末列显示规则求值动作(允许/阻断,规则引擎默认放行)
fn connections_ui(ui: &mut egui::Ui, ctx: &mut UiCtx) -> bool {
    let UiCtx {
        conns,
        i18n,
        rdns,
        icon_tex,
        config,
        rules,
        conn_rates,
        conn_sort,
        ..
    } = ctx;
    // &mut UiCtx 解构出的引用字段带两层 &mut,借类型注解 coerce 回单层
    let conn_sort: &mut ConnSortState = conn_sort;
    ui.heading(theme::accent_text(&i18n.t("conns-title"), 20.0));
    ui.label(theme::dim_text(&i18n.t("conns-subtitle"), 13.0));
    ui.add_space(6.0);

    // 本地/局域网远端噪音过滤(config 持久化,连接页与历史页共享)
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut config.general.hide_local, i18n.t("filter-hide-local"))
            .changed()
        {
            changed = true;
        }
        if ui
            .checkbox(&mut config.general.hide_lan, i18n.t("filter-hide-lan"))
            .changed()
        {
            changed = true;
        }
    });
    ui.add_space(4.0);

    // 空态判定与过滤同口径:全部连接都被隐藏时同样提示无连接
    let mut shown: Vec<&Connection> = conns.iter().filter(|c| conn_visible(config, c)).collect();
    sort_conns(&mut shown, conn_sort, conn_rates, i18n);
    if shown.is_empty() {
        ui.label(theme::dim_text(&i18n.t("conns-empty"), 14.0));
        return changed;
    }

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            egui::Grid::new("connections_grid")
                .num_columns(9)
                .spacing([24.0, 9.0])
                .striped(true)
                .show(ui, |ui| {
                    // 表头:可排序列可点击(当前排序列带方向三角),
                    // 协议/远端/动作为纯展示列
                    let mut header = |ui: &mut egui::Ui, key: &str, sort: Option<ConnSort>| {
                        let mut text = i18n.t(key);
                        let active = conn_sort.is_some_and(|(k, _)| Some(k) == sort);
                        if active {
                            let tri = if conn_sort.is_some_and(|(_, asc)| asc) {
                                icons::CARET_UP_FILL
                            } else {
                                icons::CARET_DOWN_FILL
                            };
                            text = format!("{text} {tri}");
                        }
                        let label = RichText::new(text)
                            .size(12.0)
                            .strong()
                            .color(theme::c().text_dim);
                        let resp = match sort {
                            Some(s) => {
                                let r = ui.selectable_label(active, label);
                                if r.clicked() {
                                    let current: ConnSortState = *conn_sort;
                                    let next = match current {
                                        Some((k, asc)) if k == s => (s, !asc),
                                        _ => (s, true),
                                    };
                                    *conn_sort = Some(next);
                                }
                                r
                            }
                            None => {
                                ui.add(egui::Label::new(label));
                                ui.interact(
                                    ui.max_rect(),
                                    egui::Id::new(("conn-header", key)),
                                    egui::Sense::hover(),
                                )
                            }
                        };
                        resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                    };
                    header(ui, "col-process", Some(ConnSort::Process));
                    header(ui, "col-proto", None);
                    header(ui, "col-remote", None);
                    header(ui, "col-location", Some(ConnSort::Location));
                    header(ui, "col-down", Some(ConnSort::RateDown));
                    header(ui, "col-up", Some(ConnSort::RateUp));
                    header(ui, "col-down-total", Some(ConnSort::TotalDown));
                    header(ui, "col-up-total", Some(ConnSort::TotalUp));
                    header(ui, "col-action", None);
                    ui.end_row();

                    for conn in shown {
                        let process = if conn.process.is_empty() {
                            format!("{} (PID {})", i18n.t("conn-proc-unknown"), conn.pid)
                        } else {
                            conn.process.clone()
                        };
                        // 进程列两行:映像名(带图标)+ 弱化的签名状态与路径。
                        // 无图标的进程也占位 18px,保证各行文字起点对齐不跳动
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                let tex = conn
                                    .proc_path
                                    .as_deref()
                                    .and_then(|p| icon_tex.get(p))
                                    .and_then(|t| t.as_ref());
                                match tex {
                                    Some(t) => {
                                        ui.add(
                                            egui::Image::new(t)
                                                .fit_to_exact_size(egui::vec2(16.0, 16.0)),
                                        );
                                    }
                                    None => {
                                        ui.allocate_exact_size(
                                            egui::vec2(16.0, 16.0),
                                            egui::Sense::hover(),
                                        );
                                    }
                                }
                                ui.add(
                                    Label::new(
                                        RichText::new(process).size(13.0).color(theme::c().text),
                                    )
                                    .wrap_mode(egui::TextWrapMode::Extend),
                                );
                            });
                            ui.add(
                                Label::new(theme::dim_text(&proc_detail(conn, i18n), 11.0))
                                    .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        });
                        ui.label(theme::dim_text(conn.proto.as_str(), 13.0));
                        // rDNS 域名优先,域名下方弱化显示裸 IP;无 PTR 回退地址:端口。
                        // 单行延伸(Extend)禁用自动折行,列宽由最宽内容撑开
                        ui.vertical(|ui| match rdns.lookup(conn.remote_ip) {
                            Some(host) => {
                                ui.add(
                                    Label::new(
                                        RichText::new(rdns::display(host, conn.remote_port, 36))
                                            .size(13.0)
                                            .color(theme::c().text),
                                    )
                                    .wrap_mode(egui::TextWrapMode::Extend),
                                );
                                ui.add(
                                    Label::new(theme::dim_text(&conn.remote_ip.to_string(), 11.0))
                                        .wrap_mode(egui::TextWrapMode::Extend),
                                );
                            }
                            None => {
                                ui.add(
                                    Label::new(
                                        RichText::new(conn.remote_display())
                                            .size(13.0)
                                            .color(theme::c().text),
                                    )
                                    .wrap_mode(egui::TextWrapMode::Extend),
                                );
                            }
                        });
                        let location = match conn.city {
                            Some(place) => geoip::place_label(place, i18n),
                            None => i18n.t("conn-loc-unknown"),
                        };
                        ui.label(theme::dim_text(&location, 13.0));
                        // 下载/上传列显示实时速率(ETW 字节差值),悬停显示累计字节;
                        // 未提权时 ETW 未启动,速率恒 0
                        let (rin, rout) = conn_rates.get(&conn.id).copied().unwrap_or((0, 0));
                        ui.label(
                            RichText::new(format!("{}/s", fmt_bytes(rin)))
                                .size(13.0)
                                .color(theme::c().inbound),
                        )
                        .on_hover_text(format!(
                            "{} {}",
                            i18n.t("conn-total-bytes"),
                            fmt_bytes(conn.bytes_in)
                        ));
                        ui.label(
                            RichText::new(format!("{}/s", fmt_bytes(rout)))
                                .size(13.0)
                                .color(theme::c().outbound),
                        )
                        .on_hover_text(format!(
                            "{} {}",
                            i18n.t("conn-total-bytes"),
                            fmt_bytes(conn.bytes_out)
                        ));
                        // 累计字节列(速率列的悬停信息在此显式展示)
                        ui.label(
                            RichText::new(fmt_bytes(conn.bytes_in))
                                .size(13.0)
                                .color(theme::c().inbound),
                        );
                        ui.label(
                            RichText::new(fmt_bytes(conn.bytes_out))
                                .size(13.0)
                                .color(theme::c().outbound),
                        );
                        // 规则求值:命中规则的连接标注动作,未命中默认放行不标注
                        match rules.evaluate(&rules_engine::MatchReq::from_conn(
                            conn,
                            rdns.lookup(conn.remote_ip),
                        )) {
                            Some(hit) => {
                                let (key, color) = match hit.action {
                                    rules_engine::Action::Allow => {
                                        ("conn-action-allow", theme::c().status_ok)
                                    }
                                    rules_engine::Action::Block => {
                                        ("conn-action-block", theme::c().danger)
                                    }
                                };
                                ui.label(RichText::new(i18n.t(key)).size(13.0).color(color));
                            }
                            None => {
                                ui.label("");
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    changed
}

/// 按表头排序状态排列连接(None = 表快照原序);文本键大小写不敏感,
/// 未知归属(无定位)无论方向恒排在有位置连接之后
fn sort_conns(
    shown: &mut [&Connection],
    sort: &ConnSortState,
    conn_rates: &HashMap<u64, (u64, u64)>,
    i18n: &I18n,
) {
    use std::cmp::Ordering;
    let Some((key, asc)) = *sort else {
        return;
    };
    let flip = |c: Ordering| if asc { c } else { c.reverse() };
    let rate = |id: u64, up: bool| match conn_rates.get(&id) {
        Some(r) => {
            if up {
                r.1
            } else {
                r.0
            }
        }
        None => 0,
    };
    shown.sort_unstable_by(|a, b| match key {
        ConnSort::Process => flip(a.process.to_lowercase().cmp(&b.process.to_lowercase())),
        ConnSort::Location => match (a.city, b.city) {
            (Some(x), Some(y)) => {
                flip(geoip::place_label(x, i18n).cmp(&geoip::place_label(y, i18n)))
            }
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        },
        ConnSort::RateDown => flip(rate(a.id, false).cmp(&rate(b.id, false))),
        ConnSort::RateUp => flip(rate(a.id, true).cmp(&rate(b.id, true))),
        ConnSort::TotalDown => flip(a.bytes_in.cmp(&b.bytes_in)),
        ConnSort::TotalUp => flip(a.bytes_out.cmp(&b.bytes_out)),
    });
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

/// 设置页:语言切换(词条即时生效)、主题、数据源;返回是否直接改动了配置。
/// 日志区:级别下拉与文件开关立即生效(直接调 logging),窗口入口只置位状态
fn settings_ui(
    ui: &mut egui::Ui,
    config: &mut Config,
    i18n: &mut I18n,
    log_window: &mut log_window::PageState,
) -> bool {
    ui.heading(theme::accent_text(&i18n.t("settings-title"), 20.0));
    ui.add_space(16.0);
    ui.label(
        RichText::new(i18n.t("settings-language"))
            .size(14.0)
            .strong()
            .color(theme::c().text),
    );
    ui.add_space(4.0);

    let current_name = i18n
        .available_langs
        .iter()
        .find(|info| info.code == i18n.current_lang)
        .map(|info| info.name.clone())
        .unwrap_or_else(|| i18n.current_lang.clone());
    egui::ComboBox::from_id_salt("settings-language-select")
        .width(180.0)
        .selected_text(current_name)
        .show_ui(ui, |ui| {
            for (code, name) in i18n.lang_codes_and_names() {
                let selected = code == i18n.current_lang;
                let label = RichText::new(name).size(14.0).color(if selected {
                    theme::c().accent
                } else {
                    theme::c().text
                });
                if ui.selectable_label(selected, label).clicked() {
                    i18n.set_language(code);
                }
            }
        });
    ui.add_space(6.0);
    ui.label(theme::dim_text(&i18n.t("settings-language-hint"), 12.0));
    ui.add_space(16.0);

    // 界面主题:切换立即生效(调色板与 Visuals 同步刷新)
    ui.label(
        RichText::new(i18n.t("settings-theme"))
            .size(14.0)
            .strong()
            .color(theme::c().text),
    );
    ui.add_space(4.0);
    let theme_name = if theme::is_dark() {
        i18n.t("theme-dark")
    } else {
        i18n.t("theme-light")
    };
    egui::ComboBox::from_id_salt("settings-theme-select")
        .width(180.0)
        .selected_text(theme_name)
        .show_ui(ui, |ui| {
            for (dark, name) in [(true, i18n.t("theme-dark")), (false, i18n.t("theme-light"))] {
                let selected = theme::is_dark() == dark;
                let label = RichText::new(name).size(14.0).color(if selected {
                    theme::c().accent
                } else {
                    theme::c().text
                });
                if ui.selectable_label(selected, label).clicked() {
                    theme::set_theme(dark, ui.ctx());
                }
            }
        });
    ui.add_space(6.0);
    ui.label(theme::dim_text(&i18n.t("settings-theme-hint"), 12.0));
    ui.add_space(16.0);

    // 数据源:真实采集 / 模拟演示,切换后由 logic 检测配置变化并重建采集器
    ui.label(
        RichText::new(i18n.t("settings-datasource"))
            .size(14.0)
            .strong()
            .color(theme::c().text),
    );
    ui.add_space(4.0);
    let current = CollectorKind::from_config(&config.general.collector);
    let datasource_name = |i18n: &I18n, kind: CollectorKind| match kind {
        CollectorKind::Real => i18n.t("datasource-real"),
        CollectorKind::Mock => i18n.t("datasource-mock"),
    };
    egui::ComboBox::from_id_salt("settings-datasource-select")
        .width(180.0)
        .selected_text(datasource_name(i18n, current))
        .show_ui(ui, |ui| {
            for kind in [CollectorKind::Real, CollectorKind::Mock] {
                let selected = current == kind;
                let label =
                    RichText::new(datasource_name(i18n, kind))
                        .size(14.0)
                        .color(if selected {
                            theme::c().accent
                        } else {
                            theme::c().text
                        });
                if ui.selectable_label(selected, label).clicked() {
                    config.general.collector = kind.as_config().to_owned();
                }
            }
        });
    ui.add_space(6.0);
    ui.label(theme::dim_text(&i18n.t("settings-datasource-hint"), 12.0));
    ui.add_space(16.0);

    // 历史数据自动清理天数(0 = 不自动清理)
    ui.label(
        RichText::new(i18n.t("settings-history-days"))
            .size(14.0)
            .strong()
            .color(theme::c().text),
    );
    ui.add_space(4.0);
    let days = ui.add(
        egui::DragValue::new(&mut config.general.history_days)
            .range(0..=365)
            .suffix(format!(" {}", i18n.t("settings-history-days-unit"))),
    );
    ui.add_space(6.0);
    ui.label(theme::dim_text(&i18n.t("settings-history-days-hint"), 12.0));
    ui.add_space(16.0);

    // 新连接询问弹窗(默认关闭;开启后未命中规则的公网新连接弹窗询问)
    ui.label(
        RichText::new(i18n.t("settings-ask"))
            .size(14.0)
            .strong()
            .color(theme::c().text),
    );
    ui.add_space(4.0);
    let ask = ui.checkbox(
        &mut config.general.ask_connections,
        i18n.t("settings-ask-on"),
    );
    ui.add_space(6.0);
    ui.label(theme::dim_text(&i18n.t("settings-ask-hint"), 12.0));
    ui.add_space(16.0);

    // 托盘图标常驻(注册表 IsPromoted,写入失败静默保持系统默认;新会话生效)
    ui.label(
        RichText::new(i18n.t("settings-tray-pin"))
            .size(14.0)
            .strong()
            .color(theme::c().text),
    );
    ui.add_space(4.0);
    let tray = ui.checkbox(
        &mut config.general.tray_pinned,
        i18n.t("settings-tray-pin-on"),
    );
    ui.add_space(6.0);
    ui.label(theme::dim_text(&i18n.t("settings-tray-pin-hint"), 12.0));
    ui.add_space(16.0);

    // 运行日志:级别下拉立即 reload;文件开关切换建/停写线程;查看入口
    // 打开日志浏览窗口(内存层收集,诊断用;参照 wallwarp)
    ui.label(
        RichText::new(i18n.t("settings-log"))
            .size(14.0)
            .strong()
            .color(theme::c().text),
    );
    ui.add_space(4.0);
    let mut changed_log = false;
    ui.horizontal(|ui| {
        ui.label(theme::dim_text(&i18n.t("settings-log-level"), 13.0));
        let current = LogLevel::parse(&config.general.log_level);
        egui::ComboBox::from_id_salt("settings-log-level")
            .width(110.0)
            .selected_text(i18n.t(log_window::level_key(current)))
            .show_ui(ui, |ui| {
                for level in LogLevel::ALL {
                    if ui
                        .selectable_label(
                            current == level,
                            RichText::new(i18n.t(log_window::level_key(level))).size(13.0),
                        )
                        .clicked()
                    {
                        config.general.log_level = level.as_str().to_owned();
                        crate::logging::set_level(level);
                    }
                }
            });
        ui.add_space(12.0);
        if ui
            .checkbox(
                &mut config.general.log_to_file,
                i18n.t("settings-log-file-on"),
            )
            .changed()
        {
            crate::logging::set_file_enabled(config.general.log_to_file);
            changed_log = true;
        }
        ui.add_space(12.0);
        if ui.button(i18n.t("settings-log-view")).clicked() {
            log_window::open(log_window);
        }
    });
    ui.add_space(6.0);
    ui.label(theme::dim_text(&i18n.t("settings-log-hint"), 12.0));
    days.changed() || ask.changed() || tray.changed() || changed_log
}
