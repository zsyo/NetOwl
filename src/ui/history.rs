//! 历史页:明细/聚合双视图、时间档位与进程/远端/协议筛选、
//! 数据库大小展示、超容提醒与手动清空。

use std::collections::HashMap;

use eframe::egui;
use egui::{CornerRadius, Frame, Label, Margin, RichText, Stroke, containers::menu::MenuButton};
use rusqlite::Connection as Db;

use crate::i18n::I18n;
use crate::model::{Place, Protocol, fmt_bytes};
use crate::net::geoip;
use crate::storage::config::Config;
use crate::storage::history;
use crate::storage::history_query::{self, AggregateSort, Rows, SummarySort, ViewMode};
use crate::ui::{TOOLBAR_ROW_H, icons, theme, widgets};

/// 历史页;返回是否直接改动了配置(勾选不再提醒/清空还原提醒)
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    state: &mut history_query::PageState,
    i18n: &I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    db: &Db,
    writer: &history::Writer,
    config: &mut Config,
    row_hover: &mut widgets::table::RowHover,
) -> bool {
    state.refresh_if_needed(db, config.general.hide_local, config.general.hide_lan);
    let mut config_changed = false;

    widgets::header::page_header(ui, &i18n.t("history-title"), &i18n.t("history-subtitle"));
    ui.add_space(theme::sp::SM);

    // 超容提醒:大小超阈值且用户未勾选"不再提醒"时展示
    if state.db_size > history_query::REMIND_SIZE && config.general.history_remind {
        config_changed |= remind_card(ui, state, i18n, config);
        ui.add_space(theme::sp::SM);
    }

    toolbar(ui, state, i18n, db, writer, config, &mut config_changed);
    ui.add_space(theme::sp::SM);

    rows_table(ui, state, i18n, icon_tex, default_icon_tex, row_hover);
    config_changed
}

/// 超容提醒卡片(warn 低透明底 + 警示图标);勾选"不再提醒"写 config 持久化
fn remind_card(
    ui: &mut egui::Ui,
    state: &history_query::PageState,
    i18n: &I18n,
    config: &mut Config,
) -> bool {
    let mut changed = false;
    let warn = theme::c().status_warn;
    Frame::new()
        .fill(warn.gamma_multiply(0.10))
        .stroke(Stroke::new(1.0, warn.gamma_multiply(0.4)))
        .corner_radius(CornerRadius::same(theme::RADIUS_MD))
        .inner_margin(Margin::same(theme::sp::MD as i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(icons::EXCLAMATION_CIRCLE)
                        .size(theme::font::H3)
                        .color(warn),
                );
                let text =
                    i18n.t_with_args("history-remind-text", &[("size", fmt_bytes(state.db_size))]);
                ui.label(
                    RichText::new(text)
                        .size(theme::font::BODY)
                        .color(theme::c().text),
                );
                if ui
                    .checkbox(&mut false, i18n.t("history-remind-dismiss"))
                    .clicked()
                {
                    config.general.history_remind = false;
                    changed = true;
                }
            });
        });
    changed
}

