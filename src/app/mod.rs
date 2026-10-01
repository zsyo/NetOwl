//! 应用编排:托盘事件、采集 tick、页面切换、配置持久化(窗口几何/语言)、
//! 关闭到托盘与退出。
//!
//! eframe 0.36 的 App trait 拆分为 logic(每帧逻辑,窗口隐藏时仍会被调用)
//! 与 ui(绘制)。托盘命令、几何捕获与配置写盘节流放在 logic。

pub mod ask;

use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;

use self::ask::{Asker, Decision, Scope, temp_rule_holds};
use crate::collector::{self, Collector, CollectorKind};
use crate::i18n::I18n;
use crate::logging;
use crate::map::basemap;
use crate::map::world;
use crate::model::{Connection, Place, Protocol, Signing};
use crate::net::etw;
use crate::net::geoip;
use crate::net::local_ip;
use crate::net::rdns;
use crate::net::traffic;
use crate::platform::resize::{self, DragResize};
use crate::platform::shutdown_hook;
use crate::platform::single_instance;
use crate::platform::tray::{self, Tray};
use crate::rules;
use crate::rules::wfp;
use crate::storage::config::Config;
use crate::storage::history;
use crate::storage::history_query;
use crate::ui::ask as ui_ask;
use crate::ui::rules as ui_rules;
use crate::ui::theme;
use crate::ui::{self, Page};

/// 重绘节奏:地图动画 30fps;连接页速率与历史页反查 500ms;其余静态页 1s;
/// 窗口隐藏(托盘)时一律 500ms(动画不可见,不再高帧率空转)
const REPAINT_ANIMATED: Duration = Duration::from_millis(33);
const REPAINT_IDLE: Duration = Duration::from_millis(500);
const REPAINT_STATIC: Duration = Duration::from_millis(1000);
/// 配置写盘防抖:合并连续变更(窗口拖动/缩放每帧都在变)
const CONFIG_SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
/// 窗口几何恢复完成判定:超时放弃匹配(避免命令未生效时永久跳过捕获)
const RESTORE_TIMEOUT: Duration = Duration::from_secs(2);
/// 恢复匹配容差(物理像素)
const RESTORE_TOLERANCE: i32 = 2;
/// 本机公网 IP 重探间隔(重拨/换网后点位跟随更新)
const LOCAL_IP_PROBE_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// 总速率采样间隔:窗口可见时 1s,隐藏(托盘)时放宽到 5s 降低功耗
const TRAFFIC_INTERVAL_ACTIVE: Duration = Duration::from_secs(1);
const TRAFFIC_INTERVAL_HIDDEN: Duration = Duration::from_secs(5);
/// 速率历史采样点数(约 1 分钟窗口,迷你走势图用)
const RATE_HIST_LEN: usize = 60;
/// 主窗口最小逻辑尺寸(= 默认窗口尺寸;main.rs 视口 min_inner_size 与
/// 无边框缩放钳制同源)
pub const MIN_WINDOW_SIZE: (f32, f32) = (1440.0, 800.0);
/// WFP 过滤器目标集合同步间隔(与采集同频:进程路径出现/消失的生效延迟上限)
const WFP_SYNC_INTERVAL: Duration = Duration::from_secs(1);
/// ETW 流量事件合并间隔(与表快照采集同频)
const ETW_POLL_INTERVAL: Duration = Duration::from_secs(1);
/// 连接快照与派生数据(rDNS/图标/历史/速率/询问)刷新间隔:与表快照采集
/// 同频;地图动画 30fps 的帧内只做绘制,O(连接数) 的逻辑不逐帧执行
const CONNS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
/// 托盘常驻写入重试间隔:托盘设置项由 Explorer 在图标注册时创建,
/// 启动数秒内可能尚不存在
const TRAY_PIN_RETRY_INTERVAL: Duration = Duration::from_secs(60);

/// 待恢复的窗口几何(物理像素)
type WindowRect = (i32, i32, i32, i32, bool);

