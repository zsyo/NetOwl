//! 连接页工具栏:视图切换(活动连接/端口监听)、噪音过滤(隐藏本地/
//! 局域网)、按进程分组、协议三态筛选与搜索框;监听视图仅协议+搜索。
//! 筛选项随视图切换,行高统一 TOOLBAR_ROW_H。

use eframe::egui;
use egui::RichText;

use super::super::{ConnView, TOOLBAR_ROW_H, icons, theme, widgets};
use crate::i18n::I18n;
use crate::model::Protocol;
use crate::storage::config::Config;

/// 工具栏一行;返回是否直接改动了配置(隐藏本地/局域网开关)
pub(super) fn toolbar(
    ui: &mut egui::Ui,
    i18n: &I18n,
    conn_view: &mut ConnView,
    conn_grouped: &mut bool,
    config: &mut Config,
    conn_proto: &mut Option<Protocol>,
    conn_search: &mut String,
) -> bool {
    // 视图切换:活动连接 / 端口监听
    let view_items = [
        (i18n.t("conns-view-conns"), icons::LIST_UL),
        (i18n.t("conns-view-listen"), icons::ETHERNET),
    ];
    let view_items: Vec<(&str, &str)> = view_items.iter().map(|(t, g)| (t.as_str(), *g)).collect();
    // 工具栏一行:视图切换 + 筛选项(与历史页同形态);筛选随视图切换——
    // 连接视图为噪音过滤/分组/搜索,监听视图仅搜索(按进程/路径过滤)
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
        if *conn_view == ConnView::Listens {
            proto_filter_segmented(ui, conn_proto, i18n);
            ui.add_space(theme::sp::MD);
            ui.label(
                RichText::new(icons::SEARCH)
                    .size(theme::font::XS)
                    .color(theme::c().text_dim),
            );
            widgets::search_box::search_box(ui, conn_search, i18n.t("listen-search"), 220.0);
            return false;
        }
        // 按进程分组:列表视图形态(平铺/分组),会话态不入 config;
        // 与噪音过滤含义不同,紧随视图切换、留隙与过滤组分段
        ui.checkbox(conn_grouped, i18n.t("conns-group"));
        ui.add_space(theme::sp::MD);
        let mut changed = false;
        ui.label(
            RichText::new(icons::FUNNEL)
                .size(theme::font::XS)
                .color(theme::c().text_dim),
        );
        // 本地/局域网远端噪音过滤(config 持久化,连接页与历史页共享)
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
        ui.add_space(theme::sp::SM);
        proto_filter_segmented(ui, conn_proto, i18n);
        ui.add_space(theme::sp::MD);
        // 搜索框(进程/映像路径/远端 IP/rDNS 域名包含匹配,会话态)
        ui.label(
            RichText::new(icons::SEARCH)
                .size(theme::font::XS)
                .color(theme::c().text_dim),
        );
        widgets::search_box::search_box(ui, conn_search, i18n.t("conns-search"), 220.0);
        changed
    });
    toolbar_changed.inner
}

/// 协议筛选三态分段(全部/TCP/UDP):连接与监听两视图共用一份会话态
fn proto_filter_segmented(ui: &mut egui::Ui, conn_proto: &mut Option<Protocol>, i18n: &I18n) {
    let items = [
        (i18n.t("proto-all"), ""),
        ("TCP".to_owned(), ""),
        ("UDP".to_owned(), ""),
    ];
    let items: Vec<(&str, &str)> = items.iter().map(|(t, g)| (t.as_str(), *g)).collect();
    let selected = match conn_proto {
        None => 0,
        Some(Protocol::Tcp) => 1,
        Some(Protocol::Udp) => 2,
    };
    if let Some(i) = widgets::segmented::segmented(ui, &items, selected) {
        *conn_proto = match i {
            1 => Some(Protocol::Tcp),
            2 => Some(Protocol::Udp),
            _ => None,
        };
    }
}
