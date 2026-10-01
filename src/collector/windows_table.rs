//! 真实连接采集:GetExtendedTcpTable/GetExtendedUdpTable(owner-PID)定期
//! 快照(底层查询见 query),进程元数据(路径/签名)按 PID 缓存;路径反查
//! 被目标 DACL 拒绝(非提权/受保护服务)时以 NtQuerySystemInformation
//! 全量名字快照兜底,拿名字但无完整路径。
//! 均为只读查询,普通用户权限即可,无需管理员。
//!
//! 表快照无字节计数语义(ESTATS Data 采集在本机系统上返回值不可信,
//! 已实测为部分未初始化数据),下载/上传列显示 0;短命 UDP 交互与字节
//! 统计依赖后续 ETW 事件源补齐。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use super::icon::{self, IconImage};
use super::query::{ConnKey, query_process_names, query_process_path, query_tcp, query_udp};
use super::{Collector, CollectorKind, IconState, signature};
use crate::model::{Connection, Place, Signing};

/// 表快照间隔:连接增减的可见延迟上限(与任务管理器刷新节奏相当)
const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// System 进程(PID 4)持有内核级 socket,任务管理器同样显示为 System
const SYSTEM_PID: u32 = 4;
/// 签名校验在途/每轮派发上限:WinVerifyTrust 对大文件可能秒级,防线程爆发
const SIG_MAX_INFLIGHT: usize = 4;
const SIG_DISPATCH_PER_POLL: usize = 2;
/// 图标提取每轮派发上限(读文件 + GDI 操作,后台线程执行)
const ICON_DISPATCH_PER_POLL: usize = 4;

/// 进程元数据(按 PID 缓存;签名状态异步回填)
#[derive(Clone)]
struct ProcMeta {
    name: String,
    path: Option<String>,
    signed: Signing,
}

/// 图标提取状态(按映像路径缓存;None 表示已尝试且无图标)
/// 真实采集器:每 POLL_INTERVAL 查询一次系统表,快照间维护连接与进程元数据缓存
pub struct TableCollector {
    /// 有序连接快照(累计流量降序),非轮询帧直接返回
    ordered: Vec<Connection>,
    /// 连接身份 -> 最近快照(保留 first_seen)
    live: HashMap<ConnKey, Connection>,
    /// PID -> 进程元数据(仅存活于连接表中的进程)
    proc_metas: HashMap<u32, ProcMeta>,
    /// 签名校验回报通道(发送端克隆给每个查询线程)
    sig_tx: Sender<(u32, Signing)>,
    sig_rx: Receiver<(u32, Signing)>,
    /// 已派发未返回签名结果的 PID
    sig_pending: HashSet<u32>,
    /// 映像路径 -> 图标状态(常驻缓存:连接关闭后进程再现时图标即取即用)
    icons: HashMap<String, IconState>,
    /// 图标提取回报通道(发送端克隆给每个提取线程)
    icon_tx: Sender<(String, Option<IconImage>)>,
    icon_rx: Receiver<(String, Option<IconImage>)>,
    /// 已派发未返回的图标提取请求(按映像路径)
    icon_inflight: HashSet<String>,
    last_poll: Instant,
    pending_poll: bool,
}

impl TableCollector {
    pub fn new() -> Self {
        let (sig_tx, sig_rx) = mpsc::channel();
        let (icon_tx, icon_rx) = mpsc::channel();
        TableCollector {
            ordered: Vec::new(),
            live: HashMap::new(),
            proc_metas: HashMap::new(),
            sig_tx,
            sig_rx,
            sig_pending: HashSet::new(),
            icons: HashMap::new(),
            icon_tx,
            icon_rx,
            icon_inflight: HashSet::new(),
            last_poll: Instant::now(),
            pending_poll: true,
        }
    }

