//! 主窗口布局:导航侧栏与各页面(地图 / 连接 / 历史 / 规则 / 设置)。
//! 全部界面文本经 I18n 词条获取(AGENTS.md 规范 4)。

pub mod ask;
pub mod history;
pub mod icons;
pub mod log_window;
pub mod map_conn;
pub mod map_inspector;
pub mod map_panel;
pub mod map_widgets;
pub mod rules;
pub mod theme;
pub mod titlebar;
pub mod widgets;

mod connections;
mod settings;

use std::collections::HashMap;

use eframe::egui;
use egui::{Align2, Button, Color32, CornerRadius, FontId, Margin, RichText, Sense, Stroke};

use crate::collector::CollectorKind;
use crate::i18n::I18n;
use crate::map;
use crate::map::basemap;
use crate::model::{Connection, fmt_bytes};
use crate::net::rdns;
use crate::rules::wfp;
use crate::storage::config::Config;
use crate::storage::history as history_store;
use crate::storage::history_query;

/// 连接列表排序键(表头点击切换;None = 表快照原序)
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConnSort {
    Process,
    Location,
    RateDown,
    RateUp,
    TotalDown,
    TotalUp,
}

/// 连接列表排序状态:(键, 是否正序);点击已激活表头反转方向
pub type ConnSortState = Option<(ConnSort, bool)>;

/// 主窗口页面
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Page {
    Map,
    Connections,
    History,
    Rules,
    Settings,
}

/// 左侧导航项:(页面, 词条键, 图标码点(ui::icons,glyph 名见常量注释))
const NAV_ITEMS: &[(Page, &str, &str)] = &[
    (Page::Map, "nav-map", icons::GLOBE),
    (Page::Connections, "nav-connections", icons::LIST_UL),
    (Page::History, "nav-history", icons::CLOCK_HISTORY),
    (Page::Rules, "nav-rules", icons::SHIELD),
    (Page::Settings, "nav-settings", icons::GEAR),
];

/// 导航项尺寸与选中指示条
const NAV_ITEM_H: f32 = 36.0;
const NAV_INDICATOR_W: f32 = 3.0;
/// 导航选中/悬停过渡动画时长(秒)
const NAV_ANIM_SECS: f32 = 0.15;

/// 工具栏行内控件最小交互高度:egui horizontal 行高从默认 18px 起步,
/// 混排高低控件会基线错位;统一抬高让整行垂直居中(历史/规则/日志工具栏共用)
pub(crate) const TOOLBAR_ROW_H: f32 = 26.0;

/// 左侧导航栏:品牌区、页面切换(选中指示条 + 过渡)、底部速率卡与状态行
pub fn nav_ui(
    ui: &mut egui::Ui,
    page: &mut Page,
    ctx: &UiCtx,
    logo: &egui::TextureHandle,
    collector_kind: CollectorKind,
) {
    // 品牌区:应用图标圆角块 + 名称/副标题,左对齐
    ui.horizontal(|ui| {
        ui.add(
            egui::Image::new(logo)
                .fit_to_exact_size(egui::vec2(28.0, 28.0))
                .corner_radius(CornerRadius::same(theme::RADIUS_SM)),
        );
        ui.vertical(|ui| {
            ui.label(
                RichText::new(ctx.i18n.t("app-name"))
                    .size(theme::font::H3)
                    .strong()
                    .color(theme::c().text),
            );
            ui.label(theme::dim_text(
                &ctx.i18n.t("app-subtitle"),
                theme::font::MICRO,
            ));
        });
    });
    ui.add_space(theme::sp::MD);

    for (target, key, icon) in NAV_ITEMS {
        nav_item(ui, page, *target, &ctx.i18n.t(key), icon);
    }

    // 底部:监控状态在其上,速率卡贴底(bottom_up 先绘制者在底部)
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        // 会话累计 = 当前活跃连接字节总和(程序启动起算,重启归零;
        // 完结连接的字节转入历史库,由历史页统计)
        let totals = (
            ctx.conns.iter().map(|c| c.bytes_in).sum::<u64>(),
            ctx.conns.iter().map(|c| c.bytes_out).sum::<u64>(),
        );
        rate_card(ui, ctx.i18n, ctx.rates, ctx.rate_hist, totals);
        ui.add_space(theme::sp::MD);
        status_rows(ui, ctx.conns, ctx.i18n, collector_kind);
    });
}