pub struct NetOwlApp {
    page: Page,
    /// 上次所在页面:用于检测进入设置页时重扫 locales 新增语言
    last_page: Page,
    /// 流量地图视图(中心/缩放,跨帧保持)
    map_view: basemap::View,
    collector: Box<dyn Collector>,
    /// rDNS 解析(异步 PTR 查询,连接列表/地图信息卡域名优先显示)
    rdns: rdns::Rdns,
    /// 总上传/下载速率采样(GetIfTable2 接口字节差值)
    traffic: traffic::Sampler,
    /// 最近总速率(字节/秒):(下行, 上行),导航栏展示
    rates: (u64, u64),
    /// 总速率历史环形缓冲(时间正序,(下行, 上行));导航栏迷你走势图数据源
    rate_hist: Vec<(u64, u64)>,
    /// 应用图标纹理(导航栏品牌区);启动时从内嵌 PNG 建一次
    logo_tex: egui::TextureHandle,
    /// Windows 默认"应用程序"图标纹理(无路径/提取失败进程的兜底);
    /// None = 尚未提取成功,每轮 poll_icons 重试
    default_icon_tex: Option<egui::TextureHandle>,
    /// 进程图标纹理(键 = 映像路径);None 表示已提取且无图标
    icon_tex: HashMap<String, Option<egui::TextureHandle>>,
    /// 连接历史写线程(批量落盘 conn_events)
    writer: history::Writer,
    /// 连接快照对比器:跟踪活跃连接,消失时生成完结事件
    tracker: history::Tracker,
    /// 历史页状态与查询只读连接
    history: history_query::PageState,
    history_db: history::Db,
    /// 规则集(内存 + SQLite 同步,规则页编辑与连接页求值共用)
    rules: rules::RuleSet,
    rules_page: ui_rules::PageState,
    /// 地图页左右面板与选中状态(端点点击联动,会话态)
    map_panels: ui::map_panel::MapPanelState,
    /// WFP 拦截引擎(启用规则翻译为过滤器,管理线程持有动态会话)
    wfp: wfp::Manager,
    wfp_sync_at: Instant,
    /// ETW 流量事件采集(提权时启动:连接字节填充与短命连接收割)
    etw: Option<etw::Etw>,
    etw_poll_at: Instant,
    /// 连接快照上次刷新时刻(1s 节流,高帧率帧内跳过 O(连接数) 逻辑)
    conns_refresh_at: Instant,
    /// 曾被表快照合并覆盖的 ETW 流键:完结流命中此集合说明表快照
    /// 跟踪器已记录,不按短命连接重复落盘
    etw_seen: HashSet<etw::FlowKey>,
    /// 每连接实时速率(键 = conn.id,ETW 字节差值/秒)
    conn_rates: HashMap<u64, (u64, u64)>,
    /// UDP 行最近已知远端缓存:表快照无远端,活跃流回填仅在流活跃的轮次
    /// 生效,流空闲收割后行会回落 *:*;缓存 socket 最后通信的远端,
    /// socket 存活期间持续展示(键 = (pid, 本地端口),表行消失时清理)
    udp_last_remote: HashMap<(u32, u16), (Ipv4Addr, u16)>,
    /// 连接列表表头排序状态(会话内,不持久化)
    conn_sort: ui::ConnSortState,
    /// 表格行悬停辅助(连接/规则页,跨帧行高供行首垫底判定)
    conn_row_hover: ui::widgets::table::RowHover,
    rules_row_hover: ui::widgets::table::RowHover,
    /// 日志浏览窗口状态(内存层日志展示,参照 wallwarp)
    log_window: ui::log_window::PageState,
    /// 上一轮 ETW 字节快照(速率差值基准,键 = conn.id)
    conn_prev_bytes: HashMap<u64, (u64, u64)>,
    /// 新连接询问(Little Snitch 式弹窗)
    asker: Asker,
    /// 本机公网 IP 探测(公共接口并发,最先成功者胜出)
    local_probe: local_ip::Probe,
    /// 本机公网 IP 的归属定位键;探测失败/未收录时为 None(地图用默认点位)
    local_place: Option<Place>,
    local_probe_at: Instant,
    i18n: I18n,
    config: Config,
    conns: Vec<Connection>,
    tray_rx: Receiver<String>,
    should_exit: bool,
    /// 主窗口可见性(自行跟踪):egui 0.36 的 viewport().visible() 恒为 None
    /// 不可依赖;全部可见性变更路径(托盘命令/关闭按钮/单实例唤出)都必须同步此字段
    window_visible: bool,
    /// 主窗口 HWND(启动时按标题缓存,0 = 未找到);用于感知外部 ShowWindow
    main_hwnd: isize,
    /// 无边框窗口拖拽缩放状态(winit 对 undecorated 窗口无边缘 hit-test)
    window_resize: DragResize,
    /// 首帧窗口几何恢复目标;发送命令后转为 restore_active 等待生效
    pending_restore: Option<WindowRect>,
    /// 恢复命令已发送,几何生效前跳过捕获(防止默认位置覆盖配置)
    restore_active: bool,
    restore_started: Instant,
    config_dirty: bool,
    config_dirty_since: Instant,
    /// 已写入注册表的托盘常驻状态(None = 尚未成功达成目标态)
    tray_pinned_applied: Option<bool>,
    /// 托盘常驻下次重试时刻(写入失败后定时重试)
    tray_pin_retry_at: Instant,
    /// 托盘句柄保活,drop 时移除托盘图标
    _tray: Tray,
}

