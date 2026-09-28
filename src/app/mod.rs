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
use crate::map::basemap;
use crate::map::world;
use crate::model::{Connection, Place, Protocol, Signing};
use crate::net::etw;
use crate::net::geoip;
use crate::net::local_ip;
use crate::net::rdns;
use crate::net::traffic;
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

/// 重绘节奏:地图页动画 30fps,静态页面低频
const REPAINT_ANIMATED: Duration = Duration::from_millis(33);
const REPAINT_IDLE: Duration = Duration::from_millis(500);
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
/// WFP 过滤器目标集合同步间隔(与采集同频:进程路径出现/消失的生效延迟上限)
const WFP_SYNC_INTERVAL: Duration = Duration::from_secs(1);
/// ETW 流量事件合并间隔(与表快照采集同频)
const ETW_POLL_INTERVAL: Duration = Duration::from_secs(1);
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
    /// WFP 拦截引擎(启用规则翻译为过滤器,管理线程持有动态会话)
    wfp: wfp::Manager,
    wfp_sync_at: Instant,
    /// ETW 流量事件采集(提权时启动:连接字节填充与短命连接收割)
    etw: Option<etw::Etw>,
    etw_poll_at: Instant,
    /// 曾被表快照合并覆盖的 ETW 流键:完结流命中此集合说明表快照
    /// 跟踪器已记录,不按短命连接重复落盘
    etw_seen: HashSet<etw::FlowKey>,
    /// 每连接实时速率(键 = conn.id,ETW 字节差值/秒)
    conn_rates: HashMap<u64, (u64, u64)>,
    /// 连接列表表头排序状态(会话内,不持久化)
    conn_sort: ui::ConnSortState,
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
                Ok(e) => Some(e),
                Err(e) => {
                    tracing::warn!("[ETW] {e}");
                    None
                }
            }
        } else {
            None
        };
        NetOwlApp {
            page: Page::Map,
            last_page: Page::Map,
            map_view: basemap::View::global(),
            collector: collector::build(CollectorKind::from_config(&config.general.collector)),
            rdns: rdns::Rdns::new(),
            traffic: traffic::Sampler::new(),
            rates: (0, 0),
            icon_tex: HashMap::new(),
            writer: history::Writer::spawn(config.general.history_days),
            tracker: history::Tracker::new(),
            history: history_query::PageState::new(),
            history_db,
            rules,
            rules_page: ui_rules::PageState::new(),
            wfp: wfp::Manager::spawn(),
            wfp_sync_at: Instant::now(),
            etw,
            etw_poll_at: Instant::now(),
            etw_seen: HashSet::new(),
            conn_rates: HashMap::new(),
            conn_sort: None,
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
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                tray::CMD_HIDE => {
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

    /// 总速率采样:窗口隐藏(托盘)时放宽采样间隔降低功耗
    fn poll_traffic(&mut self, ctx: &egui::Context) {
        let visible = ctx.input(|i| i.viewport().visible()) != Some(false);
        let interval = if visible {
            TRAFFIC_INTERVAL_ACTIVE
        } else {
            TRAFFIC_INTERVAL_HIDDEN
        };
        self.rates = self.traffic.poll(interval);
    }

    /// 进程图标:活跃连接的映像路径逐个请求采集器,到位即建纹理缓存。
    /// 纹理键 = 路径,同进程连接共享;提取失败缓存 None 不再重复请求
    fn poll_icons(&mut self, ctx: &egui::Context) {
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
    /// 1) 活跃流的收发字节填充到连接快照(键 = PID+协议+本地端口+远端,
    ///    不含本地 IP —— Connection 未暴露该字段);
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
        let by_key: HashMap<(u32, Protocol, u16, Ipv4Addr, u16), &etw::FlowAgg> =
            flows.iter().map(|f| (flow_merge_key(&f.key), f)).collect();
        let mut hits: Vec<etw::FlowKey> = Vec::new();
        if real {
            for c in &mut self.conns {
                let Some(f) =
                    by_key.get(&(c.pid, c.proto, c.local_port, c.remote_ip, c.remote_port))
                else {
                    continue;
                };
                c.bytes_in = f.down_bytes;
                c.bytes_out = f.up_bytes;
                hits.push(f.key);
            }
        }
        self.etw_seen.extend(hits);
        drop(by_key);
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
    }
}

impl eframe::App for NetOwlApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_tray_commands(ctx);
        self.sync_tray_pinned();
        self.ensure_collector();
        self.poll_local_ip();
        self.poll_traffic(ctx);
        self.conns = self.collector.snapshot();
        self.rdns.update(&self.conns);
        self.poll_icons(ctx);
        self.writer.set_retention(self.config.general.history_days);
        self.poll_history();
        self.poll_etw();
        self.poll_ask();
        self.poll_temp_rules();
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
            conn_rates: &self.conn_rates,
            conn_sort: &mut self.conn_sort,
            icon_tex: &self.icon_tex,
            history: &mut self.history,
            history_db: &self.history_db,
            rules: &mut self.rules,
            rules_page: &mut self.rules_page,
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
                ui::nav_ui(ui, page, ctx.conns, ctx.i18n, ctx.rates, collector_kind)
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::c().bg_base)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ui, |ui| {
                config_changed |= ui::central_ui(ui, page, &mut ctx);
            });
        let repaint = match page {
            Page::Map => REPAINT_ANIMATED,
            _ => REPAINT_IDLE,
        };
        ui.ctx().request_repaint_after(repaint);

        // 新连接询问弹窗(独立 viewport);决策即时生效
        let decision = self
            .asker
            .active
            .as_mut()
            .and_then(|item| ui_ask::show(ui.ctx(), item, &self.i18n));
        if let Some(d) = decision {
            self.apply_decision(d);
        }
        if config_changed {
            self.mark_config_dirty();
        }
    }
}
