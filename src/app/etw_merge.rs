//! ETW 流量事件合并:活跃流字节回填连接快照(TCP 精确键 / UDP 按
//! socket 归并)、UDP 远端回填与缓存回退、每连接实时速率、短命连接
//! 收割落盘。流采集引擎见 crate::net::etw。

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::net::Ipv4Addr;
use std::time::Instant;

use super::{ETW_POLL_INTERVAL, NetOwlApp};
use crate::collector::{self, CollectorKind};
use crate::model::{Place, Protocol, Signing};
use crate::net::{etw, geoip};
use crate::storage::history;

impl NetOwlApp {
    /// ETW 流量事件合并(与采集同频):
    /// 1) 活跃流的收发字节填充到连接快照:TCP 按精确键(PID+协议+本地端口+
    ///    远端,不含本地 IP —— Connection 未暴露该字段);UDP 表行无远端
    ///    (系统 UDP 表不含对端),按 PID+本地端口归并,行字节为该端口全部
    ///    远端流之和,远端回填最近活跃流的端点(见 merge_udp_groups),归属
    ///    就地重算,下游(显示/归属/域名/规则/询问/过滤/地图)随之生效;
    /// 2) 完结流若从未被表快照覆盖(存活短于采样间隙的短命连接)则
    ///    生成历史事件落盘;曾被覆盖的由 Tracker 正常处理,跳过后从
    ///    etw_seen 移除(集合有界,同键新流不误挡)。
    pub(super) fn poll_etw(&mut self) {
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
            if !real {
                continue;
            }
            // 曾被表快照覆盖的流由 Tracker 按表口径落库,不再按短命连接
            // 双计;查完即移除,同键新流(连接重建后的短命流)不被旧记录误挡
            if self.etw_seen.remove(&f.key) {
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
            tracing::debug!("[ETW] 短命连接落库 {} 条", events.len());
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
            let name = crate::model::image_name(&path).to_owned();
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
