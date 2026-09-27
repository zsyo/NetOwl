//! 应用编排:托盘事件、采集 tick、页面切换、配置持久化(窗口几何/语言)、
//! 关闭到托盘与退出。
//!
//! eframe 0.36 的 App trait 拆分为 logic(每帧逻辑,窗口隐藏时仍会被调用)
//! 与 ui(绘制)。托盘命令、几何捕获与配置写盘节流放在 logic。

use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::basemap;
use crate::collector::{self, Collector, CollectorKind};
use crate::config::Config;
use crate::geoip;
use crate::i18n::I18n;
use crate::local_ip;
use crate::model::{Connection, Place};
use crate::rdns;
use crate::theme;
use crate::traffic;
use crate::tray::{self, Tray};
use crate::ui::{self, Page};
use crate::world;

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
        NetOwlApp {
            page: Page::Map,
            last_page: Page::Map,
            map_view: basemap::View::global(),
            collector: collector::build(CollectorKind::from_config(&config.general.collector)),
            rdns: rdns::Rdns::new(),
            traffic: traffic::Sampler::new(),
            rates: (0, 0),
            icon_tex: HashMap::new(),
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
                    // 退出前把待写配置立即落盘
                    self.config.save_to_file();
                    self.config_dirty = false;
                    self.should_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                _ => {}
            }
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
                    eprintln!("[LocalIp] 本机公网 IP {ip}({source})");
                } else {
                    eprintln!("[LocalIp] 本机公网 IP {ip}({source}) 无归属,回退默认点位");
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
        let interval = if visible { TRAFFIC_INTERVAL_ACTIVE } else { TRAFFIC_INTERVAL_HIDDEN };
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

    fn mark_config_dirty(&mut self) {
        self.config_dirty = true;
        self.config_dirty_since = Instant::now();
    }
}

impl eframe::App for NetOwlApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_tray_commands(ctx);
        self.ensure_collector();
        self.poll_local_ip();
        self.poll_traffic(ctx);
        self.conns = self.collector.snapshot();
        self.rdns.update(&self.conns);
        self.poll_icons(ctx);

        // 进入设置页时重扫 locales,加载运行期间新增的词条文件
        if self.page == Page::Settings && self.last_page != Page::Settings {
            self.i18n.refresh_languages();
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
            icon_tex: &self.icon_tex,
            local_pos,
        };

        egui::Panel::left("nav")
            .exact_size(210.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::c().bg_panel)
                    .inner_margin(egui::Margin { left: 14, right: 14, top: 18, bottom: 14 }),
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
            .show(ui, |ui| ui::central_ui(ui, page, &mut ctx));

        let repaint = match page {
            Page::Map => REPAINT_ANIMATED,
            _ => REPAINT_IDLE,
        };
        ui.ctx().request_repaint_after(repaint);
    }
}
