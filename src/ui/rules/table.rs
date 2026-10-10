//! 规则列表表格编排:表头(命中列带口径悬停说明)、滚动 Grid 与行循环。
//! 单行渲染在 row,列宽常量与行渲染共用。

use eframe::egui;
use egui::{Label, RichText};

use super::PageState;
use super::row;
use rusqlite::Connection as Db;

use crate::i18n::I18n;
use crate::model::CountUnits;
use crate::rules::RuleSet;
use crate::ui::{theme, widgets};

/// 表格列宽(逻辑点);表头与数据列同宽,内容居中。
/// 列贴列布局(Grid spacing.x = 0),内容与列缘间距由单元格内边距
/// CELL_PAD_X 提供,定宽列 = 内容宽 + 2×CELL_PAD_X;
/// 名称/进程/远端三列弹性列宽:均分剩余宽,窗口放大时表格铺满中央区
pub(super) const COL_ENABLED: f32 = 64.0;
pub(super) const COL_ACTION: f32 = 64.0;
pub(super) const COL_DIRECTION: f32 = 64.0;
pub(super) const COL_PROTO: f32 = 64.0;
pub(super) const COL_PORT: f32 = 64.0;
/// 命中数列:累计命中连接数(落库,每连接计一次)。宽度按最宽紧凑值
/// 预留(中文 "9999万"/"1000万亿" ≈ 4 半角 + 2 全角,西文 "9999T+")
pub(super) const COL_HITS: f32 = 84.0;
pub(super) const COL_OPS: f32 = 128.0;

fn header_cell(ui: &mut egui::Ui, w: f32, text: String) -> egui::Response {
    // 列贴列布局:占位整列宽,内容区(w - 2×CELL_PAD_X)内居中,与数据格
    // (fixed_cell 内容同缩进)同心;Grid 格内不能 add_space(egui 断言),
    // 用定宽占位 + 缩进子区域承载
    let pad = widgets::table::CELL_PAD_X;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 16.0), egui::Sense::hover());
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(pad, 0.0))));
    // add_sized(居中布局)实测表头稳定居中于列;徽章列的数据格用
    // 手动 add_space 居中(见 row),两者同心
    child.add_sized(
        [w - 2.0 * pad, 16.0],
        Label::new(
            RichText::new(text)
                .size(theme::font::SM)
                .strong()
                .color(theme::c().text_dim),
        ),
    );
    resp
}

/// 规则表(自上而下即优先级从高到低)
pub(super) fn rules_table(
    ui: &mut egui::Ui,
    state: &mut PageState,
    i18n: &I18n,
    db: &Db,
    rules: &mut RuleSet,
    row_hover: &mut widgets::table::RowHover,
    count_units: CountUnits,
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
                .striped(false)
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
                    // 悬停说明口径:累计、每连接计一次、跨重启保留
                    header_cell(ui, COL_HITS, i18n.t("rules-col-hits"))
                        .on_hover_text(i18n.t("rules-col-hits-tip"));
                    header_cell(ui, COL_OPS, i18n.t("rules-col-ops"));
                    ui.end_row();

                    // 删除会缩短 rules 数组、上移/下移会重排 rules 数组,
                    // 同帧继续按旧索引渲染会越界崩溃或把被移动的行再画
                    // 一遍:操作后立即结束本帧表格,下一帧按新列表重建
                    for i in 0..rules.rules.len() {
                        // 克隆所有权副本:行内要改 rules,不能同时持有其
                        // 规则借用(见 row::rule_row)
                        let rule = rules.rules[i].clone();
                        let ops = row::rule_row(
                            ui,
                            state,
                            i18n,
                            db,
                            rules,
                            row_hover,
                            rule,
                            i,
                            flex_w,
                            table_left,
                            table_right,
                            count_units,
                        );
                        if ops.removed || ops.moved {
                            break;
                        }
                    }
                });
        });
}
