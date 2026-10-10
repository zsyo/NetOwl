//! 历史页:明细/聚合/汇总/用量四视图、时间档位与进程/远端/协议筛选、
//! CSV 导出、数据库大小展示、超容提醒与手动清空。
//!
//! 四视图表格按 variant 分文件(detail/aggregate/summary/usage),共享
//! 单元格渲染在 cells,行右键菜单在 menu,删除确认弹窗在 confirm。

mod aggregate;
mod cells;
mod confirm;
mod detail;
mod export;
mod menu;
mod summary;
mod toolbar;
mod usage;

use std::collections::HashMap;

use eframe::egui;
use egui::{CornerRadius, Frame, Margin, RichText, Stroke};
use rusqlite::Connection as Db;

use crate::i18n::I18n;
use crate::model::{Connection, fmt_bytes};
use crate::storage::config::Config;
use crate::storage::history;
use crate::storage::history_query;
use crate::ui::{icons, theme, widgets};

/// 历史页;返回是否直接改动了配置(勾选不再提醒/清空还原提醒)
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    state: &mut history_query::PageState,
    i18n: &I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    conns: &[Connection],
    db: &Db,
    writer: &history::Writer,
    config: &mut Config,
    elevated: bool,
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

    toolbar::toolbar(
        ui,
        state,
        i18n,
        db,
        writer,
        config,
        &mut config_changed,
        conns,
    );
    ui.add_space(theme::sp::SM);

    rows_table(
        ui,
        state,
        i18n,
        icon_tex,
        default_icon_tex,
        conns,
        db,
        config,
        elevated,
    );
    confirm::confirm_delete_modal(ui, state, i18n, db);
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
                // 绑定局部变量而非 &mut false:后者令复选框恒显示未勾选,
                // 点击无任何状态反馈(卡片消失前的那一帧也不可信)
                let mut dismiss = false;
                if ui
                    .checkbox(&mut dismiss, i18n.t("history-remind-dismiss"))
                    .clicked()
                {
                    config.general.history_remind = false;
                    changed = true;
                }
            });
        });
    changed
}

/// 结果表分发:明细(8 列)/ 聚合(9 列)/ 汇总(5 列,表头可排序)/ 用量(柱图)。
/// 各视图表格虚拟化(show_rows 只布局可见行):查询上限 500 行,
/// 窗口缩放/滚动每帧全量重排会卡顿。行高恒定 22,行色手动垫底
/// (虚拟化后 Grid striped 的奇偶不再对应全局行号);
/// 表头固定在滚动区之外(show_rows 的行定位不含表头,混入会整体错位),
/// 列宽与数据 Grid 同一公式计算保持对齐
#[allow(clippy::too_many_arguments)]
fn rows_table(
    ui: &mut egui::Ui,
    state: &mut history_query::PageState,
    i18n: &I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    conns: &[Connection],
    db: &Db,
    config: &Config,
    elevated: bool,
) {
    match &state.rows {
        history_query::Rows::Detail(_) => {
            detail::table(ui, state, i18n, icon_tex, default_icon_tex, db);
        }
        history_query::Rows::Aggregate(_) => {
            aggregate::table(ui, state, i18n, icon_tex, default_icon_tex);
        }
        history_query::Rows::Summary(_) => {
            summary::table(ui, state, i18n, icon_tex, default_icon_tex, conns, config);
        }
        history_query::Rows::Usage(_) => {
            usage::table(ui, state, i18n, elevated);
        }
    }
}
