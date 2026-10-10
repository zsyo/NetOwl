//! 规则表单行渲染:启停开关、名称(临时规则弱化标注)、动作/方向/协议
//! 徽章、进程/远端/端口文本列、命中数列与操作列(上移/下移/编辑/删除)。
//! 表格编排(表头/滚动/Grid)在 table。

use eframe::egui;
use egui::RichText;
use rusqlite::Connection as Db;

use super::PageState;
use super::draft::Draft;
use super::labels;
use super::table::{
    COL_ACTION, COL_DIRECTION, COL_ENABLED, COL_HITS, COL_OPS, COL_PORT, COL_PROTO,
};
use crate::i18n::I18n;
use crate::model::CountUnits;
use crate::rules::{Action, Rule, RuleSet};
use crate::ui::{icons, theme, widgets};

/// 行内操作结果:删除/移动后调用方结束本帧表格(防同帧旧索引渲染)
#[derive(Default)]
pub(super) struct RowOps {
    pub(super) removed: bool,
    pub(super) moved: bool,
}

/// 规则表单行(10 列)。`rule` 为调用方克隆的所有权副本——行内要改
/// `rules`(启停/移动/删除),不能同时持有其规则借用
#[allow(clippy::too_many_arguments)]
pub(super) fn rule_row(
    ui: &mut egui::Ui,
    state: &mut PageState,
    i18n: &I18n,
    db: &Db,
    rules: &mut RuleSet,
    row_hover: &mut widgets::table::RowHover,
    rule: Rule,
    i: usize,
    flex_w: f32,
    table_left: f32,
    table_right: f32,
    count_units: CountUnits,
) -> RowOps {
    let mut ops = RowOps::default();
    let row_top = ui.cursor().top();
    row_hover.begin(ui, table_left, table_right, row_top, i);
    let mut enabled = rule.enabled;
    widgets::table::fixed_center_cell(ui, COL_ENABLED, 26.0, |ui| {
        let toggle = widgets::toggle::toggle_switch(
            ui,
            &mut enabled,
            egui::Id::new(("rule-enabled-toggle", rule.id)),
        );
        if toggle.changed()
            && let Err(e) = rules.set_enabled(db, rule.id, enabled)
        {
            tracing::warn!("[Rules] 切换规则 {}({}) 启停失败: {e}", rule.name, rule.id);
        }
    });
    // 会话临时规则(负 id):名称加标注并以弱化色显示
    let is_temp = rule.id < 0;
    let name_text = if is_temp {
        format!("{} ({})", rule.name, i18n.t("rules-temp-badge"))
    } else {
        rule.name.clone()
    };
    widgets::table::fixed_cell(ui, flex_w, 18.0, |ui| {
        ui.add_sized(
            [flex_w - 2.0 * widgets::table::CELL_PAD_X, 18.0],
            egui::Label::new(
                RichText::new(name_text)
                    .size(theme::font::BODY)
                    .color(if is_temp {
                        theme::c().text_dim
                    } else {
                        theme::c().text
                    }),
            )
            .wrap_mode(egui::TextWrapMode::Truncate),
        );
    });
    let (action_key, action_kind) = match rule.action {
        Action::Allow => ("rule-action-allow", widgets::badge::BadgeKind::Ok),
        Action::Block => ("rule-action-block", widgets::badge::BadgeKind::Danger),
    };
    widgets::badge::badge_centered(ui, COL_ACTION, 26.0, &i18n.t(action_key), action_kind);
    widgets::table::fixed_cell(ui, COL_DIRECTION, 18.0, |ui| {
        ui.add_sized(
            [COL_DIRECTION - 2.0 * widgets::table::CELL_PAD_X, 18.0],
            egui::Label::new(theme::dim_text(
                &labels::direction_name(i18n, rule.direction),
                theme::font::BODY,
            )),
        );
    });
    let proto_text = labels::proto_name(i18n, rule.proto);
    widgets::badge::badge_centered(
        ui,
        COL_PROTO,
        26.0,
        &proto_text,
        widgets::badge::BadgeKind::Neutral,
    );
    widgets::table::fixed_cell(ui, flex_w, 18.0, |ui| {
        ui.add_sized(
            [flex_w - 2.0 * widgets::table::CELL_PAD_X, 18.0],
            egui::Label::new(theme::dim_text(
                &labels::process_display(&rule),
                theme::font::BODY,
            ))
            .wrap_mode(egui::TextWrapMode::Truncate),
        );
    });
    widgets::table::fixed_cell(ui, flex_w, 18.0, |ui| {
        ui.add_sized(
            [flex_w - 2.0 * widgets::table::CELL_PAD_X, 18.0],
            egui::Label::new(theme::dim_text(
                &labels::remote_display(&rule),
                theme::font::BODY,
            ))
            .wrap_mode(egui::TextWrapMode::Truncate),
        );
    });
    widgets::table::fixed_cell(ui, COL_PORT, 18.0, |ui| {
        ui.add_sized(
            [COL_PORT - 2.0 * widgets::table::CELL_PAD_X, 18.0],
            egui::Label::new(theme::dim_text(
                &labels::port_display(rule.port),
                theme::font::BODY,
            )),
        );
    });
    // 命中数列:累计命中连接数(每连接计一次,落库跨重启保留;0 弱化灰)。
    // 临时规则与兜底不计,poll 层 1s 口径;最小单位以上紧凑显示,
    // 精确值悬停展示
    let hits = rule.hit_count;
    widgets::table::fixed_num_cell(ui, COL_HITS, |ui| {
        let cell = widgets::table::num_cell(
            ui,
            crate::model::fmt_count(hits, count_units),
            if hits == 0 {
                theme::c().text_dim
            } else {
                theme::c().text
            },
        );
        // 紧凑显示时悬停给精确值(最小单位以下本就是精确数)
        let base = if count_units == CountUnits::Chinese {
            10_000
        } else {
            1_000
        };
        if hits >= base {
            cell.on_hover_text(hits.to_string());
        }
    });
    widgets::table::fixed_cell(ui, COL_OPS, 26.0, |ui| {
        ui.style_mut().spacing.item_spacing.x = 2.0;
        // 首行禁上移、末行禁下移(边界置灰,不可点);会话临时规则
        // (负 id)不参与排序,移动一律禁用
        let last = i + 1 == rules.rules.len();
        let movable = rule.id > 0;
        if widgets::button::icon_btn(
            ui,
            icons::ARROW_UP,
            Some(i18n.t("rules-move-up")),
            false,
            i > 0 && movable,
        )
        .clicked()
        {
            if let Err(e) = rules.move_rule(db, rule.id, -1) {
                tracing::warn!("[Rules] 上移规则 {} 失败: {e}", rule.id);
            } else {
                ops.moved = true;
            }
        }
        if widgets::button::icon_btn(
            ui,
            icons::ARROW_DOWN,
            Some(i18n.t("rules-move-down")),
            false,
            !last && movable,
        )
        .clicked()
        {
            if let Err(e) = rules.move_rule(db, rule.id, 1) {
                tracing::warn!("[Rules] 下移规则 {} 失败: {e}", rule.id);
            } else {
                ops.moved = true;
            }
        }
        if widgets::button::icon_btn(ui, icons::PENCIL, Some(i18n.t("rules-edit")), false, true)
            .clicked()
        {
            state.draft = Some(Draft::from_rule(&rule));
        }
        if widgets::button::icon_btn(ui, icons::TRASH, Some(i18n.t("rules-delete")), true, true)
            .clicked()
        {
            if let Err(e) = rules.delete(db, rule.id) {
                tracing::warn!("[Rules] 删除规则 {} 失败: {e}", rule.id);
            }
            ops.removed = true;
        }
    });
    ui.end_row();
    row_hover.end(ui, row_top);
    ops
}