impl NetOwlApp {
    pub fn new(cc: &eframe::CreationContext<'_>, i18n: I18n, config: Config) -> Self {
        theme::install(&cc.egui_ctx, &config.general.theme);
        let (_tray, tray_rx) = tray::create(cc.egui_ctx.clone());
        let pending_restore = config.window_position();
        let pending_restore =
            pending_restore.map(|(x, y, w, h)| (x, y, w, h, config.window.maximized));
        let history_db = crate::storage::db::open();
        let rules = rules::RuleSet::load(&history_db);
        // ETW 流量事件仅在提权进程内可用;失败只记录,字节列退化为 0
        let etw = if wfp::is_elevated() {
            match etw::Etw::start() {
                Ok(e) => {
                    tracing::info!("[ETW] 流量事件采集会话已启动");
                    Some(e)
                }
                Err(e) => {
                    tracing::warn!("[ETW] {e}");
                    None
                }
            }
        } else {
            None
        };
        // 导航栏品牌区图标:内嵌 PNG 一次性建纹理(256x256 RGBA)
        let icon_data = crate::platform::icon::window_icon();
        let logo_tex = cc.egui_ctx.load_texture(
            "app-logo",
            egui::ColorImage::from_rgba_unmultiplied(
                [icon_data.width as usize, icon_data.height as usize],
                &icon_data.rgba,
            ),
            egui::TextureOptions::LINEAR,
        );
        NetOwlApp {
            page: Page::Map,
            last_page: Page::Map,
            map_view: basemap::View::global(),
            collector: collector::build(CollectorKind::from_config(&config.general.collector)),
            rdns: rdns::Rdns::new(),
            traffic: traffic::Sampler::new(),
            rates: (0, 0),
            rate_hist: Vec::with_capacity(RATE_HIST_LEN + 1),
            logo_tex,
            default_icon_tex: None,
            icon_tex: HashMap::new(),
            writer: history::Writer::spawn(config.general.history_days),
            tracker: history::Tracker::new(),
            history: history_query::PageState::new(),
            history_db,
            rules,
            rules_page: ui_rules::PageState::new(),
            map_panels: ui::map_panel::MapPanelState::default(),
            wfp: wfp::Manager::spawn(),
            wfp_sync_at: Instant::now(),
            etw,
            etw_poll_at: Instant::now(),
            // 首帧立即拉取快照:把起点回拨一个周期
            conns_refresh_at: Instant::now() - CONNS_REFRESH_INTERVAL,
            etw_seen: HashSet::new(),
            conn_rates: HashMap::new(),
            udp_last_remote: HashMap::new(),
            conn_sort: None,
            conn_row_hover: Default::default(),
            rules_row_hover: Default::default(),
            log_window: ui::log_window::PageState::new(),
            conn_prev_bytes: HashMap::new(),
            asker: Asker::new(),
            local_probe: local_ip::Probe::new(),
            local_place: None,
            local_probe_at: Instant::now(),
            i18n,
            config,
            conns: Vec::new(),
            tray_rx,
            should_exit: false,
            window_visible: true,
            main_hwnd: single_instance::main_hwnd(crate::APP_NAME),
            window_resize: DragResize::default(),
            pending_restore,
            restore_active: false,
            restore_started: Instant::now(),
            config_dirty: false,
            config_dirty_since: Instant::now(),
            tray_pinned_applied: None,
            tray_pin_retry_at: Instant::now(),
            _tray,
        }
    }

    fn handle_tray_commands(&mut self, ctx: &egui::Context) {
        for cmd in tray::drain(&self.tray_rx) {
            match cmd.as_str() {
                tray::CMD_SHOW => {
                    self.window_visible = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                tray::CMD_HIDE => {
                    self.window_visible = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
                tray::CMD_QUIT => {
                    // 退出收尾:仍活跃的连接补写为已完结行,等待写线程清空队列,
                    // 再把待写配置立即落盘
                    let events = self.tracker.flush(history::unix_now());
                    self.writer.send(events);
                    self.writer.shutdown();
                    if let Some(e) = self.etw.as_mut() {
                        e.shutdown();
                    }
                    logging::flush();
                    self.config.save_to_file();
                    self.config_dirty = false;
                    self.should_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                _ => {}
            }
        }
    }

    /// 托盘图标常驻:配置开关变化时写注册表 IsPromoted;失败(托盘项未注册、
    /// 系统不支持)静默保持系统默认行为并定时重试,直到达成目标态
    fn sync_tray_pinned(&mut self) {
        let want = self.config.general.tray_pinned;
        if self.tray_pinned_applied == Some(want) || Instant::now() < self.tray_pin_retry_at {
            return;
        }
        if tray::set_pinned(want) {
            self.tray_pinned_applied = Some(want);
        } else {
            self.tray_pin_retry_at = Instant::now() + TRAY_PIN_RETRY_INTERVAL;
        }
    }

    /// 首帧修正窗口几何:创建时 with_position 用物理坐标当逻辑值,主屏 DPI 为 100%
    /// 时已精确;其他 DPI 下 winit 会按主屏 scale 放大产生偏差,这里按当前
    /// pixels_per_point 反推逻辑值重新下发,egui 命令路径乘回同一 ppp 后物理精确。
    /// 若创建位置已正确,这些命令为无操作。
    fn restore_window_geometry(&mut self, ctx: &egui::Context) {
        let Some((x, y, w, h, maximized)) = self.pending_restore.take() else {
            return;
        };
        let ppp = ctx.pixels_per_point();
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
            x as f32 / ppp,
            y as f32 / ppp,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            w as f32 / ppp,
            h as f32 / ppp,
        )));
        if maximized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        self.restore_active = true;
        self.restore_started = Instant::now();
    }

    /// 捕获窗口几何(物理像素)写入配置;恢复生效期间跳过,防默认位置覆盖。
    fn capture_window_geometry(&mut self, ctx: &egui::Context) {
        if self.restore_active {
            let target = self
                .config
                .window_position()
                .map(|(x, y, w, h)| (x, y, w, h, self.config.window.maximized));
            let ppp = ctx.pixels_per_point();
            let (outer, inner, maximized) = ctx.input(|i| {
                let v = i.viewport();
                (v.outer_rect, v.inner_rect, v.maximized)
            });
            let settled = match (target, outer, inner) {
                (Some((tx, ty, tw, th, tmax)), Some(outer), Some(inner)) => {
                    if tmax {
                        maximized == Some(true)
                    } else {
                        (outer.min.x * ppp).round() as i32 - tx <= RESTORE_TOLERANCE
                            && (outer.min.y * ppp).round() as i32 - ty <= RESTORE_TOLERANCE
                            && (inner.width() * ppp).round() as i32 - tw <= RESTORE_TOLERANCE
                            && (inner.height() * ppp).round() as i32 - th <= RESTORE_TOLERANCE
                    }
                }
                _ => false,
            };
            if settled || self.restore_started.elapsed() > RESTORE_TIMEOUT {
                self.restore_active = false;
            } else {
                return;
            }
        }

        let minimized = ctx.input(|i| i.viewport().minimized);
        if minimized == Some(true) {
            return;
        }
        let ppp = ctx.pixels_per_point();
        let (outer, inner, maximized) = ctx.input(|i| {
            let v = i.viewport();
            (v.outer_rect, v.inner_rect, v.maximized)
        });
        if let (Some(outer), Some(inner)) = (outer, inner) {
            let changed = self.config.set_window(
                (outer.min.x * ppp).round() as i32,
                (outer.min.y * ppp).round() as i32,
                (inner.width() * ppp).round() as i32,
                (inner.height() * ppp).round() as i32,
                maximized == Some(true),
            );
            if changed {
                self.mark_config_dirty();
            }
        }
    }