/// 导航项:图标 + 文字按钮,选中态底色与左侧强调指示条(高度随过渡动画展开)
fn nav_item(ui: &mut egui::Ui, page: &mut Page, target: Page, label: &str, icon: &str) {
    let p = theme::c();
    let selected = *page == target;
    let t = ui.ctx().animate_value_with_time(
        egui::Id::new(("nav-item", target)),
        if selected { 1.0 } else { 0.0 },
        NAV_ANIM_SECS,
    );

    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), NAV_ITEM_H), Sense::click());
    let radius = CornerRadius::same(theme::RADIUS_MD);
    if selected {
        ui.painter().rect_filled(rect, radius, p.accent_soft);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, radius, p.hover_bg);
    }
    if t > 0.01 {
        let h = (NAV_ITEM_H - 14.0) * t;
        ui.painter().rect_filled(
            egui::Rect::from_center_size(
                egui::pos2(rect.left() + NAV_INDICATOR_W / 2.0, rect.center().y),
                egui::vec2(NAV_INDICATOR_W, h),
            ),
            CornerRadius::same(theme::RADIUS_PILL),
            p.accent,
        );
    }
    let text_color = if selected || resp.hovered() {
        p.text
    } else {
        p.text_dim
    };
    let center = rect.center();
    ui.painter().text(
        egui::pos2(rect.left() + 18.0, center.y),
        Align2::CENTER_CENTER,
        icon,
        FontId::proportional(theme::font::BODY),
        if selected { p.accent } else { text_color },
    );
    ui.painter().text(
        egui::pos2(rect.left() + 38.0, center.y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(theme::font::H3),
        text_color,
    );
    if resp.clicked() {
        *page = target;
    }
}

/// 底部速率卡:一分钟双色走势 + 当前速率 + 会话累计
fn rate_card(
    ui: &mut egui::Ui,
    i18n: &I18n,
    rates: (u64, u64),
    hist: &[(u64, u64)],
    totals: (u64, u64),
) {
    let p = theme::c();
    egui::Frame::new()
        .fill(p.bg_card)
        .stroke(Stroke::new(1.0, p.stroke))
        .corner_radius(CornerRadius::same(theme::RADIUS_MD))
        .inner_margin(Margin::same(theme::sp::MD as i8))
        .show(ui, |ui| {
            let down: Vec<u64> = hist.iter().map(|d| d.0).collect();
            let up: Vec<u64> = hist.iter().map(|d| d.1).collect();
            widgets::sparkline::sparklines(
                ui,
                &[(&down, p.inbound), (&up, p.outbound)],
                egui::vec2(ui.available_width(), 34.0),
            );
            ui.add_space(theme::sp::XS);
            rate_row(
                ui,
                i18n,
                "nav-rate-down",
                rates.0,
                p.inbound,
                icons::ARROW_DOWN,
            );
            ui.add_space(theme::sp::XS);
            rate_row(
                ui,
                i18n,
                "nav-rate-up",
                rates.1,
                p.outbound,
                icons::ARROW_UP,
            );
            ui.add_space(theme::sp::XS);
            session_row(ui, i18n, totals);
        });
}

/// 会话累计行:标签 + 双向字节小字
fn session_row(ui: &mut egui::Ui, i18n: &I18n, totals: (u64, u64)) {
    let p = theme::c();
    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 3.0;
        ui.label(theme::dim_text(
            &i18n.t("nav-session-total"),
            theme::font::XS,
        ));
        ui.label(
            RichText::new(icons::ARROW_DOWN)
                .size(theme::font::XS)
                .color(p.inbound),
        );
        ui.label(
            RichText::new(fmt_bytes(totals.0))
                .size(theme::font::XS)
                .color(p.text_dim),
        );
        ui.label(
            RichText::new(icons::ARROW_UP)
                .size(theme::font::XS)
                .color(p.outbound),
        );
        ui.label(
            RichText::new(fmt_bytes(totals.1))
                .size(theme::font::XS)
                .color(p.text_dim),
        );
    });
}

