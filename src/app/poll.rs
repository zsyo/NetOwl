//! 采集编排:连接快照轮询与其派生数据(UDP 去重/rDNS/图标/历史/询问)、
//! 总速率采样、公网 IP 探测与 WFP 过滤器目标集合同步;悬浮球快照/
//! 配额告警/更新回收等外围任务见 poll_derived。
//! 全部按 1s 节流,高帧率重绘帧内跳过。

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use eframe::egui;

use super::{
    CONNS_REFRESH_INTERVAL, LOCAL_IP_PROBE_INTERVAL, NetOwlApp, QUALITY_ALERT_FAILS, RATE_HIST_LEN,
    TRAFFIC_INTERVAL_ACTIVE, TRAFFIC_INTERVAL_HIDDEN, WFP_SYNC_INTERVAL,
};
use crate::collector::{self, CollectorKind};
use crate::map;
use crate::map::world;
use crate::model::{Place, Protocol};
use crate::net::geoip;
use crate::net::rdns;
use crate::rules;
use crate::rules::wfp;
use crate::storage::history;

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
        // 这些行注定共享同一份回填数据,按 (pid, 本地端口) 去重留一行。
        // 保留键必须确定:兄弟行同轮首现时 first_seen 相同,快照排序
        // 退化为 HashMap 迭代序,顺序保留会让保留行逐轮漂移——Tracker
        // 把旧 id 判完结落库、新 id 重建,历史与累计双计。固定保留
        // local_addr 最小者
        let mut keep_min: HashMap<(u32, u16), IpAddr> = HashMap::new();
        for c in self.conns.iter().filter(|c| c.proto == Protocol::Udp) {
            keep_min
                .entry((c.pid, c.local_port))
                .and_modify(|m| {
                    if c.local_addr < *m {
                        *m = c.local_addr;
                    }
                })
                .or_insert(c.local_addr);
        }
        self.conns.retain(|c| {
            c.proto != Protocol::Udp || keep_min.get(&(c.pid, c.local_port)) == Some(&c.local_addr)
        });
        self.poll_session_bytes();
        self.rdns.update(&self.conns);
        self.poll_icons(ctx);
        self.poll_history();
        self.poll_ask();
        self.poll_temp_rules();
        // 规则命中计数(1s 快照口径,非渲染帧):供规则页调优参考
        self.rules.note_hits(&self.conns, &self.rdns);
        // 新监听端口提醒(基线后新出现的监听端口):与 listens 刷新同轮,
        // toast 通知,设置页可关
        if self.config.general.listen_notify {
            for l in self.listen_watch.observe(&self.listens) {
                let process = if l.process.is_empty() {
                    self.i18n.t("conn-proc-unknown")
                } else {
                    l.process.clone()
                };
                let text = self.i18n.t_with_args(
                    "listen-notify-toast",
                    &[
                        ("process", process),
                        (
                            "endpoint",
                            format!("{} {}:{}", l.proto.as_str(), l.local_addr, l.local_port),
                        ),
                    ],
                );
                crate::ui::toast::push(&mut self.toasts, crate::ui::toast::ToastKind::Info, text);
            }
        }
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
        // 直接迭代 conns 查缓存(字段级不相交借用),不为未命中路径构造
        // 中间 Vec:每秒热路径,连接数大时是线性分配
        for c in &self.conns {
            let Some(path) = c.proc_path.as_deref() else {
                continue;
            };
            if self.icon_tex.contains_key(path) {
                continue;
            }
            if let collector::IconState::Ready(img) = self.collector.icon_image(path) {
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
                self.icon_tex.insert(path.to_owned(), tex);
            }
        }
    }

    /// 连接历史:diff 前后快照生成完结事件交写线程;mock 数据不入库
    pub(super) fn poll_history(&mut self) {
        let real = self.collector.kind() == CollectorKind::Real;
        let events = self.tracker.diff(real, &self.conns, history::unix_now());
        self.writer.send(events);
    }

    /// 本次会话累计(程序启动起算,只增不减,仅内存不落库):每轮按
    /// "本轮字节 - 上轮字节"增量累加。连接关闭后行从快照消失,已累加部分
    /// 保留在累计值里,不随快照收缩而扣减——会话总量是运行期间观测到的
    /// 总量,不是当前活跃连接的字节和。必须晚于 ETW 合并与 UDP 去重:
    /// 前者让字节取合并后的真实值,后者避免同一 socket 的多行重复计入。
    /// 三维度同源:全局 = 导航栏速率卡与 Inspector 概览卡,节点按归属地、
    /// 进程按映像名各计一张表——节点与进程在地图上是动态实体(无连接落在
    /// 该节点、进程退出即从面板消失),条目不淘汰,统计数据不因实体暂时
    /// 消失而丢失;程序重启归零
    pub(super) fn poll_session_bytes(&mut self) {
        let prev = std::mem::take(&mut self.session_prev_bytes);
        let mut cur: HashMap<u64, (u64, u64)> = HashMap::with_capacity(self.conns.len());
        for c in &self.conns {
            let (pin, pout) = prev.get(&c.id).copied().unwrap_or((0, 0));
            let din = c.bytes_in.saturating_sub(pin);
            let dout = c.bytes_out.saturating_sub(pout);
            self.session_bytes.0 += din;
            self.session_bytes.1 += dout;
            // 节点/进程维度与地图可见口径一致:本地/局域网远端按配置过滤
            // (节点必为公网远端,该过滤对节点维度是空操作)
            if crate::ui::conn_visible(&self.config, c) {
                if let Some(place) = c.city {
                    let e = self.session_place_bytes.entry(place).or_insert((0, 0));
                    e.0 += din;
                    e.1 += dout;
                }
                match self.session_proc_bytes.get_mut(c.process.as_str()) {
                    Some(e) => {
                        e.0 += din;
                        e.1 += dout;
                    }
                    None => {
                        self.session_proc_bytes
                            .insert(c.process.clone(), (din, dout));
                    }
                }
            }
            cur.insert(c.id, (c.bytes_in, c.bytes_out));
        }
        self.session_prev_bytes = cur;
    }

    /// 网络质量采样:自动目标 = 活跃连接的公网远端(rDNS 同口径),自定义
    /// 目标(设置页)叠加;60s TTL 内复用结果。与采集同频节流;开关关闭时
    /// 不派发、不清理(既有样本自然过期,重开后从零重建)
    pub(super) fn poll_quality(&mut self) {
        if self.quality_poll_at.elapsed() < CONNS_REFRESH_INTERVAL {
            return;
        }
        self.quality_poll_at = Instant::now();
        self.quality.update();
        if !self.config.general.quality_monitor {
            return;
        }
        let mut wanted: HashSet<IpAddr> = self
            .conns
            .iter()
            .map(|c| c.remote_ip)
            .filter(rdns::is_queryable)
            .collect();
        for ip in &wanted {
            self.quality.enqueue(*ip, None);
        }
        // 自定义目标:同 IP 时 TCP 补测端口覆盖自动目标(自定义优先);
        // IP 解析失败(手输未完成)跳过,设置页有即时校验提示
        let mut custom: Vec<(IpAddr, String)> = Vec::new();
        for t in &self.config.general.quality_targets {
            let Ok(ip) = t.ip.trim().parse::<IpAddr>() else {
                continue;
            };
            self.quality.enqueue(ip, (t.port != 0).then_some(t.port));
            custom.push((ip, t.label.clone()));
        }
        let custom_ips: HashSet<IpAddr> = custom.iter().map(|(ip, _)| *ip).collect();
        wanted.extend(custom_ips.iter().copied());
        self.quality.prune(&wanted);
        self.poll_quality_alert(&custom);
    }

    /// 自定义目标连续多轮无响应:toast 告警一次(集合去重),恢复响应后
    /// 重新武装;目标删除后样本经 prune 回收,告警态随之失效
    fn poll_quality_alert(&mut self, custom: &[(IpAddr, String)]) {
        for (ip, label) in custom {
            let fails = self.quality.consec_fails(*ip);
            if fails >= QUALITY_ALERT_FAILS {
                if self.quality_alerted.insert(*ip) {
                    let name = if label.is_empty() {
                        ip.to_string()
                    } else {
                        label.clone()
                    };
                    let text = self.i18n.t_with_args(
                        "quality-alert-toast",
                        &[("name", name), ("n", QUALITY_ALERT_FAILS.to_string())],
                    );
                    crate::ui::toast::push(
                        &mut self.toasts,
                        crate::ui::toast::ToastKind::Warn,
                        text,
                    );
                    tracing::warn!("[Quality] 自定义目标 {ip} 连续 {fails} 轮无响应");
                }
            } else if fails == 0 {
                self.quality_alerted.remove(ip);
            }
        }
    }

    /// WFP 过滤器同步:与采集同频重建目标集合(进程路径粘滞展开,
    /// 见 RuleSet::wfp_specs);mock 数据源下清空过滤器,拦截只针对
    /// 真实连接。询问中的连接追加最高 weight 的临时阻断(安全默认)
    pub(super) fn poll_wfp(&mut self) {
        if self.wfp_sync_at.elapsed() < WFP_SYNC_INTERVAL {
            return;
        }
        self.wfp_sync_at = Instant::now();
        // 域名规则解析服务:收割结果、全量入队启用中的域名(幂等,缓存
        // 未过期跳过,TTL 到期自然重解析),并清理已删除/停用规则的域名;
        // 解析 IP 回填规则集后,本轮 wfp_specs 即可展开
        self.resolve.update();
        let wanted: HashSet<String> = self
            .rules
            .rules
            .iter()
            .filter(|r| r.enabled && r.remote_kind == rules::RemoteKind::Domain)
            .map(|r| r.remote_value.trim().to_lowercase())
            .collect();
        for d in &wanted {
            self.resolve.enqueue(d);
        }
        self.resolve.prune(&wanted);
        self.rules.sync_domain_ips(&self.resolve);
        let mut specs = if self.collector.kind() == CollectorKind::Real {
            self.rules.wfp_specs(&self.conns)
        } else {
            Vec::new()
        };
        // 询问等待期间临时阻断:active 与 queue 全部条目都生成 pending
        // spec——队列上限 10 项,只覆盖 active 时排队项身份的连接最长
        // 数百秒自由收发。同为 weight 15,不受用户规则 14 条钳制
        for item in self.asker.active.iter().chain(self.asker.queue.iter()) {
            specs.insert(0, item.pending_block_spec());
        }
        // 静默拒绝:兜底全通配阻断(weight 0,低于全部用户规则)+ 自身
        // 放行(weight 15,防兜底切断 NetOwl 自身连接)。不限远端的条件
        // 在 v4/v6 四层同时铺,任一族流量都不脱管。与询问 pending spec
        // 互斥:deny 下询问关闭,pending 不存在
        if self.config.general.silent_mode == "deny" && self.collector.kind() == CollectorKind::Real
        {
            // 自身路径启动时缓存一次(进程路径不变),不逐秒系统调用
            for layer in [
                wfp::Layer::Out,
                wfp::Layer::In,
                wfp::Layer::Out6,
                wfp::Layer::In6,
            ] {
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
}