    /// 界面语言与配置不一致(设置页切换)时同步进配置
    fn sync_language_to_config(&mut self) {
        if self.i18n.current_lang != self.config.general.language {
            self.config.set_language(self.i18n.current_lang.clone());
            self.mark_config_dirty();
        }
    }

    /// 界面主题与配置不一致(设置页切换)时同步进配置
    fn sync_theme_to_config(&mut self) {
        let current = theme::theme_str();
        if current != self.config.general.theme {
            self.config.set_theme(current.to_owned());
            self.mark_config_dirty();
        }
    }

    /// 数据源配置与当前实例不一致(设置页切换)时重建采集器,切换即时生效
    fn ensure_collector(&mut self) {
        let kind = CollectorKind::from_config(&self.config.general.collector);
        if kind != self.collector.kind() {
            self.collector = collector::build(kind);
            self.mark_config_dirty();
        }
    }

    /// 连接数据刷新(1s 节流):快照拉取与其全部派生逻辑。
    /// 高帧率重绘(地图动画)帧内直接跳过,连接数据本就是秒级口径。
    /// ETW 合并(UDP 远端回填)先于 rDNS 派发与 Tracker diff:
    /// 前者让 UDP 远端可查 PTR,后者让完结落库带真实远端;
    /// 远端未知行的排除也必须夹在两者之间:早于回填会把待回填的
    /// 活跃 UDP 行一并丢弃,晚于落库则 0.0.0.0:0 已写入历史
    fn poll_conns(&mut self, ctx: &egui::Context) {
        if self.conns_refresh_at.elapsed() < CONNS_REFRESH_INTERVAL {
            return;
        }
        self.conns_refresh_at = Instant::now();
        self.conns = self.collector.snapshot();
        self.poll_etw();
        // UDP 表行无远端且 ETW 合并后仍无回退值(未提权/从未通信/启动前
        // 已存在),无归属无流量,只余噪音;列表/地图/历史一并排除
        self.conns
            .retain(|c| !(c.proto == Protocol::Udp && c.remote_ip.is_unspecified()));
        // UDP 表对同一 socket 的多个绑定地址各返回一行(同 pid+本地端口,
        // 多网卡机器 NetBIOS 类服务可达 5+ 行);ETW 合并键不含本机地址,
        // 这些行注定共享同一份回填数据,按 (pid, 本地端口) 去重留一行
        let mut seen_udp = HashSet::new();
        self.conns
            .retain(|c| c.proto != Protocol::Udp || seen_udp.insert((c.pid, c.local_port)));
        self.rdns.update(&self.conns);
        self.poll_icons(ctx);
        self.poll_history();
        self.poll_ask();
        self.poll_temp_rules();
    }

    /// 本机公网 IP 探测:取每轮首个成功结果,归属变化时刷新地图本机点位
    fn poll_local_ip(&mut self) {
        if let Some((ip, source)) = self.local_probe.poll() {
            let place = geoip::locate(ip).map(Place::Geo);
            if place != self.local_place {
                self.local_place = place;
                if place.is_some() {
                    tracing::info!("[LocalIp] 本机公网 IP {ip}({source})");
                } else {
                    tracing::info!("[LocalIp] 本机公网 IP {ip}({source}) 无归属,回退默认点位");
                }
            }
        }
        if self.local_probe_at.elapsed() >= LOCAL_IP_PROBE_INTERVAL {
            self.local_probe_at = Instant::now();
            self.local_probe.begin_round();
        }
    }

    /// 总速率采样:窗口隐藏(托盘)或最小化时放宽采样间隔降低功耗。
    /// 仅在真实采样(节流间隔到达且读表成功)时更新当前值并推进历史序列:
    /// logic 每帧执行,无条件 push 会让走势图随帧率滚动(地图动画 30fps
    /// 时 60 点缓冲两秒滚完,数据相同画成横线)
    fn poll_traffic(&mut self, ctx: &egui::Context) {
        let interval = if self.is_shown(ctx) {
            TRAFFIC_INTERVAL_ACTIVE
        } else {
            TRAFFIC_INTERVAL_HIDDEN
        };
        if let Some(rates) = self.traffic.poll(interval) {
            self.rates = rates;
            self.rate_hist.push(rates);
            if self.rate_hist.len() > RATE_HIST_LEN {
                self.rate_hist.remove(0);
            }
        }
    }

