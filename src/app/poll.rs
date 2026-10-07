//! 采集编排:连接快照轮询与其派生数据(UDP 去重/rDNS/图标/历史/询问)、
//! 总速率采样、公网 IP 探测、WFP 过滤器目标集合同步与悬浮球数据快照。
//! 全部按 1s 节流,高帧率重绘帧内跳过。

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

use super::{
    CONNS_REFRESH_INTERVAL, LOCAL_IP_PROBE_INTERVAL, NetOwlApp, RATE_HIST_LEN,
    TRAFFIC_INTERVAL_ACTIVE, TRAFFIC_INTERVAL_HIDDEN, WFP_SYNC_INTERVAL,
};
use crate::collector::{self, CollectorKind};
use crate::map;
use crate::map::world;
use crate::model::{Place, Protocol};
use crate::net::geoip;
use crate::rules;
use crate::rules::wfp;
use crate::storage::history;
use crate::ui::floating_ball;

impl NetOwlApp {
    /// 数据源配置与当前实例不一致(设置页切换)时重建采集器,切换即时生效;
    /// 静默兜底按数据源判定生效,重建后强制重新校准
    pub(super) fn ensure_collector(&mut self) {
        let kind = CollectorKind::from_config(&self.config.general.collector);
        if kind != self.collector.kind() {
            tracing::info!("[Collector] 数据源切换 -> {kind:?},重建采集器");
            self.collector = collector::build(kind);
            self.silent_synced = None;
            self.mark_config_dirty();
        }
    }

    /// 连接数据刷新(1s 节流):快照拉取与其全部派生逻辑。
    /// 高帧率重绘(地图动画)帧内直接跳过,连接数据本就是秒级口径。
    /// ETW 合并(UDP 远端回填)先于 rDNS 派发与 Tracker diff:
    /// 前者让 UDP 远端可查 PTR,后者让完结落库带真实远端;
    /// 远端未知行的排除也必须夹在两者之间:早于回填会把待回填的
    /// 活跃 UDP 行一并丢弃,晚于落库则 0.0.0.0:0 已写入历史
    pub(super) fn poll_conns(&mut self, ctx: &egui::Context) {
        if self.conns_refresh_at.elapsed() < CONNS_REFRESH_INTERVAL {
            return;
        }
        self.conns_refresh_at = Instant::now();
        self.conns = self.collector.snapshot();
        self.listens = self.collector.listening();
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

    /// 本机公网 IP 探测:取每轮首个成功结果,归属变化时刷新地图本机点位;
    /// 启动定位窗口内的首个结果把地图瞬跳到本机中心最大缩放(默认视图)
    pub(super) fn poll_local_ip(&mut self, ctx: &egui::Context) {
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
            if self.map_locate_pending {
                self.map_locate_pending = false;
                if Instant::now() <= self.map_locate_deadline {
                    // 与地图本机节点同源:归属未收录时回退默认点位
                    let pos = place
                        .map(geoip::place_pos)
                        .unwrap_or((world::LOCAL.lon, world::LOCAL.lat));
                    self.map_view = map::home_view(pos);
                    ctx.request_repaint();
                    tracing::debug!("[LocalIp] 地图定位到本机点位 ({:.1}, {:.1})", pos.0, pos.1);
                }
            }
        }
        if self.local_probe_at.elapsed() >= LOCAL_IP_PROBE_INTERVAL {
            self.local_probe_at = Instant::now();
            self.local_probe.begin_round();
        }
    }

