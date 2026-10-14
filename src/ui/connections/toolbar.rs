//! 连接页工具栏:视图切换(活动连接/端口监听)、搜索框、协议筛选
//! 下拉、噪音过滤下拉(隐藏本地/局域网)与按进程分组;监听视图仅
//! 搜索+协议。筛选项随视图切换,行高统一 TOOLBAR_ROW_H;协议与筛选
//! 下拉的形态与历史页一致(跨页面统一,降低学习成本)。

use eframe::egui;
use egui::RichText;

use super::super::{ConnView, TOOLBAR_ROW_H, icons, theme, widgets};
use crate::i18n::I18n;
use crate::model::Protocol;
use crate::storage::config::Config;

/// 工具栏一行;返回是否直接改动了配置(隐藏本地/局域网开关)
#[allow(clippy::too_many_arguments)]
pub(super) fn toolbar(
    ui: &mut egui::Ui,
    i18n: &I18n,
    conn_view: &mut ConnView,
    conn_grouped: &mut bool,
    config: &mut Config,
    conn_proto: &mut Option<Protocol>,
    conn_search: &mut String,
    focus_conn_search: &mut bool,
) -> bool {
    // 视图切换:活动连接 / 端口监听
    let view_items = [
        (i18n.t("conns-view-conns"), icons::LIST_UL),
        (i18n.t("conns-view-listen"), icons::ETHERNET),
    ];
    let view_items: Vec<(&str, &str)> = view_items.iter().map(|(t, g)| (t.as_str(), *g)).collect();
    // 工具栏一行:视图切换 + 筛选项(顺序与历史页对齐:搜索 → 协议 →
    // 筛选 → 分组);筛选随视图切换——连接视图含噪音过滤/分组,监听
    // 视图仅搜索(按进程/路径过滤)
    let toolbar_changed = ui.horizontal(|ui| {
        // 行高抬升只作用于本工具栏行(style_mut 泄漏到整页会把表格
        // Grid 的最小行高一并抬到 26,行内容与色带错位);统一控件
        // 最小交互高,带图标与纯文本的 segmented 等高对齐
        ui.style_mut().spacing.interact_size.y = TOOLBAR_ROW_H;
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
        ui.add_space(theme::sp::MD);

        // 1. 搜索框(进程/映像路径/远端 IP/rDNS 域名包含匹配,会话态);
        // Ctrl+F 聚焦(经 App 层标志位),焦点在框内时 Esc 清空搜索词
        // (连接/监听两视图共用一份搜索词)
        ui.label(
            RichText::new(icons::SEARCH)
                .size(theme::font::XS)
                .color(theme::c().text_dim),
        );
        let hint = if *conn_view == ConnView::Listens {
            i18n.t("listen-search")
        } else {
            i18n.t("conns-search")
        };
        let search =
            widgets::search_box::search_box_focus(ui, conn_search, hint, 220.0, focus_conn_search);
        if search.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            conn_search.clear();
        }
        ui.add_space(theme::sp::MD);

        // 2. 协议筛选下拉(收起态 ComboBox,与历史页协议筛选同型)
        proto_filter_combo(ui, conn_proto, i18n);

        if *conn_view == ConnView::Listens {
            return false;
        }
        ui.add_space(theme::sp::MD);

        // 3. 噪音过滤下拉(隐藏本地/局域网,与历史页共用的组件)
        let changed = widgets::noise_filter::noise_filter(
            ui,
            i18n,
            &mut config.general.hide_local,
            &mut config.general.hide_lan,
        );
        ui.add_space(theme::sp::MD);

        // 4. 按进程分组:列表视图形态(平铺/分组),会话态不入 config
        ui.checkbox(conn_grouped, i18n.t("conns-group"));
        changed
    });
    toolbar_changed.inner
}

/// 协议筛选下拉(全部/TCP/UDP):连接与监听两视图共用一份会话态;
/// 收起态 ComboBox 与历史页同型(原为常展开 segmented,三态平铺在
/// 英文下占宽且与历史页形态不一致)
fn proto_filter_combo(ui: &mut egui::Ui, conn_proto: &mut Option<Protocol>, i18n: &I18n) {
    egui::ComboBox::from_id_salt("conns-proto")
        .width(90.0)
        .selected_text(RichText::new(proto_name(i18n, *conn_proto)).size(theme::font::BODY))
        .show_ui(ui, |ui| {
            for p in [None, Some(Protocol::Tcp), Some(Protocol::Udp)] {
                if ui
                    .selectable_label(
                        *conn_proto == p,
                        RichText::new(proto_name(i18n, p)).size(theme::font::BODY),
                    )
                    .clicked()
                {
                    *conn_proto = p;
                }
            }
        });
}

fn proto_name(i18n: &I18n, proto: Option<Protocol>) -> String {
    match proto {
        None => i18n.t("filter-proto-all"),
        Some(p) => p.as_str().to_owned(),
    }
}