    /// 窗口是否对用户可见:自行跟踪的可见性 && 未最小化
    /// (最小化状态由 egui 填充,可信)
    fn is_shown(&self, ctx: &egui::Context) -> bool {
        let minimized = ctx.input(|i| i.viewport().minimized) == Some(true);
        self.window_visible && !minimized
    }

    /// 校准可见性跟踪:单实例二次启动经系统 API 直接 ShowWindow 唤出主窗口,
    /// 不经过 app 命令路径,以 IsWindowVisible 为准纠正(隐藏/显示命令自身的
    /// 发送路径窗口状态一致,校准为无操作)
    fn calibrate_window_visible(&mut self) {
        let visible = single_instance::is_window_visible(self.main_hwnd);
        if visible != self.window_visible {
            self.window_visible = visible;
        }
    }

    /// 进程图标:活跃连接的映像路径逐个请求采集器,到位即建纹理缓存。
    /// 纹理键 = 路径,同进程连接共享;提取失败缓存 None 不再重复请求
    fn poll_icons(&mut self, ctx: &egui::Context) {
        // 默认"应用程序"图标兜底:无路径(服务进程反查受限)或提取失败的
        // 进程统一显示;SHGFI_USEFILEATTRIBUTES 纯注册表查询,同步可接受
        if self.default_icon_tex.is_none() {
            self.default_icon_tex = collector::default_app_icon().map(|i| {
                ctx.load_texture(
                    "icon:default-app",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [i.width as usize, i.height as usize],
                        &i.rgba,
                    ),
                    egui::TextureOptions::LINEAR,
                )
            });
        }
        let keys: Vec<String> = self
            .conns
            .iter()
            .filter_map(|c| c.proc_path.clone())
            .filter(|p| !self.icon_tex.contains_key(p))
            .collect();
        for path in keys {
            if let collector::IconState::Ready(img) = self.collector.icon_image(&path) {
                let tex = img.map(|i| {
                    ctx.load_texture(
                        format!("icon:{path}"),
                        egui::ColorImage::from_rgba_unmultiplied(
                            [i.width as usize, i.height as usize],
                            &i.rgba,
                        ),
                        egui::TextureOptions::LINEAR,
                    )
                });
                self.icon_tex.insert(path, tex);
            }
        }
    }

    /// 连接历史:diff 前后快照生成完结事件交写线程;mock 数据不入库
    fn poll_history(&mut self) {
        let real = self.collector.kind() == CollectorKind::Real;
        let events = self.tracker.diff(real, &self.conns, history::unix_now());
        self.writer.send(events);
    }

    /// WFP 过滤器同步:与采集同频重建目标集合(进程路径粘滞展开,
    /// 见 RuleSet::wfp_specs);mock 数据源下清空过滤器,拦截只针对
    /// 真实连接。询问中的连接追加最高 weight 的临时阻断(安全默认)
    fn poll_wfp(&mut self) {
        if self.wfp_sync_at.elapsed() < WFP_SYNC_INTERVAL {
            return;
        }
        self.wfp_sync_at = Instant::now();
        let mut specs = if self.collector.kind() == CollectorKind::Real {
            self.rules.wfp_specs(&self.conns)
        } else {
            Vec::new()
        };
        if let Some(item) = self.asker.active.as_ref() {
            specs.insert(0, item.pending_block_spec());
        }
        self.wfp.sync(Arc::new(specs));
    }

    /// 新连接询问:开关开启时检测未命中规则的公网连接并入队;
    /// 倒计时超时执行默认动作(拒绝·仅本次)
    fn poll_ask(&mut self) {
        if self.collector.kind() != CollectorKind::Real || !self.config.general.ask_connections {
            self.asker.clear();
            return;
        }
        self.asker.update(&self.conns, &self.rules, &self.rdns);
        self.asker.poll();
        if self.asker.expired() {
            self.apply_decision(Decision {
                allow: false,
                scope: Scope::Once,
            });
        }
    }

    /// 应用询问决策:永久选项落库;拒绝·仅本次写入内存临时规则;
    /// 允许·仅本次不产生规则(询问的连接未命中任何规则,默认即放行);
    /// 仅本次决策后解除身份去重标记,同目标新连接重新询问
    fn apply_decision(&mut self, d: Decision) {
        let Some(item) = self.asker.take() else {
            return;
        };
        if d.scope == Scope::Once {
            self.asker.unask(&item);
            if d.allow {
                return;
            }
        }
        let action = if d.allow {
            crate::rules::Action::Allow
        } else {
            crate::rules::Action::Block
        };
        let rule = item.to_rule(action);
        match d.scope {
            Scope::Once => self.rules.insert_temp(rule),
            Scope::Target | Scope::Process => {
                let _ = self.rules.insert(&self.history_db, rule);
            }
        }
    }

    /// 临时规则生命周期:仅绑定决策时的那条连接(含本地端口的四元组
    /// 精确匹配),快照中不再有匹配连接时删除;同目标重连不会撞上旧
    /// 决策,而是重新弹窗询问
    fn poll_temp_rules(&mut self) {
        let expired: Vec<i64> = self
            .rules
            .rules
            .iter()
            .filter(|r| r.id < 0)
            .filter(|r| !self.conns.iter().any(|c| temp_rule_holds(r, c)))
            .map(|r| r.id)
            .collect();
        for id in expired {
            self.rules.delete_temp(id);
        }
    }

    /// ETW 流量事件合并(与采集同频):
    /// 1) 活跃流的收发字节填充到连接快照:TCP 按精确键(PID+协议+本地端口+
    ///    远端,不含本地 IP —— Connection 未暴露该字段);UDP 表行无远端
    ///    (系统 UDP 表不含对端),按 PID+本地端口归并,行字节为该端口全部
    ///    远端流之和,远端回填最近活跃流的端点(见 merge_udp_groups),归属
    ///    就地重算,下游(显示/归属/域名/规则/询问/过滤/地图)随之生效;
    /// 2) 完结流若从未被表快照覆盖(存活短于采样间隙的短命连接)则
    ///    生成历史事件落盘;曾被覆盖的由 Tracker 正常处理,跳过。
    fn poll_etw(&mut self) {
        let Some(etw) = self.etw.as_ref() else {
            return;
        };
        if self.etw_poll_at.elapsed() < ETW_POLL_INTERVAL {
            return;
        }
        let dt = self.etw_poll_at.elapsed().as_secs_f32();
        self.etw_poll_at = Instant::now();
        let real = self.collector.kind() == CollectorKind::Real;

        let flows = etw.snapshot();
        let finished = etw.take_finished();
        let by_key: HashMap<(u32, Protocol, u16, Ipv4Addr, u16), &etw::FlowAgg> = flows
            .iter()
            .filter(|f| f.key.proto == Protocol::Tcp)
            .map(|f| (flow_merge_key(&f.key), f))
            .collect();
        let udp_groups = merge_udp_groups(&flows);
        let mut hits: Vec<etw::FlowKey> = Vec::new();
        if real {
            for c in &mut self.conns {
                match c.proto {
                    Protocol::Tcp => {
                        let Some(f) =
                            by_key.get(&(c.pid, c.proto, c.local_port, c.remote_ip, c.remote_port))
                        else {
                            continue;
                        };
                        c.bytes_in = f.down_bytes;
                        c.bytes_out = f.up_bytes;
                        hits.push(f.key);
                    }
                    Protocol::Udp => {
                        match udp_groups.get(&(c.pid, c.local_port)) {
                            Some(g) => {
                                if c.remote_ip != g.rep.key.remote_ip
                                    || c.remote_port != g.rep.key.remote_port
                                {
                                    c.remote_ip = g.rep.key.remote_ip;
                                    c.remote_port = g.rep.key.remote_port;
                                    c.city = geoip::locate(c.remote_ip).map(Place::Geo);
                                }
                                c.bytes_in = g.bytes.0;
                                c.bytes_out = g.bytes.1;
                                // 记录最后通信的远端,流收割后 socket 行仍可展示
                                self.udp_last_remote
                                    .insert((c.pid, c.local_port), (c.remote_ip, c.remote_port));
                                hits.extend(g.keys.iter().copied());
                            }
                            None => {
                                // 无活跃流:回退最近已知远端(此前通信过的 socket)
                                if let Some((ip, port)) =
                                    self.udp_last_remote.get(&(c.pid, c.local_port))
                                    && (c.remote_ip != *ip || c.remote_port != *port)
                                {
                                    c.remote_ip = *ip;
                                    c.remote_port = *port;
                                    c.city = geoip::locate(c.remote_ip).map(Place::Geo);
                                }
                            }
                        }
                    }
                }
            }
        }
        self.etw_seen.extend(hits);
        drop(by_key);
        // 缓存清理:socket 关闭(UDP 表行消失)后释放对应项
        let live_udp: HashSet<(u32, u16)> = self
            .conns
            .iter()
            .filter(|c| c.proto == Protocol::Udp)
            .map(|c| (c.pid, c.local_port))
            .collect();
        self.udp_last_remote.retain(|k, _| live_udp.contains(k));
        self.update_conn_rates(dt);

        let mut events = Vec::new();
        for f in finished {
            if !real || self.etw_seen.contains(&f.key) {
                continue;
            }
            // 回环短命连接(本机内部通信)高频出现且无监控价值,不入库;
            // 存活超采样间隙的由表快照 Tracker 按既有口径处理
            if f.key.local_ip.is_loopback() || f.key.remote_ip.is_loopback() {
                continue;
            }
            events.push(short_lived_event(&f));
        }
        if !events.is_empty() {
            self.writer.send(events);
        }
    }

    /// 每连接实时速率:相邻两轮 ETW 字节快照差值 / 间隔秒数;
    /// 快照里消失的连接连带清理(差值基准与速率表同步收缩)
    fn update_conn_rates(&mut self, dt: f32) {
        let prev = std::mem::take(&mut self.conn_prev_bytes);
        let mut cur: HashMap<u64, (u64, u64)> = HashMap::with_capacity(self.conns.len());
        let mut rates: HashMap<u64, (u64, u64)> = HashMap::with_capacity(self.conns.len());
        for c in &self.conns {
            cur.insert(c.id, (c.bytes_in, c.bytes_out));
            let r = match prev.get(&c.id) {
                Some((pin, pout)) if dt > 0.0 => (
                    c.bytes_in.saturating_sub(*pin),
                    c.bytes_out.saturating_sub(*pout),
                ),
                _ => (0, 0),
            };
            rates.insert(c.id, r);
        }
        self.conn_prev_bytes = cur;
        self.conn_rates = rates;
    }

    fn mark_config_dirty(&mut self) {
        self.config_dirty = true;
        self.config_dirty_since = Instant::now();
    }
}

