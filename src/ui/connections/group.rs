//! 连接列表分组头行:按进程分组视图下的组头(图标 + 名称 + 连接数
//! 徽章 + 组上/下行累计),整行可点折叠/展开。连接行渲染在 rows。
//! 行高与连接行统一(36):虚拟化表格(show_rows)要求行高恒定,
//! 分组头与连接行交错排列也不能两种高度。

use eframe::egui;
use egui::RichText;

use std::collections::HashMap;

use super::{C_ACTION_W, C_LOC_W, C_PROTO_W, C_RATE_W, C_TOTAL_W};
use crate::i18n::I18n;
use crate::model::fmt_bytes;
use crate::ui::{theme, widgets};

/// 分组头行高:与连接行相同(见文件头说明)
const GROUP_HEADER_H: f32 = 36.0;

/// 分组头行数据(构建扁平行列表时预计算,渲染路径零聚合)
pub(super) struct HeaderData {
    pub(super) name: String,
    /// 组内代表进程的映像路径(取首个有路径的连接,供图标)
    pub(super) proc_path: Option<String>,
    pub(super) count: usize,
    pub(super) bytes_in: u64,
    pub(super) bytes_out: u64,
}

/// 分组头行:首列 = 进程图标 + 名称 + 连接数,下载/上传总量列 =
/// 组上/下行累计(与表头语义对齐);整行可点(折叠/展开)。Grid 内
/// 按连接行列序占满 9 列(其余列空占位),行高恒定使悬停垫底可先于
/// 内容绘制,不会盖住名称;点击经行末 interact(整行内唯一 click
/// 感知件)。`idx` 为绝对行号(斑马纹与虚拟化行范围无关)
#[allow(clippy::too_many_arguments)]
pub(super) fn group_header(
    ui: &mut egui::Ui,
    data: &HeaderData,
    i18n: &I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    flex_w: f32,
    table_left: f32,
    table_right: f32,
    idx: usize,
) -> bool {
    let row_top = ui.cursor().top();
    let row_rect = egui::Rect::from_min_max(
        egui::pos2(table_left, row_top),
        egui::pos2(table_right, row_top + GROUP_HEADER_H),
    );
    // 斑马纹(奇数列)+ 悬停垫底 + 左缘 accent 竖条,均先于内容绘制
    // (与连接行垫底同法;斑马按绝对行号,虚拟化下不与滚动联动翻转)
    let p = theme::c();
    if idx % 2 == 1 {
        ui.painter().rect_filled(row_rect, 0.0, p.faint);
    }
    if ui.rect_contains_pointer(row_rect) {
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
    // 首列:图标 + 名称 + 连接数(36 行高内垂直居中:内容带收进 18 高窄条)
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(flex_w, GROUP_HEADER_H), egui::Sense::hover());
    let inner = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 18.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new().max_rect(inner.shrink2(egui::vec2(widgets::table::CELL_PAD_X, 0.0))),
    );
    child.horizontal(|ui| {
        let tex = data
            .proc_path
            .as_deref()
            .and_then(|p| icon_tex.get(p))
            .and_then(|t| t.as_ref());
        widgets::process::proc_icon(ui, tex, default_icon_tex, 16.0);
        let display = if data.name.is_empty() {
            i18n.t("conn-proc-unknown")
        } else {
            data.name.clone()
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
                i18n.t_with_args("conns-group-count", &[("count", data.count.to_string())]),
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
    group_total_cell(ui, C_TOTAL_W, fmt_bytes(data.bytes_in), theme::c().inbound);
    group_total_cell(
        ui,
        C_TOTAL_W,
        fmt_bytes(data.bytes_out),
        theme::c().outbound,
    );
    ui.allocate_exact_size(egui::vec2(C_ACTION_W, GROUP_HEADER_H), egui::Sense::hover());
    ui.end_row();
    // 整行点击区:行内格均为 hover 感知,click 命中唯一落在本件
    ui.interact(
        row_rect,
        egui::Id::new(("conn_group_header", &data.name)),
        egui::Sense::click(),
    )
    .clicked()
}

/// 分组头累计单元格:右对齐等宽小字(36 行高内垂直居中)
fn group_total_cell(ui: &mut egui::Ui, w: f32, text: String, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, GROUP_HEADER_H), egui::Sense::hover());
    let inner = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 18.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new().max_rect(inner.shrink2(egui::vec2(widgets::table::CELL_PAD_X, 0.0))),
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
