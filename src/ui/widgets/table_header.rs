//! 表头 Grid 公共件:纯展示表头与可排序表头(整格可点、激活高亮),
//! 供连接/历史/局域网等页的独立表头使用;单元格与行悬停件在 table。

use eframe::egui;

use super::table::{header_cell_w, header_sort_cell};

/// 表头列定义(可排序表头 Grid 用):sort 为 None 的列纯展示不可点
pub struct SortCol<S> {
    pub key: &'static str,
    pub sort: Option<S>,
    pub w: f32,
    pub right: bool,
}

impl<S> SortCol<S> {
    pub fn new(key: &'static str, sort: Option<S>, w: f32, right: bool) -> Self {
        SortCol {
            key,
            sort,
            w,
            right,
        }
    }
}

/// 独立表头 Grid(位于滚动区外,列宽与数据 Grid 一致):纯展示列,
/// 列 = (词条键, 宽, 右对齐);历史明细/局域网等无排序表用
pub fn header_grid(
    ui: &mut egui::Ui,
    id: &str,
    cols: &[(&'static str, f32, bool)],
    t: &dyn Fn(&str) -> String,
) {
    egui::Grid::new(egui::Id::new(id))
        .num_columns(cols.len())
        .spacing([0.0, 0.0])
        .show(ui, |ui| {
            for (key, w, right) in cols {
                header_cell_w(ui, *w, &t(key), *right);
            }
            ui.end_row();
        });
}

/// 可排序表头 Grid(可混合纯展示列):可排序列整格可点、激活列高亮
/// 带方向三角,点击交 on_click 由调用方翻转排序状态(各页新列的默认
/// 方向语义不同);连接/历史聚合/历史汇总共用
pub fn sort_header_grid<S: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    cols: &[SortCol<S>],
    current: Option<(S, bool)>,
    mut on_click: impl FnMut(S),
    t: &dyn Fn(&str) -> String,
) {
    egui::Grid::new(egui::Id::new(id))
        .num_columns(cols.len())
        .spacing([0.0, 0.0])
        .show(ui, |ui| {
            for c in cols {
                let text = t(c.key);
                match c.sort {
                    Some(s) => {
                        let active = current.is_some_and(|(k, _)| k == s);
                        let ascending = current.is_some_and(|(_, asc)| asc);
                        let r = header_sort_cell(ui, &text, active, ascending, c.right, c.w);
                        if r.clicked() {
                            on_click(s);
                        }
                    }
                    None => header_cell_w(ui, c.w, &text, c.right),
                }
            }
            ui.end_row();
        });
}
