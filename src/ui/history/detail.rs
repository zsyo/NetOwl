//! 历史页明细视图:8 列逐条连接记录(进程/协议/远端/归属/首末时间/
//! 时长/收发字节),行右键支持删除该条、定位与复制。

use std::collections::HashMap;

use eframe::egui;
use egui::{Label, RichText};
use rusqlite::Connection as Db;

use super::{cells, menu};
use crate::i18n::I18n;
use crate::storage::history_query::{self, Rows};
use crate::ui::{theme, widgets};

pub(super) fn table(
    ui: &mut egui::Ui,
    state: &mut history_query::PageState,
    i18n: &I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    db: &Db,
) {
    let Rows::Detail(rows) = &state.rows else {
        return;
    };
    if rows.is_empty() {
        cells::empty_hint(ui, i18n);
        return;
    }
    const D_PROTO_W: f32 = 64.0;
    const D_LOC_W: f32 = 112.0;
    const D_SEEN_W: f32 = 112.0;
    const D_DUR_W: f32 = 80.0;
    const D_BYTES_W: f32 = 90.0;
    // 表格总宽在虚拟化容器之外取(Grid 闭包内 available_width 被
    // grid 布局器接管,返回当前列宽而非总宽)
    let table_w = ui.available_width();
    let flex_total =
        (table_w - (D_PROTO_W + D_LOC_W + D_SEEN_W + D_DUR_W + D_BYTES_W * 2.0)).max(320.0);
    let flex_w = flex_total * 0.5;
    widgets::table::header_grid(
        ui,
        "history_detail_header",
        &[
            ("history-col-process", flex_w, false),
            ("col-proto", D_PROTO_W, false),
            ("col-remote", flex_w, false),
            ("col-location", D_LOC_W, false),
            ("history-col-first", D_SEEN_W, false),
            ("history-col-duration", D_DUR_W, false),
            ("col-down-total", D_BYTES_W, true),
            ("col-up-total", D_BYTES_W, true),
        ],
        &|k| i18n.t(k),
    );
    // 行距对齐:show_rows 按全局 item_spacing.y 计算行步进与内容总高,
    // 而数据 Grid 的实际行距是 ROW_SPACING_Y——不一致会让底部行画到
    // 声明 rect 之外,滚动范围被反测撑大,拖住滚动条时 offset 在两套
    // 高度间反复钳制(页面抖动)。对齐后声明高 = 实际高
    ui.spacing_mut().item_spacing.y = widgets::table::ROW_SPACING_Y;
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
                .spacing([0.0, widgets::table::ROW_SPACING_Y])
                .show(ui, |ui| {
                    for r_idx in row_range {
                        let r = &rows[r_idx];
                        let row_top = ui.cursor().top();
                        let row_rect =
                            cells::row_background(ui, table_left, table_right, row_top, r_idx);
                        let row_resp = ui.interact(
                            row_rect,
                            egui::Id::new(("history_detail_row", r_idx)),
                            egui::Sense::click(),
                        );
                        row_resp.context_menu(|ui| {
                            if menu::detail_menu(ui, r, i18n) {
                                match history_query::delete_detail(db, r) {
                                    Ok(n) => {
                                        state.dirty = true;
                                        tracing::info!("[History] 已删除 {n} 条明细记录");
                                    }
                                    Err(e) => {
                                        tracing::warn!("[History] 删除明细记录失败: {e}")
                                    }
                                }
                            }
                        });
                        widgets::table::fixed_cell(ui, flex_w, 22.0, |ui| {
                            cells::proc_cell(
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
                                    RichText::new(format!("{}:{}", r.remote_ip, r.remote_port))
                                        .size(theme::font::BODY)
                                        .color(theme::c().text),
                                )
                                .wrap_mode(egui::TextWrapMode::Truncate),
                            );
                        });
                        widgets::table::fixed_cell(ui, D_LOC_W, 22.0, |ui| {
                            cells::location_cell(ui, i18n, r.remote_ip);
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
                            cells::bytes_cell(ui, r.bytes_in, false);
                        });
                        widgets::table::fixed_num_cell(ui, D_BYTES_W, |ui| {
                            cells::bytes_cell(ui, r.bytes_out, true);
                        });
                        ui.end_row();
                    }
                });
        },
    );
    cells::truncated_hint(ui, rows.len(), i18n);
}
