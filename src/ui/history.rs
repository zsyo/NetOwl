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
use crate::storage::history_query::{self, Rows, ViewMode};
use crate::ui::theme;

/// 历史页;返回是否直接改动了配置(勾选不再提醒/清空还原提醒)
pub fn show(
    ui: &mut egui::Ui,
    state: &mut history_query::PageState,
    i18n: &I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    db: &Db,
    writer: &history::Writer,
    config: &mut Config,
) -> bool {
    state.refresh_if_needed(db, config.general.hide_local, config.general.hide_lan);
    let mut config_changed = false;

    ui.heading(theme::accent_text(&i18n.t("history-title"), 20.0));
    ui.label(theme::dim_text(&i18n.t("history-subtitle"), 13.0));
    ui.add_space(10.0);

    // 超容提醒:大小超阈值且用户未勾选"不再提醒"时展示
    if state.db_size > history_query::REMIND_SIZE && config.general.history_remind {
        config_changed |= remind_card(ui, state, i18n, config);
        ui.add_space(8.0);
    }

    toolbar(ui, state, i18n, db, writer, config, &mut config_changed);
    ui.add_space(8.0);

    rows_table(ui, state, i18n, icon_tex);
    config_changed
}

/// 超容提醒卡片;勾选"不再提醒"写 config 持久化
fn remind_card(
    ui: &mut egui::Ui,
    state: &history_query::PageState,
    i18n: &I18n,
    config: &mut Config,
) -> bool {
    let mut changed = false;
    Frame::new()
        .fill(theme::c().bg_card)
        .stroke(Stroke::new(1.0, theme::c().accent.gamma_multiply(0.5)))
        .corner_radius(CornerRadius::same(theme::RADIUS_MD))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let text =
                    i18n.t_with_args("history-remind-text", &[("size", fmt_bytes(state.db_size))]);
                ui.label(RichText::new(text).size(13.0).color(theme::c().text));
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

/// 工具栏行内控件的最小交互高度:egui horizontal 的行高从默认 18px 起步,
/// 高于行高的控件被强制顶部对齐而矮控件行内居中,造成文字基线错位;统一
/// 抬高该值,让整行控件在同一行高内垂直居中(取行内最高控件并留余量)
const TOOLBAR_ROW_H: f32 = 26.0;

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
        // 视图切换
        let mut switch = None;
        let view_label = |i18n: &I18n, mode: ViewMode| match mode {
            ViewMode::Detail => i18n.t("history-view-detail"),
            ViewMode::Aggregate => i18n.t("history-view-aggregate"),
        };
        for mode in [ViewMode::Detail, ViewMode::Aggregate] {
            let selected = state.view == mode;
            let label = RichText::new(view_label(i18n, mode))
                .size(13.0)
                .color(if selected {
                    theme::c().accent
                } else {
                    theme::c().text
                });
            if ui.selectable_label(selected, label).clicked() {
                switch = Some(mode);
            }
        }
        if let Some(mode) = switch {
            state.view = mode;
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
            .selected_text(RichText::new(range_name(i18n, state.range)).size(13.0))
            .show_ui(ui, |ui| {
                for (r, key) in ranges {
                    if ui
                        .selectable_label(state.range == r, RichText::new(i18n.t(key)).size(13.0))
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
                .font(egui::FontId::proportional(13.0))
                .desired_width(110.0),
        );
        let remote = ui.add(
            egui::TextEdit::singleline(&mut state.remote)
                .hint_text(i18n.t("history-filter-remote"))
                .font(egui::FontId::proportional(13.0))
                .desired_width(110.0),
        );

        // 协议
        egui::ComboBox::from_id_salt("history-proto")
            .width(90.0)
            .selected_text(RichText::new(proto_name(i18n, state.proto)).size(13.0))
            .show_ui(ui, |ui| {
                for p in [None, Some(Protocol::Tcp), Some(Protocol::Udp)] {
                    if ui
                        .selectable_label(
                            state.proto == p,
                            RichText::new(proto_name(i18n, p)).size(13.0),
                        )
                        .clicked()
                    {
                        state.proto = p;
                        state.dirty = true;
                    }
                }
            });

        if ui
            .button(RichText::new(i18n.t("history-refresh")).size(13.0))
            .clicked()
        {
            state.dirty = true;
        }
        if process.changed() || remote.changed() || process.lost_focus() || remote.lost_focus() {
            state.dirty = true;
        }

        // 本地/局域网远端噪音过滤(config 持久化,连接页与历史页共享)
        let hide_local_text = RichText::new(i18n.t("filter-hide-local")).size(13.0);
        if ui
            .checkbox(&mut config.general.hide_local, hide_local_text)
            .changed()
        {
            state.dirty = true;
            *config_changed = true;
        }
        let hide_lan_text = RichText::new(i18n.t("filter-hide-lan")).size(13.0);
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
            ui.label(theme::dim_text(&size, 12.0));
            let purge_button = egui::Button::new(RichText::new(i18n.t("history-purge")).size(13.0))
                .fill(theme::c().accent_soft)
                .corner_radius(CornerRadius::same(theme::RADIUS_SM));
            MenuButton::from_button(purge_button).ui(ui, |ui| {
                ui.with_layout(
                    egui::Layout::top_down(egui::Align::LEFT).with_cross_justify(true),
                    |ui| {
                        for (days, text) in purge_choices(i18n) {
                            if ui
                                .selectable_label(false, RichText::new(text).size(13.0))
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

/// 结果表:明细(6 列)或聚合(7 列)
fn rows_table(
    ui: &mut egui::Ui,
    state: &history_query::PageState,
    i18n: &I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
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
                    egui::Grid::new("history_detail")
                        .num_columns(6)
                        .spacing([24.0, 7.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for key in [
                                "history-col-process",
                                "col-proto",
                                "col-remote",
                                "col-location",
                                "history-col-first",
                                "history-col-duration",
                            ] {
                                header(ui, i18n.t(key));
                            }
                            ui.end_row();
                            for r in rows {
                                proc_cell(
                                    ui,
                                    &r.process,
                                    Some(r.pid),
                                    r.proc_path.as_deref(),
                                    icon_tex,
                                    i18n,
                                );
                                ui.label(theme::dim_text(r.proto.as_str(), 13.0));
                                ui.add(
                                    Label::new(
                                        RichText::new(format!("{}:{}", r.remote_ip, r.remote_port))
                                            .size(13.0)
                                            .color(theme::c().text),
                                    )
                                    .wrap_mode(egui::TextWrapMode::Extend),
                                );
                                location_cell(ui, i18n, r.remote_ip);
                                ui.label(theme::dim_text(
                                    &history_query::fmt_local(r.first_seen),
                                    13.0,
                                ));
                                ui.label(theme::dim_text(
                                    &history_query::fmt_duration(
                                        r.last_seen.saturating_sub(r.first_seen),
                                    ),
                                    13.0,
                                ));
                                ui.end_row();
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
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    egui::Grid::new("history_aggregate")
                        .num_columns(7)
                        .spacing([24.0, 7.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for key in [
                                "history-col-process",
                                "col-proto",
                                "col-remote",
                                "col-location",
                                "history-col-count",
                                "history-col-total",
                                "history-col-last",
                            ] {
                                header(ui, i18n.t(key));
                            }
                            ui.end_row();
                            for r in rows {
                                proc_cell(ui, &r.process, None, None, icon_tex, i18n);
                                ui.label(theme::dim_text(r.proto.as_str(), 13.0));
                                ui.add(
                                    Label::new(
                                        RichText::new(r.remote_ip.to_string())
                                            .size(13.0)
                                            .color(theme::c().text),
                                    )
                                    .wrap_mode(egui::TextWrapMode::Extend),
                                );
                                location_cell(ui, i18n, r.remote_ip);
                                ui.label(
                                    RichText::new(r.count.to_string())
                                        .size(13.0)
                                        .color(theme::c().text),
                                );
                                ui.label(theme::dim_text(
                                    &history_query::fmt_duration(r.total_secs),
                                    13.0,
                                ));
                                ui.label(theme::dim_text(
                                    &history_query::fmt_local(r.last_active),
                                    13.0,
                                ));
                                ui.end_row();
                            }
                        });
                    truncated_hint(ui, rows.len(), i18n);
                });
        }
    }
}

fn header(ui: &mut egui::Ui, text: String) {
    ui.label(
        RichText::new(text)
            .size(12.0)
            .strong()
            .color(theme::c().text_dim),
    );
}

fn empty_hint(ui: &mut egui::Ui, i18n: &I18n) {
    ui.add_space(20.0);
    ui.label(theme::dim_text(&i18n.t("history-empty"), 14.0));
}

fn truncated_hint(ui: &mut egui::Ui, len: usize, i18n: &I18n) {
    if len >= history_query::QUERY_LIMIT {
        ui.add_space(8.0);
        ui.label(theme::dim_text(
            &i18n.t_with_args(
                "history-truncated",
                &[("n", history_query::QUERY_LIMIT.to_string())],
            ),
            12.0,
        ));
    }
}

/// 进程单元格:图标 + 名称(未知进程占位),聚合行无 PID
fn proc_cell(
    ui: &mut egui::Ui,
    name: &str,
    pid: Option<u32>,
    path: Option<&str>,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    i18n: &I18n,
) {
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
        let text = match (name.is_empty(), pid) {
            (true, Some(pid)) => {
                format!("{} (PID {pid})", i18n.t("conn-proc-unknown"))
            }
            (true, None) => i18n.t("conn-proc-unknown"),
            (false, Some(pid)) => format!("{name} ({pid})"),
            (false, None) => name.to_owned(),
        };
        ui.add(
            Label::new(RichText::new(text).size(13.0).color(theme::c().text))
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
    ui.label(theme::dim_text(&text, 13.0));
}
