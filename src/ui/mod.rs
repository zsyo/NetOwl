//! 主窗口布局:导航侧栏与各页面(地图 / 连接 / 规则占位 / 设置)。
//! 全部界面文本经 I18n 词条获取(AGENTS.md 规范 4)。

pub mod ask;
pub mod history;
pub mod rules;
pub mod theme;

use std::collections::HashMap;

use eframe::egui;
use egui::{Button, Color32, CornerRadius, Label, RichText, Stroke};

use crate::collector::CollectorKind;
use crate::i18n::I18n;
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

/// 主窗口页面
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Map,
    Connections,
    History,
    Rules,
    Settings,
}

const NAV_ITEMS: &[(Page, &str)] = &[
    (Page::Map, "nav-map"),
    (Page::Connections, "nav-connections"),
    (Page::History, "nav-history"),
    (Page::Rules, "nav-rules"),
    (Page::Settings, "nav-settings"),
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

    for (target, key) in NAV_ITEMS {
        let selected = page == target;
        let label_text = RichText::new(i18n.t(key)).size(15.0).color(if selected {
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

/// 侧栏字体下文本宽度(居中偏移计算用)
fn text_width(ui: &egui::Ui, text: &str, size: f32) -> f32 {
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
    /// 进程图标纹理(键 = 映像路径);None 表示已提取且无图标
    pub icon_tex: &'a HashMap<String, Option<egui::TextureHandle>>,
    /// 历史页状态
    pub history: &'a mut history_query::PageState,
    /// 历史查询只读连接
    pub history_db: &'a history_store::Db,
    /// 规则集(连接页求值与规则页编辑)
    pub rules: &'a mut rules_engine::RuleSet,
    /// 规则页状态
    pub rules_page: &'a mut rules::PageState,
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
            map::draw(
                ui,
                ctx.conns,
                ctx.i18n,
                ctx.map_view,
                ctx.rdns,
                ctx.icon_tex,
                ctx.local_pos,
            );
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
        Page::Settings => settings_ui(ui, ctx.config, ctx.i18n),
    }
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
        ..
    } = ctx;
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
    let visible = |c: &Connection| {
        !(config.general.hide_local && c.remote_ip.is_loopback())
            && !(config.general.hide_lan && c.remote_ip.is_private())
    };
    let shown: Vec<&Connection> = conns.iter().filter(|c| visible(c)).collect();
    if shown.is_empty() {
        ui.label(theme::dim_text(&i18n.t("conns-empty"), 14.0));
        return changed;
    }

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            egui::Grid::new("connections_grid")
                .num_columns(7)
                .spacing([24.0, 9.0])
                .striped(true)
                .show(ui, |ui| {
                    for key in [
                        "col-process",
                        "col-proto",
                        "col-remote",
                        "col-location",
                        "col-down",
                        "col-up",
                        "col-action",
                    ] {
                        ui.label(
                            RichText::new(i18n.t(key))
                                .size(12.0)
                                .strong()
                                .color(theme::c().text_dim),
                        );
                    }
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

/// 设置页:语言切换(词条即时生效)、主题、数据源;返回是否直接改动了配置
fn settings_ui(ui: &mut egui::Ui, config: &mut Config, i18n: &mut I18n) -> bool {
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
    days.changed() || ask.changed()
}
