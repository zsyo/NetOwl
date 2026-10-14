//! 历史页工具栏:四视图切换、时间档位与进程/远端/协议筛选(防抖)、
//! 噪音过滤下拉(隐藏本地/局域网)、刷新与 CSV 导出按钮、库大小与
//! 手动清理菜单(库大小置于菜单首行)。

use eframe::egui;
use egui::{
    CornerRadius, RichText,
    containers::PopupCloseBehavior,
    containers::menu::{MenuButton, MenuConfig},
};
use rusqlite::Connection as Db;

use super::export;
use crate::i18n::I18n;
use crate::model::{Connection, Protocol, fmt_bytes};
use crate::storage::config::Config;
use crate::storage::history;
use crate::storage::history_query::{self, ViewMode};
use crate::ui::{TOOLBAR_ROW_H, icons, theme, widgets};

/// 工具栏:视图切换、筛选与刷新(左),库大小与清空(右)。
/// conns 供汇总视图导出时做活跃合并(与页面同口径)
#[allow(clippy::too_many_arguments)]
pub(super) fn toolbar(
    ui: &mut egui::Ui,
    state: &mut history_query::PageState,
    i18n: &I18n,
    db: &Db,
    writer: &history::Writer,
    config: &mut Config,
    config_changed: &mut bool,
    conns: &[Connection],
) {
    ui.horizontal(|ui| {
        // 行高抬升只作用于本工具栏行(style_mut 泄漏到整页会把表格
        // Grid 的最小行高一并抬到 26,行内容与色带错位)
        ui.style_mut().spacing.interact_size.y = TOOLBAR_ROW_H;
        // 视图切换(segmented:明细/聚合/汇总/用量)
        let view_items = [
            (&*i18n.t("history-view-detail"), icons::VIEW_LIST),
            (&*i18n.t("history-view-aggregate"), icons::VIEW_STACKED),
            (&*i18n.t("history-view-summary"), icons::BAR_CHART_LINE),
            (&*i18n.t("history-view-usage"), icons::GRAPH_UP),
        ];
        let view_idx = match state.view {
            ViewMode::Detail => 0,
            ViewMode::Aggregate => 1,
            ViewMode::Summary => 2,
            ViewMode::Usage => 3,
        };
        if let Some(i) = widgets::segmented::segmented(ui, &view_items, view_idx) {
            state.view = match i {
                0 => ViewMode::Detail,
                1 => ViewMode::Aggregate,
                2 => ViewMode::Summary,
                _ => ViewMode::Usage,
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
        let process = widgets::search_box::search_box(
            ui,
            &mut state.process,
            i18n.t("history-filter-process"),
            100.0,
        );
        let remote = widgets::search_box::search_box(
            ui,
            &mut state.remote,
            i18n.t("history-filter-remote"),
            100.0,
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

        // 噪音过滤下拉(config 持久化,连接页与历史页共享):英文文案
        // 长,行内平铺两个 checkbox 会把右组库大小+清理挤出叠印(最小
        // 窗口 1440 下英文总宽约 1350 > 可用 1198),收进漏斗下拉;
        // 勾选项点击后菜单保持(CloseOnClickOutside),可连续勾两项
        let noise_on = config.general.hide_local || config.general.hide_lan;
        let mut filter_button = egui::Button::new(
            RichText::new(format!(
                "{}  {}  {}",
                icons::FUNNEL,
                i18n.t("history-filter"),
                icons::CARET_DOWN_FILL
            ))
            .size(theme::font::BODY),
        )
        .corner_radius(CornerRadius::same(theme::RADIUS_SM));
        if noise_on {
            filter_button = filter_button.fill(theme::c().accent_soft);
        }
        MenuButton::from_button(filter_button)
            .config(MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside))
            .ui(ui, |ui| {
                ui.with_layout(
                    egui::Layout::top_down(egui::Align::LEFT).with_cross_justify(true),
                    |ui| {
                        if ui
                            .selectable_label(
                                config.general.hide_local,
                                RichText::new(i18n.t("filter-hide-local")).size(theme::font::BODY),
                            )
                            .clicked()
                        {
                            config.general.hide_local = !config.general.hide_local;
                            state.dirty = true;
                            *config_changed = true;
                        }
                        if ui
                            .selectable_label(
                                config.general.hide_lan,
                                RichText::new(i18n.t("filter-hide-lan")).size(theme::font::BODY),
                            )
                            .clicked()
                        {
                            config.general.hide_lan = !config.general.hide_lan;
                            state.dirty = true;
                            *config_changed = true;
                        }
                    },
                );
            });

        if ui
            .button(RichText::new(i18n.t("history-refresh")).size(theme::font::BODY))
            .clicked()
        {
            state.dirty = true;
        }
        if ui
            .button(RichText::new(i18n.t("history-export")).size(theme::font::BODY))
            .clicked()
        {
            export::export_csv(state, i18n, conns, config);
        }
        // 输入中防抖重查(合并连续按键);失焦视为输入结束立即刷新
        if process.changed() || remote.changed() {
            state.defer_refresh();
        }
        if process.lost_focus() || remote.lost_focus() {
            state.dirty = true;
        }

        // 手动清理(右对齐):点击弹出档位菜单;库大小置于菜单首行——
        // 行内平铺该标签在英文下会把右组挤出叠印,而它是清理决策的
        // 上下文信息,随菜单展示更合适
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let purge_button =
                egui::Button::new(RichText::new(i18n.t("history-purge")).size(theme::font::BODY))
                    .fill(theme::c().accent_soft)
                    .corner_radius(CornerRadius::same(theme::RADIUS_SM));
            MenuButton::from_button(purge_button).ui(ui, |ui| {
                ui.with_layout(
                    egui::Layout::top_down(egui::Align::LEFT).with_cross_justify(true),
                    |ui| {
                        let size = i18n
                            .t_with_args("history-db-size", &[("size", fmt_bytes(state.db_size))]);
                        ui.label(theme::dim_text(&size, theme::font::SM));
                        ui.separator();
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