/// 表快照连接与 ETW 流的合并键(不含本地 IP)
fn flow_merge_key(k: &etw::FlowKey) -> (u32, Protocol, u16, Ipv4Addr, u16) {
    (k.pid, k.proto, k.local_port, k.remote_ip, k.remote_port)
}

/// 同一 UDP socket 行(PID+本地端口)的 ETW 流归并结果
struct UdpGroup<'a> {
    /// 代表流(最近活跃),其远端端点回填到表快照行
    rep: &'a etw::FlowAgg,
    /// 组内全部流的收发字节和 (下行, 上行):远端列只展示一个代表端点,
    /// 字节列保持端口级总量不丢账
    bytes: (u64, u64),
    /// 组内全部流键:合并命中的流完结时不再按短命连接落盘,由 Tracker
    /// 按表行口径处理(与 TCP 一致)
    keys: Vec<etw::FlowKey>,
}

/// UDP 流按 (PID, 本地端口) 分组:一个 socket 可与多个远端通信,
/// 表快照行只有一条,归并后单行承载全部远端的流量
fn merge_udp_groups<'a>(flows: &'a [etw::FlowAgg]) -> HashMap<(u32, u16), UdpGroup<'a>> {
    let mut groups: HashMap<(u32, u16), UdpGroup<'a>> = HashMap::new();
    for f in flows.iter().filter(|f| f.key.proto == Protocol::Udp) {
        let g = groups
            .entry((f.key.pid, f.key.local_port))
            .or_insert_with(|| UdpGroup {
                rep: f,
                bytes: (0, 0),
                keys: Vec::new(),
            });
        g.bytes.0 += f.down_bytes;
        g.bytes.1 += f.up_bytes;
        g.keys.push(f.key);
        // 代表流取 (最近活跃, 流量) 字典序最大:活跃度并列时选择不抖动
        if (f.last, f.down_bytes + f.up_bytes) > (g.rep.last, g.rep.down_bytes + g.rep.up_bytes) {
            g.rep = f;
        }
    }
    groups
}