/// 工具栏:视图切换、筛选与刷新(左),库大小与清空(右)
fn toolbar(
    ui: &mut egui::Ui,
    state: &mut history_query::PageState,
    i18n: &I18n,
    db: &Db,
    writer: &history::Writer,
    config: &mut Config,
    config_changed: &mut bool,
) {
    ui.style_mut().spacing.interact_size.y = TOOLBAR_ROW_H;
    ui.horizontal(|ui| {
        // 视图切换(segmented:明细/聚合/汇总)
        let view_items = [
            (&*i18n.t("history-view-detail"), icons::VIEW_LIST),
            (&*i18n.t("history-view-aggregate"), icons::VIEW_STACKED),
            (&*i18n.t("history-view-summary"), icons::BAR_CHART_LINE),
        ];
        let view_idx = match state.view {
            ViewMode::Detail => 0,
            ViewMode::Aggregate => 1,
            ViewMode::Summary => 2,
        };
        if let Some(i) = widgets::segmented::segmented(ui, &view_items, view_idx) {
            state.view = match i {
                0 => ViewMode::Detail,
                1 => ViewMode::Aggregate,
                _ => ViewMode::Summary,
            };
            state.dirty = true;
        }

        // 时间范围
        let ranges = [
            (history_query::Range::LastHour, "history-range-1h"),
            (history_query::Range::Last6Hours, "history-range-6h"),
            (history_query::Range::Last24Hours, "history-range-24h"),
            (history_query::Range::Last7Days, "history-range-7d"),
            (history_query::Range::ThisMonth, "history-range-month"),
        ];
        let range_name = |i18n: &I18n, r: history_query::Range| {
            ranges
                .iter()
                .find(|(k, _)| *k == r)
                .map(|(_, key)| i18n.t(key))
                .unwrap_or_default()
        };
        egui::ComboBox::from_id_salt("history-range")
            .width(120.0)
            .selected_text(RichText::new(range_name(i18n, state.range)).size(theme::font::BODY))
            .show_ui(ui, |ui| {
                for (r, key) in ranges {
                    if ui
                        .selectable_label(
                            state.range == r,
                            RichText::new(i18n.t(key)).size(theme::font::BODY),
                        )
                        .clicked()
                    {
                        state.range = r;
                        state.dirty = true;
                    }
                }
            });

        // 进程 / 远端筛选
        let process = ui.add(
            egui::TextEdit::singleline(&mut state.process)
                .hint_text(i18n.t("history-filter-process"))
                .font(egui::FontId::proportional(theme::font::BODY))
                .desired_width(110.0),
        );
        let remote = ui.add(
            egui::TextEdit::singleline(&mut state.remote)
                .hint_text(i18n.t("history-filter-remote"))
                .font(egui::FontId::proportional(theme::font::BODY))
                .desired_width(110.0),
        );

        // 协议
        egui::ComboBox::from_id_salt("history-proto")
            .width(90.0)
            .selected_text(RichText::new(proto_name(i18n, state.proto)).size(theme::font::BODY))
            .show_ui(ui, |ui| {
                for p in [None, Some(Protocol::Tcp), Some(Protocol::Udp)] {
                    if ui
                        .selectable_label(
                            state.proto == p,
                            RichText::new(proto_name(i18n, p)).size(theme::font::BODY),
                        )
                        .clicked()
                    {
                        state.proto = p;
                        state.dirty = true;
                    }
                }
            });

        if ui
            .button(RichText::new(i18n.t("history-refresh")).size(theme::font::BODY))
            .clicked()
        {
            state.dirty = true;
        }
        if process.changed() || remote.changed() || process.lost_focus() || remote.lost_focus() {
            state.dirty = true;
        }

        // 本地/局域网远端噪音过滤(config 持久化,连接页与历史页共享)
        let hide_local_text = RichText::new(i18n.t("filter-hide-local")).size(theme::font::BODY);
        if ui
            .checkbox(&mut config.general.hide_local, hide_local_text)
            .changed()
        {
            state.dirty = true;
            *config_changed = true;
        }
        let hide_lan_text = RichText::new(i18n.t("filter-hide-lan")).size(theme::font::BODY);
        if ui
            .checkbox(&mut config.general.hide_lan, hide_lan_text)
            .changed()
        {
            state.dirty = true;
            *config_changed = true;
        }

        // 库大小与手动清理(右对齐):点击弹出档位菜单
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let size = i18n.t_with_args("history-db-size", &[("size", fmt_bytes(state.db_size))]);
            ui.label(theme::dim_text(&size, theme::font::SM));
            let purge_button =
                egui::Button::new(RichText::new(i18n.t("history-purge")).size(theme::font::BODY))
                    .fill(theme::c().accent_soft)
                    .corner_radius(CornerRadius::same(theme::RADIUS_SM));
            MenuButton::from_button(purge_button).ui(ui, |ui| {
                ui.with_layout(
                    egui::Layout::top_down(egui::Align::LEFT).with_cross_justify(true),
                    |ui| {
                        for (days, text) in purge_choices(i18n) {
                            if ui
                                .selectable_label(
                                    false,
                                    RichText::new(text).size(theme::font::BODY),
                                )
                                .clicked()
                            {
                                state.request_purge(writer, days);
                                config.general.history_remind = true;
                                *config_changed = true;
                            }
                        }
                    },
                );
            });
        });
    });
    // 筛选文本失焦与开关变更即刷新(逐帧查询代价已由 dirty 门控)
    state.refresh_if_needed(db, config.general.hide_local, config.general.hide_lan);
}

