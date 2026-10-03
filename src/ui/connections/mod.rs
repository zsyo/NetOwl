//! 连接列表页:过滤/搜索、表头排序编排与规则求值标注。
//! 行渲染在 rows,行右键菜单在 menu,排序在 sort。

mod menu;
mod rows;
mod sort;

use eframe::egui;
use egui::RichText;

use super::{ConnSort, ConnSortState, UiCtx, conn_visible, icons, theme, widgets};
use crate::model::Connection;

/// 连接列表页;返回是否直接改动了配置(隐藏本地/局域网开关)。
/// 末列显示规则求值动作(允许/阻断,规则引擎默认放行)
pub(super) fn connections_ui(ui: &mut egui::Ui, ctx: &mut UiCtx) -> bool {
    let UiCtx {
        conns,
        i18n,
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

    // 本地/局域网远端噪音过滤(config 持久化,连接页与历史页共享)
    // + 搜索框(进程/映像路径/远端 IP/rDNS 域名包含匹配,会话态)
    let mut changed = false;
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
        ui.add(
            egui::TextEdit::singleline(conn_search)
                .hint_text(i18n.t("conns-search"))
                .desired_width(220.0),
        );
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

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let table_left = ui.max_rect().left();
            let table_right = ui.max_rect().right();
            // 表格总宽必须在 Grid 之外取:Grid 闭包内 available_width 被
            // grid 布局器接管,返回当前列宽(上帧值)而非总宽
            let table_w = ui.available_width();
            egui::Grid::new("connections_grid")
                .num_columns(9)
                .striped(true)
                .spacing([0.0, widgets::table::ROW_SPACING_Y])
                .show(ui, |ui| {
                    // 定宽列(容纳表头与内容上限)+ 进程/远端弹性列:窗口放大时
                    // 表格铺满中央区,弹性列长文本 Truncate 逐步展示;
                    // 列贴列布局,内容间隔由单元格水平内边距形成
                    const C_PROTO_W: f32 = 64.0;
                    const C_LOC_W: f32 = 112.0;
                    const C_RATE_W: f32 = 90.0;
                    const C_TOTAL_W: f32 = 90.0;
                    const C_ACTION_W: f32 = 70.0;
                    let flex_total = (table_w
                        - (C_PROTO_W + C_LOC_W + C_RATE_W * 2.0 + C_TOTAL_W * 2.0 + C_ACTION_W))
                        .max(320.0);
                    let flex_w = flex_total * 0.5;
                    // 表头:可排序列整格可点击(当前排序列高亮并带方向三角),
                    // 协议/远端/动作为纯展示列,不给手型光标(不可点);
                    // 数字列数据右对齐,表头同步贴列右缘(列宽与数据定宽一致)
                    let mut header = |ui: &mut egui::Ui,
                                      key: &str,
                                      sort: Option<ConnSort>,
                                      right: bool,
                                      w: f32| {
                        let active = conn_sort.is_some_and(|(k, _)| Some(k) == sort);
                        match sort {
                            Some(s) => {
                                let ascending = conn_sort.is_some_and(|(_, asc)| asc);
                                let r = widgets::table::header_sort_cell(
                                    ui,
                                    &i18n.t(key),
                                    active,
                                    ascending,
                                    right,
                                    w,
                                );
                                if r.clicked() {
                                    let current: ConnSortState = *conn_sort;
                                    *conn_sort = Some(match current {
                                        Some((k, asc)) if k == s => (s, !asc),
                                        _ => (s, true),
                                    });
                                }
                            }
                            None => widgets::table::header_cell_w(ui, w, &i18n.t(key), right),
                        }
                    };
                    header(ui, "col-process", Some(ConnSort::Process), false, flex_w);
                    header(ui, "col-proto", None, false, C_PROTO_W);
                    header(ui, "col-remote", None, false, flex_w);
                    header(ui, "col-location", Some(ConnSort::Location), false, C_LOC_W);
                    header(ui, "col-down", Some(ConnSort::RateDown), true, C_RATE_W);
                    header(ui, "col-up", Some(ConnSort::RateUp), true, C_RATE_W);
                    header(
                        ui,
                        "col-down-total",
                        Some(ConnSort::TotalDown),
                        true,
                        C_TOTAL_W,
                    );
                    header(ui, "col-up-total", Some(ConnSort::TotalUp), true, C_TOTAL_W);
                    header(ui, "col-action", None, false, C_ACTION_W);
                    ui.end_row();

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