    /// 查询 TCP/UDP 表并重建快照;表查询失败保留上一轮快照并输出错误
    fn poll(&mut self) {
        let tcp = match query_tcp() {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("[Collector] TCP 表查询失败: {e}");
                return;
            }
        };
        let udp = match query_udp() {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("[Collector] UDP 表查询失败: {e}");
                return;
            }
        };

        self.collect_signatures();
        self.collect_icons();

        let now = Instant::now();
        let old = std::mem::take(&mut self.live);
        let mut new_live: HashMap<ConnKey, Connection> =
            HashMap::with_capacity(tcp.len() + udp.len());
        // 名字兜底枚举按轮懒执行:存在路径反查失败的 PID 才枚举一次,行间复用
        let mut nt_names: Option<HashMap<u32, String>> = None;

        for key in tcp.into_iter().chain(udp) {
            let pid = key.pid;
            let id = key.id();
            let proto = key.proto;
            let local_addr = key.local_addr_ipv4();
            let local_port = key.local_port;
            let remote_port = key.remote_port;
            let remote_ip = key.remote_ip();
            let city = crate::net::geoip::locate(remote_ip).map(Place::Geo);
            let first_seen = old.get(&key).map_or(now, |c| c.first_seen);
            let meta = self.proc_meta(pid, &mut nt_names);
            new_live.insert(
                key,
                Connection {
                    id,
                    pid,
                    process: meta.name,
                    proc_path: meta.path,
                    signed: meta.signed,
                    proto,
                    local_addr,
                    local_port,
                    remote_ip,
                    remote_port,
                    city,
                    bytes_in: 0,
                    bytes_out: 0,
                    first_seen,
                },
            );
        }

        // 进程元数据缓存只保留本轮出现在连接表中的进程(PID 复用随行消失而自然失效)
        let live_pids: HashSet<u32> = new_live.keys().map(|k| k.pid).collect();
        self.proc_metas.retain(|pid, _| live_pids.contains(pid));

        self.dispatch_signature_queries(&live_pids);

        self.live = new_live;
        self.ordered = self.live.values().cloned().collect();
        // 表快照字节恒为 0:按连接建立时间倒序,最新连接在前
        self.ordered.sort_by_key(|c| {
            (
                std::cmp::Reverse(c.total_bytes()),
                std::cmp::Reverse(c.first_seen),
            )
        });
        self.last_poll = now;
    }

    /// 进程元数据(带缓存):优先反查完整映像路径(权限允许时),失败
    /// (服务进程 DACL 拒绝/非提权/刚退出)时以 NtQuerySystemInformation
    /// 全量名字快照兜底(nt_names 当轮懒枚举一次复用),拿名字但无路径
    /// (签名/图标依赖路径,维持未知);最终仍无名的行不缓存,下轮重查,
    /// 避免瞬时失败固化为整个连接存活期的"未知进程"
    fn proc_meta(&mut self, pid: u32, nt_names: &mut Option<HashMap<u32, String>>) -> ProcMeta {
        if pid == SYSTEM_PID {
            return ProcMeta {
                name: "System".to_owned(),
                path: None,
                signed: Signing::Unknown,
            };
        }
        if let Some(meta) = self.proc_metas.get(&pid) {
            return meta.clone();
        }
        let path = query_process_path(pid);
        let name = match &path {
            Some(p) => p.rsplit(['\\', '/']).next().unwrap_or_default().to_owned(),
            None => {
                let names = nt_names.get_or_insert_with(query_process_names);
                names.get(&pid).cloned().unwrap_or_default()
            }
        };
        let meta = ProcMeta {
            name,
            path,
            signed: Signing::Unknown,
        };
        if !meta.name.is_empty() {
            self.proc_metas.insert(pid, meta.clone());
        }
        meta
    }

    /// 收割已完成的签名查询结果回填缓存
    fn collect_signatures(&mut self) {
        while let Ok((pid, signed)) = self.sig_rx.try_recv() {
            self.sig_pending.remove(&pid);
            if let Some(meta) = self.proc_metas.get_mut(&pid) {
                meta.signed = signed;
            }
        }
    }

    /// 收割已完成的图标提取结果
    fn collect_icons(&mut self) {
        while let Ok((path, img)) = self.icon_rx.try_recv() {
            self.icon_inflight.remove(&path);
            self.icons.insert(path, IconState::Ready(img.map(Arc::new)));
        }
        // 未派发的 Pending 条目(超出上轮预算的)按上限补齐
        let budget = ICON_DISPATCH_PER_POLL.saturating_sub(self.icon_inflight.len());
        for path in self
            .icons
            .iter()
            .filter(|(_, s)| matches!(s, IconState::Pending))
            .map(|(p, _)| p.clone())
            .take(budget)
            .collect::<Vec<_>>()
        {
            self.icon_inflight.insert(path.clone());
            let tx = self.icon_tx.clone();
            std::thread::spawn(move || {
                let img = icon::extract(&path);
                let _ = tx.send((path, img));
            });
        }
    }

    /// 为缓存中签名未知的存活进程派发校验(限流:在途/每轮数量双重上限)
    fn dispatch_signature_queries(&mut self, live_pids: &HashSet<u32>) {
        let budget = SIG_MAX_INFLIGHT.saturating_sub(self.sig_pending.len());
        let candidates = self
            .proc_metas
            .iter()
            .filter(|(pid, meta)| {
                live_pids.contains(pid)
                    && meta.signed == Signing::Unknown
                    && meta.path.is_some()
                    && !self.sig_pending.contains(*pid)
            })
            .map(|(pid, meta)| (*pid, meta.path.clone().expect("path is some")))
            .take(budget.max(SIG_DISPATCH_PER_POLL.min(budget)))
            .collect::<Vec<_>>();
        for (pid, path) in candidates {
            if self.sig_pending.len() >= SIG_MAX_INFLIGHT {
                break;
            }
            self.sig_pending.insert(pid);
            let tx = self.sig_tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send((pid, signature::verify(&path)));
            });
        }
    }
}

impl Collector for TableCollector {
    fn snapshot(&mut self) -> Vec<Connection> {
        if self.pending_poll || self.last_poll.elapsed() >= POLL_INTERVAL {
            self.pending_poll = false;
            self.poll();
        }
        self.ordered.clone()
    }

    fn icon_image(&mut self, path: &str) -> IconState {
        match self.icons.get(path) {
            Some(IconState::Ready(img)) => IconState::Ready(img.clone()),
            Some(IconState::Pending) | None => {
                self.icons
                    .entry(path.to_owned())
                    .or_insert(IconState::Pending);
                IconState::Pending
            }
        }
    }

    fn kind(&self) -> CollectorKind {
        CollectorKind::Real
    }
}