/// 清理菜单档位:(删除天数, 词条文案);0 = 清空全部
fn purge_choices(i18n: &I18n) -> Vec<(u32, String)> {
    [
        (0, i18n.t("history-purge-all")),
        (
            3,
            i18n.t_with_args("history-purge-days", &[("n", "3".to_owned())]),
        ),
        (
            7,
            i18n.t_with_args("history-purge-days", &[("n", "7".to_owned())]),
        ),
        (30, i18n.t("history-purge-month")),
    ]
    .into_iter()
    .collect()
}

fn proto_name(i18n: &I18n, proto: Option<Protocol>) -> String {
    match proto {
        None => i18n.t("history-filter-proto-all"),
        Some(p) => p.as_str().to_owned(),
    }
}

/// 结果表:明细(6 列)/ 聚合(7 列)/ 汇总(5 列,表头可排序)
fn rows_table(
    ui: &mut egui::Ui,
    state: &mut history_query::PageState,
    i18n: &I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    row_hover: &mut widgets::table::RowHover,
) {
    match &state.rows {
        Rows::Detail(rows) => {
            if rows.is_empty() {
                empty_hint(ui, i18n);
                return;
            }
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    let table_left = ui.max_rect().left();
                    let table_right = ui.max_rect().right();
                    egui::Grid::new("history_detail")
                        .num_columns(9)
                        .striped(true)
                        .spacing([24.0, widgets::table::ROW_SPACING_Y])
                        .show(ui, |ui| {
                            for key in [
                                "history-col-process",
                                "col-proto",
                                "col-remote",
                                "col-location",
                                "history-col-first",
                                "history-col-duration",
                                "col-down-total",
                                "col-up-total",
                            ] {
                                widgets::table::header_cell(ui, &i18n.t(key));
                            }
                            ui.end_row();
                            for r in rows {
                                let row_top = ui.cursor().top();
                                row_hover.begin(ui, table_left, table_right, row_top);
                                proc_cell(
                                    ui,
                                    &r.process,
                                    Some(r.pid),
                                    r.proc_path.as_deref(),
                                    icon_tex,
                                    default_icon_tex,
                                    i18n,
                                );
                                widgets::badge::badge(
                                    ui,
                                    r.proto.as_str(),
                                    if r.proto.as_str() == "TCP" {
                                        widgets::badge::BadgeKind::Accent
                                    } else {
                                        widgets::badge::BadgeKind::Neutral
                                    },
                                );
                                ui.add(
                                    Label::new(
                                        RichText::new(format!("{}:{}", r.remote_ip, r.remote_port))
                                            .size(theme::font::BODY)
                                            .color(theme::c().text),
                                    )
                                    .wrap_mode(egui::TextWrapMode::Extend),
                                );
                                location_cell(ui, i18n, r.remote_ip);
                                ui.label(theme::dim_text(
                                    &history_query::fmt_local(r.first_seen),
                                    theme::font::BODY,
                                ));
                                ui.label(theme::dim_text(
                                    &history_query::fmt_duration(
                                        r.last_seen.saturating_sub(r.first_seen),
                                    ),
                                    theme::font::BODY,
                                ));
                                bytes_cell(ui, r.bytes_in, false);
                                bytes_cell(ui, r.bytes_out, true);
                                ui.label("");
                                ui.end_row();
                                row_hover.end(ui, row_top);
                            }
                        });
                    truncated_hint(ui, rows.len(), i18n);
                });
        }
        Rows::Aggregate(rows) => {
            if rows.is_empty() {
                empty_hint(ui, i18n);
                return;
            }
            let aggregate_sort = &mut state.aggregate_sort;
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    let table_left = ui.max_rect().left();
                    let table_right = ui.max_rect().right();
                    egui::Grid::new("history_aggregate")
                        .num_columns(10)
                        .striped(true)
                        .spacing([24.0, widgets::table::ROW_SPACING_Y])
                        .show(ui, |ui| {
                            // 进程/协议/远端/位置纯展示;
                            // 次数/时长/最近活动/下载总量/上传总量可排序(末列空占位同汇总表)
                            for key in [
                                "history-col-process",
                                "col-proto",
                                "col-remote",
                                "col-location",
                            ] {
                                widgets::table::header_cell(ui, &i18n.t(key));
                            }
                            let mut sort_clicked = false;
                            let mut sort_header =
                                |ui: &mut egui::Ui, key: &str, sort: AggregateSort| {
                                    let (cur, asc) = *aggregate_sort;
                                    let r = widgets::table::header_sort_cell(
                                        ui,
                                        &i18n.t(key),
                                        cur == sort,
                                        asc,
                                    );
                                    if r.clicked() {
                                        *aggregate_sort = if cur == sort {
                                            (sort, !asc)
                                        } else {
                                            (sort, false)
                                        };
                                        true
                                    } else {
                                        false
                                    }
                                };
                            for (key, sort) in [
                                ("history-col-count", AggregateSort::Count),
                                ("history-col-total", AggregateSort::TotalSecs),
                                ("history-col-last", AggregateSort::LastActive),
                                ("col-down-total", AggregateSort::BytesIn),
                                ("col-up-total", AggregateSort::BytesOut),
                            ] {
                                sort_clicked |= sort_header(ui, key, sort);
                            }
                            if sort_clicked {
                                state.dirty = true;
                            }
                            ui.label("");
                            ui.end_row();
                            for r in rows {
                                let row_top = ui.cursor().top();
                                row_hover.begin(ui, table_left, table_right, row_top);
                                proc_cell(
                                    ui,
                                    &r.process,
                                    None,
                                    None,
                                    icon_tex,
                                    default_icon_tex,
                                    i18n,
                                );
                                widgets::badge::badge(
                                    ui,
                                    r.proto.as_str(),
                                    if r.proto.as_str() == "TCP" {
                                        widgets::badge::BadgeKind::Accent
                                    } else {
                                        widgets::badge::BadgeKind::Neutral
                                    },
                                );
                                ui.add(
                                    Label::new(
                                        RichText::new(r.remote_ip.to_string())
                                            .size(theme::font::BODY)
                                            .color(theme::c().text),
                                    )
                                    .wrap_mode(egui::TextWrapMode::Extend),
                                );
                                location_cell(ui, i18n, r.remote_ip);
                                ui.label(
                                    RichText::new(r.count.to_string())
                                        .size(theme::font::BODY)
                                        .color(theme::c().text),
                                );
                                ui.label(theme::dim_text(
                                    &history_query::fmt_duration(r.total_secs),
                                    theme::font::BODY,
                                ));
                                ui.label(theme::dim_text(
                                    &history_query::fmt_local(r.last_active),
                                    theme::font::BODY,
                                ));
                                bytes_cell(ui, r.bytes_in, false);
                                bytes_cell(ui, r.bytes_out, true);
                                ui.label("");
                                ui.end_row();
                                row_hover.end(ui, row_top);
                            }
                        });
                    truncated_hint(ui, rows.len(), i18n);
                });
        }
        Rows::Summary(rows) => {
            if rows.is_empty() {
                empty_hint(ui, i18n);
                return;
            }
            // 口径注记:字节随连接完结定稿入库,活跃连接关闭后才计入统计
            ui.label(theme::dim_text(
                &i18n.t("history-summary-note"),
                theme::font::SM,
            ));
            ui.add_space(theme::sp::XS);
            let summary_sort = &mut state.summary_sort;
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    let table_left = ui.max_rect().left();
                    let table_right = ui.max_rect().right();
                    // 表格总宽必须在 Grid 之外取:Grid 闭包内 available_width
                    // 被 grid 布局器接管,返回当前列宽(上帧值)而非总宽
                    let table_w = ui.available_width();
                    egui::Grid::new("history_summary")
                        .num_columns(6)
                        .striped(true)
                        .spacing([24.0, widgets::table::ROW_SPACING_Y])
                        .show(ui, |ui| {
                            // 进程列纯展示;上传/下载/次数/时长可排序,
                            // 异键点击默认降序,同键翻转(汇总看 Top 异常,降序优先)。
                            // 末列是空占位:num_cell 右对齐占满可用宽,
                            // 作为末列时会横跨到表格右缘,须由空列兜住
                            // 数字列定宽(容纳表头与内容),进程列吃剩余宽度:
                            // 汇总仅 5 列,全按内容自适应时右侧留白过大;
                            // 定宽贴近内容上限,避免表头(贴左)与数据(右对齐)错位过大
                            const SUM_UP_W: f32 = 85.0;
                            const SUM_DOWN_W: f32 = 85.0;
                            const SUM_CNT_W: f32 = 65.0;
                            const SUM_DUR_W: f32 = 85.0;
                            let flex_w = (table_w
                                - 24.0 * 5.0
                                - (SUM_UP_W + SUM_DOWN_W + SUM_CNT_W + SUM_DUR_W))
                                .max(240.0);
                            widgets::table::header_cell(ui, &i18n.t("history-col-process"));
                            let mut sort_clicked = false;
                            let mut sort_header =
                                |ui: &mut egui::Ui, key: &str, sort: SummarySort| {
                                    let (cur, asc) = *summary_sort;
                                    let r = widgets::table::header_sort_cell(
                                        ui,
                                        &i18n.t(key),
                                        cur == sort,
                                        asc,
                                    );
                                    if r.clicked() {
                                        *summary_sort = if cur == sort {
                                            (sort, !asc)
                                        } else {
                                            (sort, false)
                                        };
                                        true
                                    } else {
                                        false
                                    }
                                };
                            for (key, sort) in [
                                ("col-up-total", SummarySort::BytesOut),
                                ("col-down-total", SummarySort::BytesIn),
                                ("history-col-count", SummarySort::Count),
                                ("history-col-total", SummarySort::TotalSecs),
                            ] {
                                sort_clicked |= sort_header(ui, key, sort);
                            }
                            if sort_clicked {
                                state.dirty = true;
                            }
                            ui.label("");
                            ui.end_row();

                            for r in rows {
                                let row_top = ui.cursor().top();
                                row_hover.begin(ui, table_left, table_right, row_top);
                                fixed_cell(ui, flex_w, |ui| {
                                    proc_cell(
                                        ui,
                                        &r.process,
                                        None,
                                        r.proc_path.as_deref(),
                                        icon_tex,
                                        default_icon_tex,
                                        i18n,
                                    );
                                });
                                fixed_num_cell(ui, SUM_UP_W, |ui| {
                                    bytes_cell(ui, r.bytes_out, true)
                                });
                                fixed_num_cell(ui, SUM_DOWN_W, |ui| {
                                    bytes_cell(ui, r.bytes_in, false)
                                });
                                fixed_num_cell(ui, SUM_CNT_W, |ui| {
                                    widgets::table::num_cell(
                                        ui,
                                        r.count.to_string(),
                                        theme::c().text,
                                    );
                                });
                                fixed_num_cell(ui, SUM_DUR_W, |ui| {
                                    widgets::table::num_cell(
                                        ui,
                                        history_query::fmt_duration(r.total_secs),
                                        theme::c().text,
                                    );
                                });
                                ui.label("");
                                ui.end_row();
                                row_hover.end(ui, row_top);
                            }
                        });
                    truncated_hint(ui, rows.len(), i18n);
                });
        }
    }
}

