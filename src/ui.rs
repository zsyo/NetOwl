//! 主窗口布局:导航侧栏与各页面(地图 / 连接 / 规则占位 / 设置)。
//! 全部界面文本经 I18n 词条获取(AGENTS.md 规范 4)。

use std::collections::HashMap;

use eframe::egui;
use egui::{Button, Color32, CornerRadius, Frame, Label, Margin, RichText, Stroke};

use crate::basemap;
use crate::collector::CollectorKind;
use crate::config::Config;
use crate::geoip;
use crate::history;
use crate::history_query;
use crate::i18n::I18n;
use crate::map;
use crate::model::{Connection, Signing, fmt_bytes};
use crate::rdns;
use crate::theme;
use crate::ui_history;

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
    ui.label(RichText::new(i18n.t("app-name")).size(22.0).strong().color(theme::c().accent));
    ui.label(theme::dim_text(&i18n.t("app-subtitle"), 10.0));
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
                .fill(if selected { theme::c().accent_soft } else { Color32::TRANSPARENT })
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

    // 底部状态区(bottom_up:先绘制的贴底)
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        ui.add_space(8.0);
        // 实时总速率(GetIfTable2 接口字节采样差值);bottom_up 先绘制的贴底,
        // 故先上传后下载,让下载行在上
        ui.label(
            RichText::new(i18n.t_with_args(
                "nav-rate-up",
                &[("rate", format!("{}/s", fmt_bytes(rates.1)))],
            ))
            .size(12.0)
            .color(theme::c().outbound),
        );
        ui.label(
            RichText::new(i18n.t_with_args(
                "nav-rate-down",
                &[("rate", format!("{}/s", fmt_bytes(rates.0)))],
            ))
            .size(12.0)
            .color(theme::c().inbound),
        );
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 4.0, theme::c().status_ok);
            ui.label(theme::dim_text(&i18n.t("status-monitoring"), 13.0));
        });
        ui.label(
            RichText::new(i18n.t_with_args("status-conn-count", &[("count", conns.len().to_string())]))
                .size(11.0)
                .color(theme::c().text_dim),
        );
        let status_key = match collector_kind {
            CollectorKind::Real => "status-monitoring",
            CollectorKind::Mock => "status-mock",
        };
        ui.label(
            RichText::new(i18n.t(status_key))
                .size(11.0)
                .color(theme::c().text_dim),
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
    /// 进程图标纹理(键 = 映像路径);None 表示已提取且无图标
    pub icon_tex: &'a HashMap<String, Option<egui::TextureHandle>>,
    /// 历史页状态
    pub history: &'a mut history_query::PageState,
    /// 历史查询只读连接
    pub history_db: &'a history::Db,
    /// 历史写线程句柄(手动清空)
    pub writer: &'a history::Writer,
    pub local_pos: (f32, f32),
}

/// 中央区域按页面分发;返回本轮是否直接改动了配置(由 App 层标脏落盘)
pub fn central_ui(ui: &mut egui::Ui, page: &Page, ctx: &mut UiCtx) -> bool {
    match page {
        Page::Map => {
            map::draw(ui, ctx.conns, ctx.i18n, ctx.map_view, ctx.rdns, ctx.icon_tex, ctx.local_pos);
            false
        }
        Page::Connections => connections_ui(ui, ctx.conns, ctx.i18n, ctx.rdns, ctx.icon_tex, ctx.config),
        Page::History => ui_history::show(
            ui,
            ctx.history,
            ctx.i18n,
            ctx.icon_tex,
            ctx.history_db,
            ctx.writer,
            ctx.config,
        ),
        Page::Rules => {
            placeholder_ui(
                ui,
                &ctx.i18n.t("rules-title"),
                &ctx.i18n.t("rules-placeholder"),
            );
            false
        }
        Page::Settings => settings_ui(ui, ctx.config, ctx.i18n),
    }
}

/// 连接列表页;返回是否直接改动了配置(隐藏本地/局域网开关)
fn connections_ui(
    ui: &mut egui::Ui,
    conns: &[Connection],
    i18n: &I18n,
    rdns: &rdns::Rdns,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    config: &mut Config,
) -> bool {
    ui.heading(theme::accent_text(&i18n.t("conns-title"), 20.0));
    ui.label(theme::dim_text(&i18n.t("conns-subtitle"), 13.0));
    ui.add_space(6.0);

    // 本地/局域网远端噪音过滤(config 持久化,连接页与历史页共享)
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui.checkbox(&mut config.general.hide_local, i18n.t("filter-hide-local")).changed() {
            changed = true;
        }
        if ui.checkbox(&mut config.general.hide_lan, i18n.t("filter-hide-lan")).changed() {
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
                .num_columns(6)
                .spacing([24.0, 9.0])
                .striped(true)
                .show(ui, |ui| {
                    for key in ["col-process", "col-proto", "col-remote", "col-location", "col-down", "col-up"] {
                        ui.label(RichText::new(i18n.t(key)).size(12.0).strong().color(theme::c().text_dim));
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
                                        ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(16.0, 16.0)));
                                    }
                                    None => {
                                        ui.allocate_exact_size(
                                            egui::vec2(16.0, 16.0),
                                            egui::Sense::hover(),
                                        );
                                    }
                                }
                                ui.add(
                                    Label::new(RichText::new(process).size(13.0).color(theme::c().text))
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
                        ui.label(
                            RichText::new(fmt_bytes(conn.bytes_in)).size(13.0).color(theme::c().inbound),
                        );
                        ui.label(
                            RichText::new(fmt_bytes(conn.bytes_out)).size(13.0).color(theme::c().outbound),
                        );
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
    ui.label(RichText::new(i18n.t("settings-language")).size(14.0).strong().color(theme::c().text));
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
    ui.label(RichText::new(i18n.t("settings-theme")).size(14.0).strong().color(theme::c().text));
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
    ui.label(RichText::new(i18n.t("settings-datasource")).size(14.0).strong().color(theme::c().text));
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
                let label = RichText::new(datasource_name(i18n, kind))
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
    ui.label(RichText::new(i18n.t("settings-history-days")).size(14.0).strong().color(theme::c().text));
    ui.add_space(4.0);
    let days = ui.add(
        egui::DragValue::new(&mut config.general.history_days)
            .range(0..=365)
            .suffix(format!(" {}", i18n.t("settings-history-days-unit"))),
    );
    ui.add_space(6.0);
    ui.label(theme::dim_text(&i18n.t("settings-history-days-hint"), 12.0));
    days.changed()
}

/// 占位页统一卡片
fn placeholder_ui(ui: &mut egui::Ui, title: &str, body: &str) {
    ui.heading(theme::accent_text(title, 20.0));
    ui.add_space(20.0);
    Frame::new()
        .fill(theme::c().bg_card)
        .stroke(Stroke::new(1.0, theme::c().stroke))
        .corner_radius(CornerRadius::same(theme::RADIUS_LG))
        .inner_margin(Margin::same(20))
        .show(ui, |ui| {
            ui.set_max_width(440.0);
            ui.label(RichText::new(body).size(13.0).color(theme::c().text_dim));
        });
}
