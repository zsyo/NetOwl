//! 连接列表分组头行:按进程分组视图下的组头(图标 + 名称 + 连接数
//! 徽章 + 组上/下行累计),整行可点折叠/展开。连接行渲染在 rows。

use eframe::egui;
use egui::RichText;

use std::collections::HashMap;

use crate::i18n::I18n;
use crate::model::{Connection, fmt_bytes};
use crate::ui::{theme, widgets};

/// 分组头行高(单行加高)
const GROUP_HEADER_H: f32 = 26.0;

/// 分组头行:首列 = 进程图标 + 名称 + 连接数,下载/上传总量列 =
/// 组上/下行累计(与表头语义对齐);整行可点(折叠/展开)。Grid 内
/// 按连接行列序占满 9 列(其余列空占位),行高恒定(单行加高)使
/// 悬停垫底可先于内容绘制,不会盖住名称;点击经行末 interact
/// (整行内唯一 click 感知件)
#[allow(clippy::too_many_arguments)]
pub(super) fn group_header(
    ui: &mut egui::Ui,
    name: &str,
    group: &[&Connection],
    i18n: &I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    flex_w: f32,
    table_left: f32,
    table_right: f32,
) -> bool {
    use super::{C_ACTION_W, C_LOC_W, C_PROTO_W, C_RATE_W, C_TOTAL_W};
    let row_top = ui.cursor().top();
    let row_rect = egui::Rect::from_min_max(
        egui::pos2(table_left, row_top),
        egui::pos2(table_right, row_top + GROUP_HEADER_H),
    );
    // 悬停垫底 + 左缘 accent 竖条(先于内容绘制,与连接行垫底同法)
    if ui.rect_contains_pointer(row_rect) {
        let p = theme::c();
        ui.painter().rect_filled(row_rect, 0.0, p.hover_bg);
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(table_left, row_top),
                egui::pos2(table_left + 2.0, row_rect.bottom()),
            ),
            0.0,
            p.accent,
        );
    }
    // 首列:图标 + 名称 + 连接数
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(flex_w, GROUP_HEADER_H), egui::Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(widgets::table::CELL_PAD_X, 0.0))),
    );
    child.horizontal(|ui| {
        let tex = group.iter().find_map(|c| {
            c.proc_path
                .as_deref()
                .and_then(|p| icon_tex.get(p))
                .and_then(|t| t.as_ref())
        });
        widgets::process::proc_icon(ui, tex, default_icon_tex, 16.0);
        let display = if name.is_empty() {
            i18n.t("conn-proc-unknown")
        } else {
            name.to_owned()
        };
        ui.add(
            egui::Label::new(
                RichText::new(display)
                    .size(theme::font::BODY)
                    .strong()
                    .color(theme::c().text),
            )
            .wrap_mode(egui::TextWrapMode::Truncate),
        );
        ui.label(
            RichText::new(
                i18n.t_with_args("conns-group-count", &[("count", group.len().to_string())]),
            )
            .size(theme::font::SM)
            .color(theme::c().text_dim),
        );
    });
    // 其余列按连接行列序排布:协议/远端/位置/两速率列空占位,组累计
    // 画进下载/上传总量列,动作列空占位,收行保持 Grid 行结构完整
    for w in [C_PROTO_W, flex_w, C_LOC_W, C_RATE_W, C_RATE_W] {
        ui.allocate_exact_size(egui::vec2(w, GROUP_HEADER_H), egui::Sense::hover());
    }
    let (up, down) = group
        .iter()
        .fold((0u64, 0u64), |a, c| (a.0 + c.bytes_out, a.1 + c.bytes_in));
    group_total_cell(ui, C_TOTAL_W, fmt_bytes(down), theme::c().inbound);
    group_total_cell(ui, C_TOTAL_W, fmt_bytes(up), theme::c().outbound);
    ui.allocate_exact_size(egui::vec2(C_ACTION_W, GROUP_HEADER_H), egui::Sense::hover());
    ui.end_row();
    // 整行点击区:行内格均为 hover 感知,click 命中唯一落在本件
    ui.interact(
        row_rect,
        egui::Id::new(("conn_group_header", name)),
        egui::Sense::click(),
    )
    .clicked()
}

/// 分组头累计单元格:右对齐等宽小字
fn group_total_cell(ui: &mut egui::Ui, w: f32, text: String, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, GROUP_HEADER_H), egui::Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(widgets::table::CELL_PAD_X, 0.0))),
    );
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(
            RichText::new(text)
                .size(theme::font::SM)
                .font(egui::FontId::monospace(theme::font::SM))
                .color(color),
        );
    });
}
