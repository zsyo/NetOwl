//! 连接列表页:过滤/搜索、表头排序编排与规则求值标注。
//! 行渲染在 rows,行右键菜单在 menu,排序在 sort。

mod listen;
mod menu;
mod rows;
mod sort;

use super::ConnView;

use eframe::egui;
use egui::RichText;

use super::{ConnSort, ConnSortState, UiCtx, conn_visible, icons, theme, widgets};
use crate::model::Connection;

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
        i18n,
        conn_view,
        listens: _,
        rdns,
        icon_tex,
        default_icon_tex,
        config,
        rules,
        conn_rates,
        conn_sort,
        conn_search,
        conn_row_hover,
        elevated,
        ..
    } = ctx;
    // &mut UiCtx 解构出的引用字段带两层 &mut,借类型注解 coerce 回单层
    let conn_sort: &mut ConnSortState = conn_sort;
    let conn_search: &mut String = conn_search;
    let conn_row_hover: &mut widgets::table::RowHover = conn_row_hover;
    let elevated = *elevated;
    widgets::header::page_header(ui, &i18n.t("conns-title"), &i18n.t("conns-subtitle"));
    ui.add_space(theme::sp::SM);

    let mut changed = false;
    // &mut UiCtx 解构出的引用字段带两层 &mut,借类型注解 coerce 回单层
    let conn_view: &mut ConnView = conn_view;
    // 视图切换:活动连接 / 端口监听
    let view_items = [
        (i18n.t("conns-view-conns"), icons::LIST_UL),
        (i18n.t("conns-view-listen"), icons::ETHERNET),
    ];
    let view_items: Vec<(&str, &str)> = view_items.iter().map(|(t, g)| (t.as_str(), *g)).collect();
    if let Some(i) = widgets::segmented::segmented(
        ui,
        &view_items,
        usize::from(*conn_view == ConnView::Listens),
    ) {
        *conn_view = if i == 0 {
            ConnView::Conns
        } else {
            ConnView::Listens
        };
    }
    ui.add_space(theme::sp::XS);

    if *conn_view == ConnView::Listens {
        listen::listen_table(ui, ctx);
        return changed;
    }

    // 本地/局域网远端噪音过滤(config 持久化,连接页与历史页共享)
    // + 搜索框(进程/映像路径/远端 IP/rDNS 域名包含匹配,会话态)
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(icons::FUNNEL)
                .size(theme::font::XS)
                .color(theme::c().text_dim),
        );
        if ui
            .checkbox(&mut config.general.hide_local, i18n.t("filter-hide-local"))
            .changed()
        {
            changed = true;
        }
        if ui
            .checkbox(&mut config.general.hide_lan, i18n.t("filter-hide-lan"))
            .changed()
        {
            changed = true;
        }
        ui.add_space(theme::sp::MD);
        ui.label(
            RichText::new(icons::SEARCH)
                .size(theme::font::XS)
                .color(theme::c().text_dim),
        );
        widgets::search_box::search_box(ui, conn_search, i18n.t("conns-search"), 220.0);
    });
    ui.add_space(theme::sp::XS);

    // 搜索词匹配:进程名/映像路径/远端 IP/rDNS 域名包含(大小写不敏感);
    // 与显示过滤叠加,空词即全量
    let needle = conn_search.trim().to_lowercase();
    let match_search = |c: &Connection| -> bool {
        needle.is_empty()
            || c.process.to_lowercase().contains(&needle)
            || c.proc_path
                .as_deref()
                .is_some_and(|p| p.to_lowercase().contains(&needle))
            || c.remote_ip.to_string().contains(&needle)
            || rdns
                .lookup(c.remote_ip)
                .is_some_and(|d| d.to_lowercase().contains(&needle))
    };
    let mut shown: Vec<&Connection> = conns
        .iter()
        .filter(|c| conn_visible(config, c) && match_search(c))
        .collect();
    sort::sort_conns(&mut shown, conn_sort, conn_rates, i18n);
    if shown.is_empty() {
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

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let table_left = ui.max_rect().left();
            let table_right = ui.max_rect().right();
            egui::Grid::new("connections_grid")
                .num_columns(9)
                .striped(true)
                .spacing([0.0, widgets::table::ROW_SPACING_Y])
                .show(ui, |ui| {
                    // 进程/远端弹性列长文本 Truncate 逐步展示;列贴列布局,
                    // 内容间隔由单元格水平内边距形成
                    for conn in shown {
                        rows::conn_row(
                            ui,
                            conn,
                            i18n,
                            icon_tex,
                            *default_icon_tex,
                            rdns,
                            conn_rates,
                            rules,
                            conn_row_hover,
                            table_left,
                            table_right,
                            flex_w,
                            elevated,
                        );
                    }
                });
        });
    changed
}

/// 进程/远端两弹性列的单列宽:表格总宽减定宽列均分,窗口过窄时兜底 320
fn flex_w_of(table_w: f32) -> f32 {
    ((table_w - (C_PROTO_W + C_LOC_W + C_RATE_W * 2.0 + C_TOTAL_W * 2.0 + C_ACTION_W)).max(320.0))
        * 0.5
}
