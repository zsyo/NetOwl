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

    rows_table(ui, state, i18n, icon_tex, default_icon_tex);
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

/// 结果表:明细(8 列)/ 聚合(9 列)/ 汇总(5 列,表头可排序)
/// 三视图均行虚拟化(show_rows 只布局可见行):查询上限 500 行,
/// 窗口缩放/滚动每帧全量重排会卡顿。行高恒定 22,行色手动垫底
/// (虚拟化后 Grid striped 的奇偶不再对应全局行号);
/// 表头固定在滚动区之外(show_rows 的行定位不含表头,混入会整体错位),
/// 列宽与数据 Grid 同一公式计算保持对齐
fn rows_table(
    ui: &mut egui::Ui,
    state: &mut history_query::PageState,
    i18n: &I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
) {
    match &state.rows {
        Rows::Detail(rows) => {
            if rows.is_empty() {
                empty_hint(ui, i18n);
                return;
            }
            const D_PROTO_W: f32 = 56.0;
            const D_LOC_W: f32 = 112.0;
            const D_SEEN_W: f32 = 105.0;
            const D_DUR_W: f32 = 80.0;
            const D_BYTES_W: f32 = 90.0;
            // 表格总宽在虚拟化容器之外取(Grid 闭包内 available_width 被
            // grid 布局器接管,返回当前列宽而非总宽)
            let table_w = ui.available_width();
            let flex_total = (table_w
                - 24.0 * 7.0
                - (D_PROTO_W + D_LOC_W + D_SEEN_W + D_DUR_W + D_BYTES_W * 2.0))
                .max(320.0);
            let flex_w = flex_total * 0.5;
            egui::Grid::new("history_detail_header")
                .num_columns(8)
                .spacing([24.0, 0.0])
                .show(ui, |ui| {
                    for (key, w, right) in [
                        ("history-col-process", flex_w, false),
                        ("col-proto", D_PROTO_W, false),
                        ("col-remote", flex_w, false),
                        ("col-location", D_LOC_W, false),
                        ("history-col-first", D_SEEN_W, false),
                        ("history-col-duration", D_DUR_W, false),
                        ("col-down-total", D_BYTES_W, true),
                        ("col-up-total", D_BYTES_W, true),
                    ] {
                        widgets::table::header_cell_w(ui, w, &i18n.t(key), right);
                    }
                    ui.end_row();
                });
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                ui,
                22.0,
                rows.len(),
                |ui, row_range| {
                    let table_left = ui.max_rect().left();
                    let table_right = ui.max_rect().right();
                    egui::Grid::new("history_detail_rows")
                        .num_columns(8)
                        .striped(false)
                        .spacing([24.0, widgets::table::ROW_SPACING_Y])
                        .show(ui, |ui| {
                            for r_idx in row_range {
                                let r = &rows[r_idx];
                                let row_top = ui.cursor().top();
                                row_background(ui, table_left, table_right, row_top, r_idx);
                                widgets::table::fixed_cell(ui, flex_w, 22.0, |ui| {
                                    proc_cell(
                                        ui,
                                        &r.process,
                                        Some(r.pid),
                                        r.proc_path.as_deref(),
                                        icon_tex,
                                        default_icon_tex,
                                        i18n,
                                    );
                                });
                                widgets::table::fixed_cell(ui, D_PROTO_W, 22.0, |ui| {
                                    widgets::badge::badge(
                                        ui,
                                        r.proto.as_str(),
                                        if r.proto.as_str() == "TCP" {
                                            widgets::badge::BadgeKind::Accent
                                        } else {
                                            widgets::badge::BadgeKind::Neutral
                                        },
                                    );
                                });
                                widgets::table::fixed_cell(ui, flex_w, 22.0, |ui| {
                                    ui.add(
                                        Label::new(
                                            RichText::new(format!(
                                                "{}:{}",
                                                r.remote_ip, r.remote_port
                                            ))
                                            .size(theme::font::BODY)
                                            .color(theme::c().text),
                                        )
                                        .wrap_mode(egui::TextWrapMode::Truncate),
                                    );
                                });
                                widgets::table::fixed_cell(ui, D_LOC_W, 22.0, |ui| {
                                    location_cell(ui, i18n, r.remote_ip);
                                });
                                widgets::table::fixed_cell(ui, D_SEEN_W, 22.0, |ui| {
                                    ui.label(theme::dim_text(
                                        &history_query::fmt_local(r.first_seen),
                                        theme::font::BODY,
                                    ));
                                });
                                widgets::table::fixed_cell(ui, D_DUR_W, 22.0, |ui| {
                                    ui.label(theme::dim_text(
                                        &history_query::fmt_duration(
                                            r.last_seen.saturating_sub(r.first_seen),
                                        ),
                                        theme::font::BODY,
                                    ));
                                });
                                widgets::table::fixed_num_cell(ui, D_BYTES_W, |ui| {
                                    bytes_cell(ui, r.bytes_in, false);
                                });
                                widgets::table::fixed_num_cell(ui, D_BYTES_W, |ui| {
                                    bytes_cell(ui, r.bytes_out, true);
                                });
                                ui.end_row();
                            }
                        });
                },
            );
            truncated_hint(ui, rows.len(), i18n);
        }
        Rows::Aggregate(rows) => {
            if rows.is_empty() {
                empty_hint(ui, i18n);
                return;
            }
            const A_PROTO_W: f32 = 56.0;
            const A_LOC_W: f32 = 112.0;
            const A_CNT_W: f32 = 65.0;
            const A_DUR_W: f32 = 80.0;
            const A_LAST_W: f32 = 105.0;
            const A_BYTES_W: f32 = 90.0;
            let table_w = ui.available_width();
            let flex_total = (table_w
                - 24.0 * 8.0
                - (A_PROTO_W + A_LOC_W + A_CNT_W + A_DUR_W + A_LAST_W + A_BYTES_W * 2.0))
                .max(320.0);
            let flex_w = flex_total * 0.5;
            let aggregate_sort = &mut state.aggregate_sort;
            // 表头固定在滚动区外(虚拟化行定位不含表头):次数/时长/最近活动
            // 数据左对齐表头贴左,下载/上传总量数据右对齐表头贴右
            egui::Grid::new("history_aggregate_header")
                .num_columns(9)
                .spacing([24.0, 0.0])
                .show(ui, |ui| {
                    for (key, w) in [
                        ("history-col-process", flex_w),
                        ("col-proto", A_PROTO_W),
                        ("col-remote", flex_w),
                        ("col-location", A_LOC_W),
                    ] {
                        widgets::table::header_cell_w(ui, w, &i18n.t(key), false);
                    }
                    let mut sort_clicked = false;
                    let mut sort_header =
                        |ui: &mut egui::Ui, key: &str, sort: AggregateSort, right: bool, w: f32| {
                            let (cur, asc) = *aggregate_sort;
                            let r = widgets::table::header_sort_cell(
                                ui,
                                &i18n.t(key),
                                cur == sort,
                                asc,
                                right,
                                w,
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
                    for (key, sort, right, w) in [
                        ("history-col-count", AggregateSort::Count, false, A_CNT_W),
                        (
                            "history-col-total",
                            AggregateSort::TotalSecs,
                            false,
                            A_DUR_W,
                        ),
                        (
                            "history-col-last",
                            AggregateSort::LastActive,
                            false,
                            A_LAST_W,
                        ),
                        ("col-down-total", AggregateSort::BytesIn, true, A_BYTES_W),
                        ("col-up-total", AggregateSort::BytesOut, true, A_BYTES_W),
                    ] {
                        sort_clicked |= sort_header(ui, key, sort, right, w);
                    }
                    if sort_clicked {
                        state.dirty = true;
                    }
                    ui.end_row();
                });
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                ui,
                22.0,
                rows.len(),
                |ui, row_range| {
                    let table_left = ui.max_rect().left();
                    let table_right = ui.max_rect().right();
                    egui::Grid::new("history_aggregate_rows")
                        .num_columns(9)
                        .striped(false)
                        .spacing([24.0, widgets::table::ROW_SPACING_Y])
                        .show(ui, |ui| {
                            for r_idx in row_range {
                                let r = &rows[r_idx];
                                let row_top = ui.cursor().top();
                                row_background(ui, table_left, table_right, row_top, r_idx);
                                widgets::table::fixed_cell(ui, flex_w, 22.0, |ui| {
                                    proc_cell(
                                        ui,
                                        &r.process,
                                        None,
                                        None,
                                        icon_tex,
                                        default_icon_tex,
                                        i18n,
                                    );
                                });
                                widgets::table::fixed_cell(ui, A_PROTO_W, 22.0, |ui| {
                                    widgets::badge::badge(
                                        ui,
                                        r.proto.as_str(),
                                        if r.proto.as_str() == "TCP" {
                                            widgets::badge::BadgeKind::Accent
                                        } else {
                                            widgets::badge::BadgeKind::Neutral
                                        },
                                    );
                                });
                                widgets::table::fixed_cell(ui, flex_w, 22.0, |ui| {
                                    ui.add(
                                        Label::new(
                                            RichText::new(r.remote_ip.to_string())
                                                .size(theme::font::BODY)
                                                .color(theme::c().text),
                                        )
                                        .wrap_mode(egui::TextWrapMode::Truncate),
                                    );
                                });
                                widgets::table::fixed_cell(ui, A_LOC_W, 22.0, |ui| {
                                    location_cell(ui, i18n, r.remote_ip);
                                });
                                widgets::table::fixed_cell(ui, A_CNT_W, 22.0, |ui| {
                                    ui.label(
                                        RichText::new(r.count.to_string())
                                            .size(theme::font::BODY)
                                            .color(theme::c().text),
                                    );
                                });
                                widgets::table::fixed_cell(ui, A_DUR_W, 22.0, |ui| {
                                    ui.label(theme::dim_text(
                                        &history_query::fmt_duration(r.total_secs),
                                        theme::font::BODY,
                                    ));
                                });
                                widgets::table::fixed_cell(ui, A_LAST_W, 22.0, |ui| {
                                    ui.label(theme::dim_text(
                                        &history_query::fmt_local(r.last_active),
                                        theme::font::BODY,
                                    ));
                                });
                                widgets::table::fixed_num_cell(ui, A_BYTES_W, |ui| {
                                    bytes_cell(ui, r.bytes_in, false);
                                });
                                widgets::table::fixed_num_cell(ui, A_BYTES_W, |ui| {
                                    bytes_cell(ui, r.bytes_out, true);
                                });
                                ui.end_row();
                            }
                        });
                    truncated_hint(ui, rows.len(), i18n);
                },
            );
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
            const SUM_UP_W: f32 = 85.0;
            const SUM_DOWN_W: f32 = 85.0;
            const SUM_CNT_W: f32 = 65.0;
            const SUM_DUR_W: f32 = 85.0;
            let table_w = ui.available_width();
            let flex_w =
                (table_w - 24.0 * 4.0 - (SUM_UP_W + SUM_DOWN_W + SUM_CNT_W + SUM_DUR_W)).max(240.0);
            let summary_sort = &mut state.summary_sort;
            // 表头固定在滚动区外(虚拟化行定位不含表头);全部数字列数据右对齐,
            // 表头贴右
            egui::Grid::new("history_summary_header")
                .num_columns(5)
                .spacing([24.0, 0.0])
                .show(ui, |ui| {
                    widgets::table::header_cell_w(
                        ui,
                        flex_w,
                        &i18n.t("history-col-process"),
                        false,
                    );
                    let mut sort_clicked = false;
                    let mut sort_header =
                        |ui: &mut egui::Ui, key: &str, sort: SummarySort, w: f32| {
                            let (cur, asc) = *summary_sort;
                            let r = widgets::table::header_sort_cell(
                                ui,
                                &i18n.t(key),
                                cur == sort,
                                asc,
                                true,
                                w,
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
                    for (key, sort, w) in [
                        ("col-up-total", SummarySort::BytesOut, SUM_UP_W),
                        ("col-down-total", SummarySort::BytesIn, SUM_DOWN_W),
                        ("history-col-count", SummarySort::Count, SUM_CNT_W),
                        ("history-col-total", SummarySort::TotalSecs, SUM_DUR_W),
                    ] {
                        sort_clicked |= sort_header(ui, key, sort, w);
                    }
                    if sort_clicked {
                        state.dirty = true;
                    }
                    ui.end_row();
                });
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                ui,
                22.0,
                rows.len(),
                |ui, row_range| {
                    let table_left = ui.max_rect().left();
                    let table_right = ui.max_rect().right();
                    egui::Grid::new("history_summary_rows")
                        .num_columns(5)
                        .striped(false)
                        .spacing([24.0, widgets::table::ROW_SPACING_Y])
                        .show(ui, |ui| {
                            for r_idx in row_range {
                                let r = &rows[r_idx];
                                let row_top = ui.cursor().top();
                                row_background(ui, table_left, table_right, row_top, r_idx);
                                widgets::table::fixed_cell(ui, flex_w, 22.0, |ui| {
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
                                widgets::table::fixed_num_cell(ui, SUM_UP_W, |ui| {
                                    bytes_cell(ui, r.bytes_out, true);
                                });
                                widgets::table::fixed_num_cell(ui, SUM_DOWN_W, |ui| {
                                    bytes_cell(ui, r.bytes_in, false);
                                });
                                widgets::table::fixed_num_cell(ui, SUM_CNT_W, |ui| {
                                    widgets::table::num_cell(
                                        ui,
                                        r.count.to_string(),
                                        theme::c().text,
                                    );
                                });
                                widgets::table::fixed_num_cell(ui, SUM_DUR_W, |ui| {
                                    widgets::table::num_cell(
                                        ui,
                                        history_query::fmt_duration(r.total_secs),
                                        theme::c().text,
                                    );
                                });
                                ui.end_row();
                            }
                        });
                    truncated_hint(ui, rows.len(), i18n);
                },
            );
        }
    }
}

fn empty_hint(ui: &mut egui::Ui, i18n: &I18n) {
    ui.add_space(theme::sp::XL);
    ui.label(theme::dim_text(&i18n.t("history-empty"), theme::font::H3));
}

/// 行底色(虚拟化表格手动斑马):行首调用,悬停高亮优先于奇数行条纹;
/// 色块上下各含半个行距,与 Grid striped 的观感一致,判定区同样含行距
fn row_background(ui: &egui::Ui, left: f32, right: f32, top: f32, idx: usize) {
    let rect = egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, top + 22.0))
        .expand2(egui::vec2(0.0, widgets::table::ROW_SPACING_Y * 0.5));
    let bg = if ui.rect_contains_pointer(rect) {
        theme::c().hover_bg
    } else if idx % 2 == 1 {
        theme::c().faint
    } else {
        return;
    };
    ui.painter().rect_filled(rect, 0.0, bg);
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
            .wrap_mode(egui::TextWrapMode::Truncate),
        );
    });
}

/// 位置单元格:归属地实时反查(geoip 数据重建不影响历史行),未知占位
fn location_cell(ui: &mut egui::Ui, i18n: &I18n, ip: std::net::Ipv4Addr) {
    let text = match geoip::locate(ip).map(Place::Geo) {
        Some(p) => geoip::place_label(p, i18n),
        None => i18n.t("conn-loc-unknown"),
    };
    ui.add(
        Label::new(theme::dim_text(&text, theme::font::BODY))
            .wrap_mode(egui::TextWrapMode::Truncate),
    );
}
