//! 主窗口布局:导航侧栏与各页面(地图 / 连接 / 历史 / 规则 / 设置)。
//! 全部界面文本经 I18n 词条获取(AGENTS.md 规范 4)。

pub mod ask;
pub mod floating_ball;
pub mod history;
pub mod icons;
pub mod log_window;
pub mod map_conn;
pub mod map_inspector;
pub mod map_panel;
pub mod map_widgets;
pub mod nav;
pub mod profile_manager;
pub mod rules;
pub mod theme;
pub mod titlebar;
pub mod toast;
pub mod widgets;

mod connections;
mod lan;
mod settings;

use std::collections::HashMap;

use eframe::egui;
use egui::{Button, Color32, RichText, Sense};

use crate::i18n::I18n;
use crate::map;
use crate::map::basemap;
use crate::model::Connection;
use crate::net::rdns;
use crate::rules::wfp;
use crate::storage::config::Config;
use crate::storage::history as history_store;
use crate::storage::history_query;

pub use nav::nav_ui;

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

/// 连接页视图形态:活动连接 / 端口监听
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConnView {
    Conns,
    Listens,
}

/// 主窗口页面
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Page {
    Map,
    Connections,
    History,
    Lan,
    Rules,
    Settings,
}

/// 工具栏行内控件最小交互高度:egui horizontal 行高从默认 18px 起步,
/// 混排高低控件会基线错位;统一抬高让整行垂直居中(历史/规则/日志工具栏共用)
pub(crate) const TOOLBAR_ROW_H: f32 = 26.0;

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
    /// 连接页视图形态(连接/端口监听 segmented 切换)
    pub conn_view: &'a mut ConnView,
    /// 监听条目快照(TCP LISTEN + UDP 绑定,与连接采集同频)
    pub listens: &'a [crate::model::ListenEntry],
    /// 连接页搜索词(进程/远端/域名包含过滤,会话态)
    pub conn_search: &'a mut String,
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
    /// 是否以管理员令牌运行(右键"结束连接"等能力判定)
    pub elevated: bool,
    /// 历史写线程句柄(手动清空)
    pub writer: &'a history_store::Writer,
    /// 局域网设备视图(ARP 发现,last_seen 降序)
    pub lan_devices: &'a [crate::net::lan::DeviceRow],
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
            ctx.conns,
            ctx.history_db,
            ctx.writer,
            ctx.config,
            ctx.elevated,
        ),
        Page::Lan => lan::lan_ui(ui, ctx),
        Page::Rules => rules::show(
            ui,
            ctx.rules_page,
            ctx.i18n,
            ctx.history_db,
            ctx.config,
            ctx.rules,
            &ctx.wfp_status,
            ctx.rules_row_hover,
        ),
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
    // frame(false) 无系统悬停底色,自绘垫底(绘制顺序在后,悬停帧可见)
    if resp.hovered() {
        ui.painter()
            .rect_filled(resp.rect, theme::RADIUS_SM, theme::c().hover_bg);
    }
    if resp.clicked() {
        *on = !*on;
    }
}

/// 图例:语义色圆点 + 文字
fn legend(ui: &mut egui::Ui, color: Color32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
    ui.label(theme::dim_text(label, theme::font::SM));
}

/// 连接显示过滤:本地/局域网远端噪音(config 持久化;连接列表与
/// 地图页左右面板共用同一口径)
pub(crate) fn conn_visible(config: &Config, c: &Connection) -> bool {
    !(config.general.hide_local && c.remote_ip.is_loopback())
        && !(config.general.hide_lan && c.remote_ip.is_private())
}
