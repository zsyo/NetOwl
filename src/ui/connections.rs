//! 连接列表页:表格、排序与规则求值标注。

use std::collections::HashMap;

use eframe::egui;
use egui::{Label, RichText};

use super::{ConnSort, ConnSortState, UiCtx, conn_visible, icons, theme, widgets};
use crate::i18n::I18n;
use crate::model::{Connection, Signing, fmt_bytes};
use crate::net::geoip;
use crate::net::rdns;
use crate::rules as rules_engine;

/// 连接列表页;返回是否直接改动了配置(隐藏本地/局域网开关)。
/// 末列显示规则求值动作(允许/阻断,规则引擎默认放行)
pub(super) fn connections_ui(ui: &mut egui::Ui, ctx: &mut UiCtx) -> bool {
    let UiCtx {
        conns,
        i18n,
        rdns,
        icon_tex,
        default_icon_tex,
        config,
        rules,
        conn_rates,
        conn_sort,
        conn_row_hover,
        ..
    } = ctx;
    // &mut UiCtx 解构出的引用字段带两层 &mut,借类型注解 coerce 回单层
    let conn_sort: &mut ConnSortState = conn_sort;
    let conn_row_hover: &mut widgets::table::RowHover = conn_row_hover;
    widgets::header::page_header(ui, &i18n.t("conns-title"), &i18n.t("conns-subtitle"));
    ui.add_space(theme::sp::SM);

    // 本地/局域网远端噪音过滤(config 持久化,连接页与历史页共享)
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(icons::FUNNEL)
                .size(theme::font::XS)
                .color(theme::c().text_dim),
        );
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
    ui.add_space(theme::sp::XS);

    // 空态判定与过滤同口径:全部连接都被隐藏时同样提示无连接
    let mut shown: Vec<&Connection> = conns.iter().filter(|c| conn_visible(config, c)).collect();
    sort_conns(&mut shown, conn_sort, conn_rates, i18n);
    if shown.is_empty() {
        ui.add_space(theme::sp::LG);
        ui.label(theme::dim_text(&i18n.t("conns-empty"), theme::font::H3));
        return changed;
    }

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let table_left = ui.max_rect().left();
            let table_right = ui.max_rect().right();
            egui::Grid::new("connections_grid")
                .num_columns(9)
                .striped(true)
                .spacing([24.0, widgets::table::ROW_SPACING_Y])
                .show(ui, |ui| {
                    // 表头:可排序列整格可点击(当前排序列高亮并带方向三角),
                    // 协议/远端/动作为纯展示列,不给手型光标(不可点)
                    let mut header = |ui: &mut egui::Ui, key: &str, sort: Option<ConnSort>| {
                        let active = conn_sort.is_some_and(|(k, _)| Some(k) == sort);
                        match sort {
                            Some(s) => {
                                let ascending = conn_sort.is_some_and(|(_, asc)| asc);
                                let r = widgets::table::header_sort_cell(
                                    ui,
                                    &i18n.t(key),
                                    active,
                                    ascending,
                                );
                                if r.clicked() {
                                    let current: ConnSortState = *conn_sort;
                                    *conn_sort = Some(match current {
                                        Some((k, asc)) if k == s => (s, !asc),
                                        _ => (s, true),
                                    });
                                }
                            }
                            None => widgets::table::header_cell(ui, &i18n.t(key)),
                        }
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
                        let row_top = ui.cursor().top();
                        conn_row_hover.begin(ui, table_left, table_right, row_top);
                        let process = if conn.process.is_empty() {
                            format!("{} (PID {})", i18n.t("conn-proc-unknown"), conn.pid)
                        } else {
                            conn.process.clone()
                        };
                        // 进程列两行:映像名(带图标)+ 弱化的签名状态与路径。
                        // 无图标的进程也占位,保证各行文字起点对齐不跳动
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                let tex = conn
                                    .proc_path
                                    .as_deref()
                                    .and_then(|p| icon_tex.get(p))
                                    .and_then(|t| t.as_ref());
                                widgets::process::proc_icon(ui, tex, *default_icon_tex, 16.0);
                                ui.add(
                                    Label::new(
                                        RichText::new(process)
                                            .size(theme::font::BODY)
                                            .color(theme::c().text),
                                    )
                                    .wrap_mode(egui::TextWrapMode::Extend),
                                );
                            });
                            ui.add(
                                Label::new(theme::dim_text(
                                    &proc_detail(conn, i18n),
                                    theme::font::XS,
                                ))
                                .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        });
                        // 协议徽章:TCP 强调 / UDP 弱化
                        let proto_kind = if conn.proto.as_str() == "TCP" {
                            widgets::badge::BadgeKind::Accent
                        } else {
                            widgets::badge::BadgeKind::Neutral
                        };
                        widgets::badge::badge(ui, conn.proto.as_str(), proto_kind);
                        // rDNS 域名优先,域名下方弱化显示裸 IP;无 PTR 回退地址:端口。
                        // 单行延伸(Extend)禁用自动折行,列宽由最宽内容撑开
                        ui.vertical(|ui| match rdns.lookup(conn.remote_ip) {
                            Some(host) => {
                                ui.add(
                                    Label::new(
                                        RichText::new(rdns::display(host, conn.remote_port, 36))
                                            .size(theme::font::BODY)
                                            .color(theme::c().text),
                                    )
                                    .wrap_mode(egui::TextWrapMode::Extend),
                                );
                                ui.add(
                                    Label::new(theme::dim_text(
                                        &conn.remote_ip.to_string(),
                                        theme::font::XS,
                                    ))
                                    .wrap_mode(egui::TextWrapMode::Extend),
                                );
                            }
                            None => {
                                ui.add(
                                    Label::new(
                                        RichText::new(conn.remote_display())
                                            .size(theme::font::BODY)
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
                        ui.label(theme::dim_text(&location, theme::font::BODY));
                        // 下载/上传列显示实时速率(ETW 字节差值),悬停显示累计字节;
                        // 未提权时 ETW 未启动,速率恒 0
                        let (rin, rout) = conn_rates.get(&conn.id).copied().unwrap_or((0, 0));
                        widgets::table::num_cell(
                            ui,
                            format!("{}/s", fmt_bytes(rin)),
                            theme::c().inbound,
                        )
                        .on_hover_text(format!(
                            "{} {}",
                            i18n.t("conn-total-bytes"),
                            fmt_bytes(conn.bytes_in)
                        ));
                        widgets::table::num_cell(
                            ui,
                            format!("{}/s", fmt_bytes(rout)),
                            theme::c().outbound,
                        )
                        .on_hover_text(format!(
                            "{} {}",
                            i18n.t("conn-total-bytes"),
                            fmt_bytes(conn.bytes_out)
                        ));
                        // 累计字节列(速率列的悬停信息在此显式展示)
                        widgets::table::num_cell(ui, fmt_bytes(conn.bytes_in), theme::c().inbound);
                        widgets::table::num_cell(
                            ui,
                            fmt_bytes(conn.bytes_out),
                            theme::c().outbound,
                        );
                        // 规则求值:命中规则的连接标注动作徽章,未命中默认放行不标注
                        match rules.evaluate(&rules_engine::MatchReq::from_conn(
                            conn,
                            rdns.lookup(conn.remote_ip),
                        )) {
                            Some(hit) => {
                                let (key, kind) = match hit.action {
                                    rules_engine::Action::Allow => {
                                        ("conn-action-allow", widgets::badge::BadgeKind::Ok)
                                    }
                                    rules_engine::Action::Block => {
                                        ("conn-action-block", widgets::badge::BadgeKind::Danger)
                                    }
                                };
                                widgets::badge::badge(ui, &i18n.t(key), kind);
                            }
                            None => {
                                ui.label("");
                            }
                        }
                        ui.end_row();
                        conn_row_hover.end(ui, row_top);
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