fn empty_hint(ui: &mut egui::Ui, i18n: &I18n) {
    ui.add_space(theme::sp::XL);
    ui.label(theme::dim_text(&i18n.t("history-empty"), theme::font::H3));
}

fn truncated_hint(ui: &mut egui::Ui, len: usize, i18n: &I18n) {
    if len >= history_query::QUERY_LIMIT {
        ui.add_space(theme::sp::SM);
        ui.label(theme::dim_text(
            &i18n.t_with_args(
                "history-truncated",
                &[("n", history_query::QUERY_LIMIT.to_string())],
            ),
            theme::font::SM,
        ));
    }
}

/// 字节总量单元格:0 弱化为灰(空载噪音),有值按方向语义色
fn bytes_cell(ui: &mut egui::Ui, bytes: u64, outbound: bool) {
    let c = theme::c();
    let color = if bytes == 0 {
        c.text_dim
    } else if outbound {
        c.outbound
    } else {
        c.inbound
    };
    widgets::table::num_cell(ui, fmt_bytes(bytes), color);
}

/// 定宽单元格容器:先在父布局精确占位(推进光标、参与 Grid 列宽测量),
/// 子 UI 画进该矩形。allocate_ui_with_layout 的尺寸是上限、内容小会收缩,
/// 不能用于定宽列
fn fixed_cell(ui: &mut egui::Ui, w: f32, add: impl FnOnce(&mut egui::Ui)) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 18.0), egui::Sense::hover());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.with_layout(egui::Layout::left_to_right(egui::Align::Center), add);
}