/// 短命连接完结事件:进程反查失败(已退出)时进程名留空;
/// 起止时间由流的首末事件时刻换算 unix 秒
fn short_lived_event(f: &etw::FlowAgg) -> history::ClosedConn {
    let (proc_path, process) = match collector::query_process_path(f.key.pid) {
        Some(path) => {
            let name = path.rsplit(['\\', '/']).next().unwrap_or(&path).to_owned();
            (Some(path), name)
        }
        None => (None, String::new()),
    };
    let now = history::unix_now() as i64;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    f.key.hash(&mut h);
    history::ClosedConn {
        event_id: h.finish(),
        first_seen: (now - f.first.elapsed().as_secs() as i64).max(0) as u64,
        last_seen: (now - f.last.elapsed().as_secs() as i64).max(0) as u64,
        pid: f.key.pid,
        process,
        proc_path,
        signed: Signing::Unknown,
        proto: f.key.proto,
        remote_ip: f.key.remote_ip,
        remote_port: f.key.remote_port,
        bytes_in: f.down_bytes,
        bytes_out: f.up_bytes,
    }
}

impl eframe::App for NetOwlApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_tray_commands(ctx);
        self.calibrate_window_visible();
        self.sync_tray_pinned();
        // 关机/注销落库钩子:首帧安装一次,内部防重复(winit 不处理
        // ENDSESSION,关机时唯一能把活跃连接落库的路径)
        shutdown_hook::install(
            if self.main_hwnd != 0 {
                self.main_hwnd
            } else {
                single_instance::main_hwnd(crate::APP_NAME)
            },
            &mut self.tracker,
            &mut self.writer,
        );
        self.ensure_collector();
        self.poll_local_ip();
        self.poll_traffic(ctx);
        self.poll_conns(ctx);
        self.writer.set_retention(self.config.general.history_days);
        self.poll_wfp();

        // 进入设置页时重扫 locales,加载运行期间新增的词条文件;
        // 进入历史页时标记重新加载(结果与库大小)
        if self.page == Page::Settings && self.last_page != Page::Settings {
            self.i18n.refresh_languages();
        }
        if self.page == Page::History && self.last_page != Page::History {
            self.history.dirty = true;
        }
        self.last_page = self.page;

        self.sync_language_to_config();
        self.sync_theme_to_config();
        self.restore_window_geometry(ctx);
        self.capture_window_geometry(ctx);

        // 点关闭按钮 = 隐藏到托盘;仅托盘"退出"命令置位后才放行
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && !self.should_exit {
            self.window_visible = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // 配置写盘节流:防抖窗口内合并连续变更,只落盘最后一次
        if self.config_dirty && self.config_dirty_since.elapsed() >= CONFIG_SAVE_DEBOUNCE {
            self.config.save_to_file();
            self.config_dirty = false;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 页面与重绘判定先行(page 借用持续到帧末,后续不能碰 self)
        let shown = self.is_shown(ui.ctx());
        let log_open = self.log_window.open;

        // 自绘标题栏(顶栏最先声明,位于导航栏之上);窗口动作直接处理,
        // 此时 page/UiCtx 借用尚未建立,Close 可安全写 self.window_visible
        let title_action = egui::Panel::top("titlebar")
            .exact_size(ui::titlebar::HEIGHT)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::c().bg_panel))
            .show(ui, |ui| ui::titlebar::show(ui, crate::APP_NAME))
            .inner;
        match title_action {
            ui::titlebar::TitleAction::Minimize => {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            ui::titlebar::TitleAction::ToggleMaximize => {
                let maximized = ui.ctx().input(|i| i.viewport().maximized).unwrap_or(false);
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            ui::titlebar::TitleAction::Close => {
                // 与系统关闭事件同路径:隐藏到托盘(logic 的 close 拦截兜底 Alt+F4)
                self.window_visible = false;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            ui::titlebar::TitleAction::None => {}
        }

        // 字段级拆借用:conns/rdns 只读,config/map_view 需可变(设置页与地图交互)
        let page = &mut self.page;
        let collector_kind = self.collector.kind();
        // 本机点位:公网 IP 归属(探测失败/未收录时用默认位置)
        let local_pos = self
            .local_place
            .map(geoip::place_pos)
            .unwrap_or((world::LOCAL.lon, world::LOCAL.lat));
        let mut ctx = ui::UiCtx {
            conns: &self.conns,
            i18n: &mut self.i18n,
            map_view: &mut self.map_view,
            config: &mut self.config,
            rdns: &self.rdns,
            rates: self.rates,
            rate_hist: &self.rate_hist,
            conn_rates: &self.conn_rates,
            conn_sort: &mut self.conn_sort,
            conn_row_hover: &mut self.conn_row_hover,
            rules_row_hover: &mut self.rules_row_hover,
            log_window: &mut self.log_window,
            icon_tex: &self.icon_tex,
            default_icon_tex: self.default_icon_tex.as_ref(),
            history: &mut self.history,
            history_db: &self.history_db,
            rules: &mut self.rules,
            rules_page: &mut self.rules_page,
            map_panels: &mut self.map_panels,
            wfp_status: self.wfp.status(),
            writer: &self.writer,
            local_pos,
        };
        let mut config_changed = false;

        egui::Panel::left("nav")
            .exact_size(210.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::c().bg_panel)
                    .inner_margin(egui::Margin {
                        left: 14,
                        right: 14,
                        top: 18,
                        bottom: 14,
                    }),
            )
            .show(ui, |ui| {
                ui::nav_ui(ui, page, &ctx, &self.logo_tex, collector_kind)
            });

        // 地图页操作面板:面板必须先于中央面板声明(egui 的面板顺序约束),
        // 位于导航栏与中央画布之间,宽度可拖拽调节
        if *page == Page::Map && ctx.map_panels.show_left {
            egui::Panel::left("map-list")
                .resizable(true)
                .default_size(300.0)
                .min_size(260.0)
                .max_size(460.0)
                .frame(
                    egui::Frame::new()
                        .fill(theme::c().bg_panel)
                        .inner_margin(egui::Margin::same(10)),
                )
                .show(ui, |ui| ui::map_panel::list_panel(ui, &mut ctx));
        }
        if *page == Page::Map && ctx.map_panels.show_right {
            egui::Panel::right("map-inspector")
                .resizable(true)
                .default_size(320.0)
                .min_size(260.0)
                .max_size(480.0)
                .frame(
                    egui::Frame::new()
                        .fill(theme::c().bg_panel)
                        .inner_margin(egui::Margin::same(10)),
                )
                .show(ui, |ui| ui::map_inspector::inspector_panel(ui, &mut ctx));
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::c().bg_base)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ui, |ui| {
                config_changed |= ui::central_ui(ui, page, &mut ctx);
            });
        // 后台(托盘/最小化)统一低频;可见时地图动画 30fps,连接页速率 500ms,
        // 历史/规则/设置等静态页 1s(页面切换与交互事件即时唤醒);
        // 日志窗口打开时 500ms 保证流式观察
        let repaint = if !shown {
            REPAINT_IDLE
        } else {
            match page {
                Page::Map => REPAINT_ANIMATED,
                Page::Connections => REPAINT_IDLE,
                _ if log_open => REPAINT_IDLE,
                _ => REPAINT_STATIC,
            }
        };
        ui.ctx().request_repaint_after(repaint);

        // 无边框窗口边缘缩放热区(winit 对 undecorated 窗口无 hit-test):
        // 命中即设置缩放光标并驱动拖拽缩放;放帧末使光标覆盖页面控件设置
        let maximized = ui.ctx().input(|i| i.viewport().maximized).unwrap_or(false);
        let hit = if maximized {
            None
        } else {
            let screen = ui.ctx().input(|i| i.viewport_rect());
            ui.ctx()
                .pointer_latest_pos()
                .and_then(|pos| resize::edge_hit_test(pos, screen))
        };
        let press = ui.ctx().input(|i| i.pointer.primary_pressed());
        let held = ui.ctx().input(|i| i.pointer.primary_down());
        self.window_resize.update(
            self.main_hwnd,
            hit,
            press,
            held,
            MIN_WINDOW_SIZE,
            ui.ctx().pixels_per_point(),
        );
        let cursor_hit = self.window_resize.active().or(hit);
        if let Some(h) = cursor_hit {
            ui.ctx().set_cursor_icon(h.cursor_icon());
        }

        // 新连接询问弹窗(独立 viewport);决策即时生效
        let decision = self
            .asker
            .active
            .as_mut()
            .and_then(|item| ui_ask::show(ui.ctx(), item, &self.i18n));
        if let Some(d) = decision {
            self.apply_decision(d);
        }
        // 日志浏览窗口(独立 viewport;内存层增量拉取,关闭时停止收集)
        ui::log_window::show(ui.ctx(), &mut self.log_window, &self.i18n);
        if config_changed {
            self.mark_config_dirty();
        }
    }
}