    /// 总速率采样:可见与隐藏(托盘)均按秒采样,托盘悬停提示跟随刷新。
    /// 仅在真实采样(节流间隔到达且读表成功)时更新当前值并推进历史序列:
    /// logic 每帧执行,无条件 push 会让走势图随帧率滚动(地图动画 30fps
    /// 时 60 点缓冲两秒滚完,数据相同画成横线)
    pub(super) fn poll_traffic(&mut self, ctx: &egui::Context) {
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

    /// 进程图标:活跃连接的映像路径逐个请求采集器,到位即建纹理缓存。
    /// 纹理键 = 路径,同进程连接共享;提取失败缓存 None 不再重复请求
    pub(super) fn poll_icons(&mut self, ctx: &egui::Context) {
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
            .filter_map(|c| c.proc_path.as_deref())
            .filter(|p| !self.icon_tex.contains_key(*p))
            .map(str::to_owned)
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
    pub(super) fn poll_history(&mut self) {
        let real = self.collector.kind() == CollectorKind::Real;
        let events = self.tracker.diff(real, &self.conns, history::unix_now());
        self.writer.send(events);
    }

    /// WFP 过滤器同步:与采集同频重建目标集合(进程路径粘滞展开,
    /// 见 RuleSet::wfp_specs);mock 数据源下清空过滤器,拦截只针对
    /// 真实连接。询问中的连接追加最高 weight 的临时阻断(安全默认)
    pub(super) fn poll_wfp(&mut self) {
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
        // 静默拒绝:兜底全通配阻断(weight 0,低于全部用户规则)+ 自身
        // 放行(weight 15,防兜底切断 NetOwl 自身连接)。与询问 pending
        // spec 互斥:deny 下询问关闭,pending 不存在
        if self.config.general.silent_mode == "deny" && self.collector.kind() == CollectorKind::Real
        {
            // 自身路径启动时缓存一次(进程路径不变),不逐秒系统调用
            for layer in [wfp::Layer::Out, wfp::Layer::In] {
                specs.push(wfp::Spec {
                    layer,
                    weight: rules::WEIGHT_RESERVED_HIGH,
                    block: false,
                    app_path: self.self_path.clone(),
                    remote: None,
                    proto: None,
                    port: None,
                });
                specs.push(wfp::Spec {
                    layer,
                    weight: rules::WEIGHT_FALLBACK,
                    block: true,
                    app_path: None,
                    remote: None,
                    proto: None,
                    port: None,
                });
            }
        }
        self.wfp.sync(Arc::new(specs));
    }

    /// 悬浮球数据快照:总速率与进程速率榜 1s 节流重建(与连接采集同频);
    /// 仅开关开启时计算
    pub(super) fn poll_ball_data(&mut self) {
        if !self.config.floating_ball.enabled
            || self.ball_data_at.elapsed() < Duration::from_secs(1)
        {
            return;
        }
        self.ball_data_at = Instant::now();
        let (up_top, down_top) = floating_ball::collect_proc_rates(
            &self.conns,
            &self.conn_rates,
            &self.config,
            &self.icon_tex,
        );
        self.ball_data = floating_ball::BallData {
            logo: Some(self.logo_tex.clone()),
            default_icon: self.default_icon_tex.clone(),
            rates: self.rates,
            up_top,
            down_top,
            elevated: self.elevated,
        };
    }

    /// 用量配额告警:分钟级轻查当月累计(落库)+ 活跃连接字节,达到
    /// 80%/100% 阈值时 toast 告警;每级会话内一次,用量回落到阈值下
    /// (月切换/数据清理)自动重置
    pub(super) fn poll_quota(&mut self) {
        let quota_gb = self.config.general.usage_quota_gb;
        if quota_gb == 0 {
            self.quota_flags = (false, false);
            return;
        }
        if self.quota_checked_at.elapsed() < Duration::from_secs(60) {
            return;
        }
        self.quota_checked_at = Instant::now();
        let month = crate::storage::history_query::query_month_bytes(&self.history_db);
        let active: u64 = self.conns.iter().map(|c| c.total_bytes()).sum();
        let total = month + active;
        let quota = u64::from(quota_gb) * (1 << 30);
        let want = (total * 100 >= quota * 4 / 5, total >= quota);
        if want.0 && !self.quota_flags.0 {
            let text = self.i18n.t_with_args(
                "quota-warn-toast",
                &[("used", crate::model::fmt_bytes(total))],
            );
            crate::ui::toast::push(&mut self.toasts, crate::ui::toast::ToastKind::Warn, text);
        }
        if want.1 && !self.quota_flags.1 {
            let text = self.i18n.t_with_args(
                "quota-exceed-toast",
                &[("used", crate::model::fmt_bytes(total))],
            );
            crate::ui::toast::push(&mut self.toasts, crate::ui::toast::ToastKind::Warn, text);
        }
        self.quota_flags = want;
    }

    /// 检查更新:接收设置页按钮触发,派发后台线程请求 GitHub Releases
    /// 并收割结果(网络阻塞调用不进主线程;完成即请求重绘,结果即时可见)
    pub(super) fn poll_update_check(&mut self, ctx: &egui::Context) {
        if self.update_request {
            self.update_request = false;
            if self.update_rx.is_none() {
                let preview = self.config.general.update_channel == "preview";
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let result = crate::platform::update::fetch_latest(preview);
                    if tx.send(result).is_err() {
                        tracing::warn!("[Update] 检查结果发送失败:接收端已关闭");
                    }
                });
                self.update_rx = Some(rx);
                self.update_result = None;
                tracing::info!(
                    "[Update] 检查更新已发起(渠道 {})",
                    if preview { "preview" } else { "stable" }
                );
            }
        }
        let Some(rx) = &self.update_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                match &result {
                    Ok(Some(info)) => {
                        let newer = crate::platform::update::is_newer(
                            &info.tag_name,
                            crate::platform::update::CURRENT_VERSION,
                        );
                        tracing::info!(
                            "[Update] 最新发布 {} (prerelease={}),{}更新",
                            info.tag_name,
                            info.prerelease,
                            if newer { "需要" } else { "无需" }
                        );
                    }
                    Ok(None) => tracing::info!("[Update] 仓库暂无发布版本"),
                    Err(e) => tracing::warn!("[Update] 检查失败: {e}"),
                }
                self.update_result = Some(result);
                self.update_rx = None;
                ctx.request_repaint();
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // 发送端先行 panic 的异常路径:复位避免 UI 永久停在检查中
                self.update_rx = None;
                self.update_result = Some(Err("更新检查线程异常退出".to_owned()));
            }
        }
    }
}