/// 一行速率:方向图标 + 标签 + 数值
fn rate_row(ui: &mut egui::Ui, i18n: &I18n, key: &str, rate: u64, color: Color32, icon: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).size(theme::font::XS).color(color));
        ui.label(theme::dim_text(&i18n.t(key), theme::font::SM));
        ui.label(
            RichText::new(format!("{}/s", fmt_bytes(rate)))
                .size(theme::font::SM)
                .color(color),
        );
    });
}

/// 底部状态:监控中(绿点)、连接数、数据源提示
fn status_rows(ui: &mut egui::Ui, conns: &[Connection], i18n: &I18n, kind: CollectorKind) {
    let p = theme::c();
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, p.status_ok);
        ui.label(theme::dim_text(
            &i18n.t("status-monitoring"),
            theme::font::SM,
        ));
    });
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(icons::ETHERNET)
                .size(theme::font::XS)
                .color(p.text_dim),
        );
        ui.label(theme::dim_text(
            &i18n.t_with_args("status-conn-count", &[("count", conns.len().to_string())]),
            theme::font::XS,
        ));
    });
    // 模拟数据源提示(真实采集时无独立状态行,避免与绿点行重复)
    if kind == CollectorKind::Mock {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(icons::FLASK)
                    .size(theme::font::XS)
                    .color(p.status_warn),
            );
            ui.label(theme::dim_text(&i18n.t("status-mock"), theme::font::XS));
        });
    }
}

/// 侧栏字体下文本宽度(居中偏移计算用;地图面板行宽计算共用)
pub(crate) fn text_width(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(size),
            egui::Color32::WHITE,
        )
        .rect
        .width()
}

/// 各页面共用的绘制上下文(集中可变状态引用,避免签名持续膨胀)
pub struct UiCtx<'a> {
    pub conns: &'a [Connection],
    pub i18n: &'a mut I18n,
    pub map_view: &'a mut basemap::View,
    pub config: &'a mut Config,
    pub rdns: &'a rdns::Rdns,
    /// 总速率(字节/秒):(下行, 上行)
    pub rates: (u64, u64),
    /// 总速率历史(时间正序,(下行, 上行),约 1 分钟窗口;迷你走势图用)
    pub rate_hist: &'a [(u64, u64)],
    /// 每连接实时速率(键 = 连接 id;ETW 字节差值/秒,未提权恒 0)
    pub conn_rates: &'a HashMap<u64, (u64, u64)>,
    /// 连接列表表头排序状态(表头点击切换)
    pub conn_sort: &'a mut ConnSortState,
    /// 连接页行悬停辅助(跨帧行高,行首垫底用)
    pub conn_row_hover: &'a mut widgets::table::RowHover,
    /// 规则页行悬停辅助(跨帧行高)
    pub rules_row_hover: &'a mut widgets::table::RowHover,
    /// 进程图标纹理(键 = 映像路径);None 表示已提取且无图标
    pub icon_tex: &'a HashMap<String, Option<egui::TextureHandle>>,
    /// Windows 默认"应用程序"图标纹理(无路径/提取失败进程的兜底)
    pub default_icon_tex: Option<&'a egui::TextureHandle>,
    /// 历史页状态
    pub history: &'a mut history_query::PageState,
    /// 历史查询只读连接
    pub history_db: &'a history_store::Db,
    /// 规则集(连接页求值与规则页编辑)
    pub rules: &'a mut rules_engine::RuleSet,
    /// 地图页左右面板与选中状态(端点点击联动)
    pub map_panels: &'a mut map_panel::MapPanelState,
    /// 规则页状态
    pub rules_page: &'a mut rules::PageState,
    /// 日志浏览窗口状态
    pub log_window: &'a mut log_window::PageState,
    /// 拦截引擎状态(WFP 管理线程回报)
    pub wfp_status: wfp::Status,
    /// 历史写线程句柄(手动清空)
    pub writer: &'a history_store::Writer,
    pub local_pos: (f32, f32),
}