/// 定宽右对齐单元格容器:列宽固定时把右对齐数字限制在列宽内
fn fixed_num_cell(ui: &mut egui::Ui, w: f32, add: impl FnOnce(&mut egui::Ui)) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 18.0), egui::Sense::hover());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
}

/// 进程单元格:图标 + 名称(未知进程占位),聚合行无 PID
fn proc_cell(
    ui: &mut egui::Ui,
    name: &str,
    pid: Option<u32>,
    path: Option<&str>,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    i18n: &I18n,
) {
    ui.horizontal(|ui| {
        let tex = path.and_then(|p| icon_tex.get(p)).and_then(|t| t.as_ref());
        widgets::process::proc_icon(ui, tex, default_icon_tex, 16.0);
        let text = match (name.is_empty(), pid) {
            (true, Some(pid)) => {
                format!("{} (PID {pid})", i18n.t("conn-proc-unknown"))
            }
            (true, None) => i18n.t("conn-proc-unknown"),
            (false, Some(pid)) => format!("{name} ({pid})"),
            (false, None) => name.to_owned(),
        };
        ui.add(
            Label::new(
                RichText::new(text)
                    .size(theme::font::BODY)
                    .color(theme::c().text),
            )
            .wrap_mode(egui::TextWrapMode::Extend),
        );
    });
}

/// 位置单元格:归属地实时反查(geoip 数据重建不影响历史行),未知占位
fn location_cell(ui: &mut egui::Ui, i18n: &I18n, ip: std::net::Ipv4Addr) {
    let text = match geoip::locate(ip).map(Place::Geo) {
        Some(p) => geoip::place_label(p, i18n),
        None => i18n.t("conn-loc-unknown"),
    };
    ui.label(theme::dim_text(&text, theme::font::BODY));
}
