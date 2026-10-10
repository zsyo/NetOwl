//! 规则列表表格:启停开关/上移下移/编辑/删除操作列,徽章列手动居中,
//! 删除后立即结束本帧表格防索引越界。

use eframe::egui;
use egui::{Label, RichText};
use rusqlite::Connection as Db;

use super::PageState;
use super::draft::Draft;
use super::labels;
use crate::i18n::I18n;
use crate::rules::{Action, RuleSet};
use crate::ui::{icons, theme, widgets};

/// 表格列宽(逻辑点);表头与数据列同宽,内容居中。
/// 列贴列布局(Grid spacing.x = 0),内容与列缘间距由单元格内边距
/// CELL_PAD_X 提供,定宽列 = 内容宽 + 2×CELL_PAD_X;
/// 名称/进程/远端三列弹性均分剩余宽:窗口放大时表格铺满中央区
const COL_ENABLED: f32 = 64.0;
const COL_ACTION: f32 = 64.0;
const COL_DIRECTION: f32 = 64.0;
const COL_PROTO: f32 = 64.0;
const COL_PORT: f32 = 64.0;
/// 命中数列:会话内命中连接数(poll 层 1s 口径累计,非渲染帧)
const COL_HITS: f32 = 64.0;
const COL_OPS: f32 = 128.0;

fn header_cell(ui: &mut egui::Ui, w: f32, text: String) {
    // 列贴列布局:占位整列宽,内容区(w - 2×CELL_PAD_X)内居中,与数据格
    // (fixed_cell 内容同缩进)同心;Grid 格内不能 add_space(egui 断言),
    // 用定宽占位 + 缩进子区域承载
    let pad = widgets::table::CELL_PAD_X;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 16.0), egui::Sense::hover());
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(pad, 0.0))));
    // add_sized(居中布局)实测表头稳定居中于列;徽章列的数据格用
    // 手动 add_space 居中(见下),两者同心
    child.add_sized(
        [w - 2.0 * pad, 16.0],
        Label::new(
            RichText::new(text)
                .size(theme::font::SM)
                .strong()
                .color(theme::c().text_dim),
        ),
    );
}

/// 规则表(自上而下即优先级从高到低)
pub(super) fn rules_table(
    ui: &mut egui::Ui,
    state: &mut PageState,
    i18n: &I18n,
    db: &Db,
    rules: &mut RuleSet,
    row_hover: &mut widgets::table::RowHover,
) {
    if rules.rules.is_empty() {
        ui.add_space(theme::sp::XL);
        ui.label(theme::dim_text(&i18n.t("rules-empty"), theme::font::H3));
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let table_left = ui.max_rect().left();
            let table_right = ui.max_rect().right();
            // 表格总宽必须在 Grid 之外取:Grid 闭包内 available_width 被
            // grid 布局器接管,返回当前列宽(上帧值)而非总宽
            let table_w = ui.available_width();
            let flex_w = ((table_w
                - (COL_ENABLED
                    + COL_ACTION
                    + COL_DIRECTION
                    + COL_PROTO
                    + COL_PORT
                    + COL_HITS
                    + COL_OPS))
                / 3.0)
                .max(220.0);
            egui::Grid::new("rules_grid")
                .num_columns(10)
                .striped(true)
                .spacing([0.0, widgets::table::ROW_SPACING_Y])
                .show(ui, |ui| {
                    header_cell(ui, COL_ENABLED, i18n.t("rules-col-enabled"));
                    header_cell(ui, flex_w, i18n.t("rules-col-name"));
                    header_cell(ui, COL_ACTION, i18n.t("rules-col-action"));
                    header_cell(ui, COL_DIRECTION, i18n.t("rules-col-direction"));
                    header_cell(ui, COL_PROTO, i18n.t("col-proto"));
                    header_cell(ui, flex_w, i18n.t("col-process"));
                    header_cell(ui, flex_w, i18n.t("rules-col-remote"));
                    header_cell(ui, COL_PORT, i18n.t("col-port"));
                    header_cell(ui, COL_HITS, i18n.t("rules-col-hits"));
                    header_cell(ui, COL_OPS, i18n.t("rules-col-ops"));
                    ui.end_row();

                    // 删除会缩短 rules 数组,同帧继续按旧索引渲染会越界
                    // 崩溃:删除后立即结束本帧表格,下一帧按新列表重建;
                    // 上移/下移同样会重排 rules 数组(move_rule 末尾
                    // sort_by_key),同帧继续会把被移动的行再画一遍
                    let mut removed = false;
                    let mut moved = false;
                    for i in 0..rules.rules.len() {
                        let rule = rules.rules[i].clone();
                        let row_top = ui.cursor().top();
                        row_hover.begin(ui, table_left, table_right, row_top);
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
                                tracing::warn!(
                                    "[Rules] 切换规则 {}({}) 启停失败: {e}",
                                    rule.name,
                                    rule.id
                                );
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
                                    RichText::new(name_text).size(theme::font::BODY).color(
                                        if is_temp {
                                            theme::c().text_dim
                                        } else {
                                            theme::c().text
                                        },
                                    ),
                                )
                                .wrap_mode(egui::TextWrapMode::Truncate),
                            );
                        });
                        let (action_key, action_kind) = match rule.action {
                            Action::Allow => ("rule-action-allow", widgets::badge::BadgeKind::Ok),
                            Action::Block => {
                                ("rule-action-block", widgets::badge::BadgeKind::Danger)
                            }
                        };
                        widgets::badge::badge_centered(
                            ui,
                            COL_ACTION,
                            26.0,
                            &i18n.t(action_key),
                            action_kind,
                        );
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
                        // 命中数列:会话内累计命中连接数(0 弱化灰);
                        // 临时规则与兜底不计,poll 层 1s 口径
                        let hits = rules.hit_count(rule.id);
                        widgets::table::fixed_num_cell(ui, COL_HITS, |ui| {
                            widgets::table::num_cell(
                                ui,
                                hits.to_string(),
                                if hits == 0 {
                                    theme::c().text_dim
                                } else {
                                    theme::c().text
                                },
                            );
                        });
                        widgets::table::fixed_cell(ui, COL_OPS, 26.0, |ui| {
                            ui.style_mut().spacing.item_spacing.x = 2.0;
                            // 首行禁上移、末行禁下移(边界置灰,不可点);
                            // 会话临时规则(负 id)不参与排序,移动一律禁用
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
                                    moved = true;
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
                                    moved = true;
                                }
                            }
                            if widgets::button::icon_btn(
                                ui,
                                icons::PENCIL,
                                Some(i18n.t("rules-edit")),
                                false,
                                true,
                            )
                            .clicked()
                            {
                                state.draft = Some(Draft::from_rule(&rule));
                            }
                            if widgets::button::icon_btn(
                                ui,
                                icons::TRASH,
                                Some(i18n.t("rules-delete")),
                                true,
                                true,
                            )
                            .clicked()
                            {
                                if let Err(e) = rules.delete(db, rule.id) {
                                    tracing::warn!("[Rules] 删除规则 {} 失败: {e}", rule.id);
                                }
                                removed = true;
                            }
                        });
                        ui.end_row();
                        row_hover.end(ui, row_top);
                        if removed || moved {
                            break;
                        }
                    }
                });
        });
}