use crate::rules as rules_engine;

/// 中央区域按页面分发;返回本轮是否直接改动了配置(由 App 层标脏落盘)
pub fn central_ui(ui: &mut egui::Ui, page: &Page, ctx: &mut UiCtx) -> bool {
    match page {
        Page::Map => {
            map_header(ui, ctx);
            ui.add_space(6.0);
            let click = map::draw(
                ui,
                ctx.conns,
                ctx.i18n,
                ctx.map_view,
                ctx.rdns,
                ctx.icon_tex,
                ctx.default_icon_tex,
                ctx.local_pos,
                ctx.map_panels.place,
            );
            match click {
                Some(map::MapClick::Place(place)) => {
                    ctx.map_panels.place = Some(place);
                    ctx.map_panels.process = None;
                }
                Some(map::MapClick::Background) => {
                    ctx.map_panels.place = None;
                    ctx.map_panels.process = None;
                }
                None => {}
            }
            false
        }
        Page::Connections => connections::connections_ui(ui, ctx),
        Page::History => history::show(
            ui,
            ctx.history,
            ctx.i18n,
            ctx.icon_tex,
            ctx.default_icon_tex,
            ctx.history_db,
            ctx.writer,
            ctx.config,
        ),
        Page::Rules => {
            rules::show(
                ui,
                ctx.rules_page,
                ctx.i18n,
                ctx.history_db,
                ctx.rules,
                &ctx.wfp_status,
                ctx.rules_row_hover,
            );
            false
        }
        Page::Settings => settings::settings_ui(ui, ctx.config, ctx.i18n, ctx.log_window),
    }
}

/// 地图页标题行:标题、副标题、图例与左右面板开关(开关在图例之后,
/// 从右往左排布)
fn map_header(ui: &mut egui::Ui, ctx: &mut UiCtx) {
    widgets::header::page_header_row(
        ui,
        &ctx.i18n.t("map-title"),
        &ctx.i18n.t("map-subtitle"),
        |ui| {
            let panels = &mut *ctx.map_panels;
            panel_toggle(
                ui,
                &mut panels.show_right,
                icons::LAYOUT_TEXT_SIDEBAR_REVERSE,
                &ctx.i18n.t("map-panel-toggle-inspector"),
            );
            panel_toggle(
                ui,
                &mut panels.show_left,
                icons::LAYOUT_SIDEBAR,
                &ctx.i18n.t("map-panel-toggle-list"),
            );
            ui.add_space(theme::sp::MD);
            legend(ui, theme::c().outbound, &ctx.i18n.t("map-legend-out"));
            ui.add_space(theme::sp::SM);
            legend(ui, theme::c().inbound, &ctx.i18n.t("map-legend-in"));
        },
    );
}

/// 面板开关小按钮(图标高亮 = 面板显示)
fn panel_toggle(ui: &mut egui::Ui, on: &mut bool, glyph: &str, tip: &str) {
    let text = RichText::new(glyph).size(theme::font::H3).color(if *on {
        theme::c().accent
    } else {
        theme::c().text_dim
    });
    let resp = ui
        .add(
            Button::new(text)
                .frame(false)
                .min_size(egui::vec2(24.0, 24.0)),
        )
        .on_hover_text(tip);
    if resp.clicked() {
        *on = !*on;
    }
}

/// 图例:语义色圆点 + 文字
fn legend(ui: &mut egui::Ui, color: Color32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
    ui.label(theme::dim_text(label, theme::font::SM));
}

/// 连接显示过滤:本地/局域网远端噪音(config 持久化;连接列表与
/// 地图页左右面板共用同一口径)
pub(crate) fn conn_visible(config: &Config, c: &Connection) -> bool {
    !(config.general.hide_local && c.remote_ip.is_loopback())
        && !(config.general.hide_lan && c.remote_ip.is_private())
}
