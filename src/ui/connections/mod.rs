//! 连接列表页:过滤/搜索、表头排序编排与规则求值标注。
//! 行渲染在 rows,行右键菜单在 menu,排序在 sort,过滤/排序/分组
//! 结果缓存与重建在 cache。

mod cache;
mod group;

pub use cache::ConnListCache;
mod listen;
mod rows;
mod sort;
mod toolbar;

use super::ConnView;

use eframe::egui;

use super::{ConnSort, ConnSortState, UiCtx, theme, widgets};
use crate::model::Protocol;

/// 连接表定宽列(协议/归属/速率/累计/动作):表头与行渲染共用,
/// mod 与 rows 引用同一组常量防两份定义错位
pub(super) const C_PROTO_W: f32 = 64.0;
pub(super) const C_LOC_W: f32 = 112.0;
pub(super) const C_RATE_W: f32 = 90.0;
pub(super) const C_TOTAL_W: f32 = 90.0;
pub(super) const C_ACTION_W: f32 = 70.0;

/// 连接列表页;返回是否直接改动了配置(隐藏本地/局域网开关)。
/// 末列显示规则求值动作(允许/阻断,规则引擎默认放行)
pub(super) fn connections_ui(ui: &mut egui::Ui, ctx: &mut UiCtx) -> bool {
    let UiCtx {
        conns,
        conns_gen,
        conn_list_cache,
        i18n,
        conn_view,
        conn_grouped,
        conn_collapsed,
        listens: _,
        rdns,
        icon_tex,
        default_icon_tex,
        config,
        rules,
        conn_rates,
        conn_sort,
        conn_search,
        focus_conn_search,
        conn_proto,
        conn_row_hover,
        row_lock,
        row_hits,
        ..
    } = ctx;
    // &mut UiCtx 解构出的引用字段带两层 &mut,借类型注解 coerce 回单层
    let conn_sort: &mut ConnSortState = conn_sort;
    let conn_search: &mut String = conn_search;
    let focus_conn_search: &mut bool = focus_conn_search;
    let conn_proto: &mut Option<Protocol> = conn_proto;
    let conn_row_hover: &mut widgets::table::RowHover = conn_row_hover;
    let row_lock: &widgets::table::RowHoverLock = row_lock;
    widgets::header::page_header(ui, &i18n.t("conns-title"), &i18n.t("conns-subtitle"));
    ui.add_space(theme::sp::SM);

    let mut changed = false;
    // &mut UiCtx 解构出的引用字段带两层 &mut,借类型注解 coerce 回单层
    let conn_view: &mut ConnView = conn_view;
    let conn_grouped: &mut bool = conn_grouped;
    changed |= toolbar::toolbar(
        ui,
        i18n,
        conn_view,
        conn_grouped,
        config,
        conn_proto,
        conn_search,
        focus_conn_search,
    );
    ui.add_space(theme::sp::XS);

    if *conn_view == ConnView::Listens {
        listen::listen_table(ui, ctx);
        return changed;
    }

    // 过滤/排序/分组结果缓存:1s 采集轮或用户改设置/折叠时重建,
    // 交互帧直接复用(见 cache)
    let needle = conn_search.trim().to_lowercase();
    conn_list_cache.ensure_fresh(
        conns,
        *conns_gen,
        *conn_sort,
        &needle,
        *conn_proto,
        config,
        *conn_grouped,
        conn_collapsed,
        conn_rates,
        i18n,
        rdns,
    );
    let flat_len = conn_list_cache.len();
    if conn_list_cache.is_empty() {
        ui.add_space(theme::sp::LG);
        // 全部连接被隐藏/搜索无命中与确实无连接区分提示
        let key = if conns.is_empty() {
            "conns-empty"
        } else {
            "conns-no-match"
        };
        ui.label(theme::dim_text(&i18n.t(key), theme::font::H3));
        return changed;
    }

    // 表头:可排序列整格可点击(当前排序列高亮并带方向三角),协议/
    // 远端/动作为纯展示列;数字列数据右对齐表头贴右缘。固定在滚动区外,
    // 与历史/局域网页同形态;列宽与数据定宽一致(表格总宽在 Grid 之外取:
    // Grid 闭包内 available_width 被 grid 布局器接管,返回上帧列宽)
    let flex_w = flex_w_of(ui.available_width());
    widgets::table::sort_header_grid(
        ui,
        "connections_header",
        &[
            widgets::table::SortCol::new("col-process", Some(ConnSort::Process), flex_w, false),
            widgets::table::SortCol::new("col-proto", None, C_PROTO_W, false),
            widgets::table::SortCol::new("col-remote", None, flex_w, false),
            widgets::table::SortCol::new("col-location", Some(ConnSort::Location), C_LOC_W, false),
            widgets::table::SortCol::new("col-down", Some(ConnSort::RateDown), C_RATE_W, true),
            widgets::table::SortCol::new("col-up", Some(ConnSort::RateUp), C_RATE_W, true),
            widgets::table::SortCol::new(
                "col-down-total",
                Some(ConnSort::TotalDown),
                C_TOTAL_W,
                true,
            ),
            widgets::table::SortCol::new("col-up-total", Some(ConnSort::TotalUp), C_TOTAL_W, true),
            widgets::table::SortCol::new("col-action", None, C_ACTION_W, false),
        ],
        *conn_sort,
        |s| {
            let current: ConnSortState = *conn_sort;
            *conn_sort = Some(match current {
                Some((k, asc)) if k == s => (s, !asc),
                _ => (s, true),
            });
        },
        &|k| i18n.t(k),
    );

    // 虚拟化:扁平行列表(分组头/连接行交错,统一 36 行高)后
    // show_rows 只布局可见行——连接数上百时全量 Grid 的帧耗时
    // 随连接数线性增长(每行含规则求值与十余次文本布局)
    ui.spacing_mut().item_spacing.y = widgets::table::ROW_SPACING_Y;
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
        ui,
        CONN_ROW_H,
        flat_len,
        |ui, row_range| {
            // 首行色带上沿的半个行距扩张会被滚动区视口裁掉(见
            // widgets::table::row_area_top_pad),预留出来保持与其余行同高
            ui.add_space(widgets::table::row_area_top_pad());
            let table_left = ui.max_rect().left();
            let table_right = ui.max_rect().right();
            egui::Grid::new("connections_grid")
                .num_columns(9)
                .striped(false)
                .spacing([0.0, widgets::table::ROW_SPACING_Y])
                .show(ui, |ui| {
                    for idx in row_range {
                        match conn_list_cache.row(idx) {
                            cache::FlatEntry::Conn(ci) => {
                                rows::conn_row(
                                    ui,
                                    &conns[*ci],
                                    i18n,
                                    icon_tex,
                                    *default_icon_tex,
                                    rdns,
                                    conn_rates,
                                    rules,
                                    conn_row_hover,
                                    row_lock,
                                    row_hits,
                                    table_left,
                                    table_right,
                                    flex_w,
                                    idx,
                                );
                            }
                            cache::FlatEntry::Header(data) => {
                                if group::group_header(
                                    ui,
                                    data,
                                    i18n,
                                    icon_tex,
                                    *default_icon_tex,
                                    flex_w,
                                    table_left,
                                    table_right,
                                    idx,
                                ) && !conn_collapsed.remove(&data.name)
                                {
                                    conn_collapsed.insert(data.name.clone());
                                }
                            }
                        }
                    }
                });
        },
    );
    changed
}

/// 连接表行内容高(与 show_rows 的行高参数一致;分组头同行高)
const CONN_ROW_H: f32 = 36.0;

/// 进程/远端两弹性列的单列宽:表格总宽减定宽列均分,窗口过窄时兜底 320
fn flex_w_of(table_w: f32) -> f32 {
    ((table_w - (C_PROTO_W + C_LOC_W + C_RATE_W * 2.0 + C_TOTAL_W * 2.0 + C_ACTION_W)).max(320.0))
        * 0.5
}
