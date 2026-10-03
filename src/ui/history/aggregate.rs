//! 历史页聚合视图:按 (进程, 协议, 远端) 分组的 9 列汇总表,
//! 次数/时长/最近活动/收发字节表头可排序,行右键删除该组。

use std::collections::HashMap;

use eframe::egui;
use egui::{Label, RichText};

use super::cells;
use crate::i18n::I18n;
use crate::storage::history_query::{self, AggregateSort, Rows};
use crate::ui::{theme, widgets};

pub(super) fn table(
    ui: &mut egui::Ui,
    state: &mut history_query::PageState,
    i18n: &I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
) {
    let Rows::Aggregate(rows) = &state.rows else {
        return;
    };
    if rows.is_empty() {
        cells::empty_hint(ui, i18n);
        return;
    }
    const A_PROTO_W: f32 = 64.0;
    const A_LOC_W: f32 = 112.0;
    const A_CNT_W: f32 = 65.0;
    const A_DUR_W: f32 = 80.0;
    const A_LAST_W: f32 = 112.0;
    const A_BYTES_W: f32 = 90.0;
    let table_w = ui.available_width();
    let flex_total = (table_w
        - (A_PROTO_W + A_LOC_W + A_CNT_W + A_DUR_W + A_LAST_W + A_BYTES_W * 2.0))
        .max(320.0);
    let flex_w = flex_total * 0.5;
    let aggregate_sort = &mut state.aggregate_sort;
    // 表头固定在滚动区外(虚拟化行定位不含表头):次数/时长/最近活动
    // 数据左对齐表头贴左,下载/上传总量数据右对齐表头贴右
    egui::Grid::new("history_aggregate_header")
        .num_columns(9)
        .spacing([0.0, 0.0])
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
            let mut sort_header = |ui: &mut egui::Ui,
                                   key: &str,
                                   sort: AggregateSort,
                                   right: bool,
                                   w: f32| {
                let (cur, asc) = *aggregate_sort;
                let r =
                    widgets::table::header_sort_cell(ui, &i18n.t(key), cur == sort, asc, right, w);
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
                .spacing([0.0, widgets::table::ROW_SPACING_Y])
                .show(ui, |ui| {
                    for r_idx in row_range {
                        let r = &rows[r_idx];
                        let row_top = ui.cursor().top();
                        let row_rect =
                            cells::row_background(ui, table_left, table_right, row_top, r_idx);
                        let row_resp = ui.interact(
                            row_rect,
                            egui::Id::new(("history_aggregate_row", r_idx)),
                            egui::Sense::click(),
                        );
                        row_resp.context_menu(|ui| {
                            if widgets::menu::menu_item(
                                ui,
                                i18n.t("history-menu-delete-group"),
                                true,
                            )
                            .clicked()
                            {
                                state.pending_delete = Some(history_query::PendingDelete {
                                    process: r.process.clone(),
                                    proto: Some(r.proto),
                                    remote_ip: Some(r.remote_ip),
                                });
                            }
                            if widgets::menu::menu_item(ui, i18n.t("menu-copy-remote"), true)
                                .clicked()
                            {
                                ui.ctx().copy_text(r.remote_ip.to_string());
                            }
                        });
                        widgets::table::fixed_cell(ui, flex_w, 22.0, |ui| {
                            cells::proc_cell(
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
                            cells::location_cell(ui, i18n, r.remote_ip);
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
                            cells::bytes_cell(ui, r.bytes_in, false);
                        });
                        widgets::table::fixed_num_cell(ui, A_BYTES_W, |ui| {
                            cells::bytes_cell(ui, r.bytes_out, true);
                        });
                        ui.end_row();
                    }
                });
        },
    );
    cells::truncated_hint(ui, rows.len(), i18n);
}
