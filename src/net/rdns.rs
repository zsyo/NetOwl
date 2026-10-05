//! rDNS 域名解析:对活跃连接的远端 IP 做异步 PTR 查询(getnameinfo),
//! 连接列表与地图信息卡优先显示域名,无记录时回退裸 IP。
//!
//! getnameinfo 无超时且可能阻塞数秒,查询全部在独立线程执行;update()
//! 非阻塞收割结果、按限流节奏派发,不拖累 UI。成功与失败(无 PTR/查询
//! 出错)分别按 TTL 缓存,到期后对仍活跃的连接重查。

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::Ipv4Addr;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use windows::Win32::Networking::WinSock::{
    AF_INET, IN_ADDR, IN_ADDR_0, NI_NAMEREQD, SOCKADDR, SOCKADDR_IN, WSADATA, WSAStartup,
    getnameinfo, socklen_t,
};

use crate::model::Connection;

/// 并发查询上限:极端情况线程滞留占满槽位即暂停派发,不拖累界面
const MAX_INFLIGHT: usize = 8;
/// 派发节奏:每个间隔最多派发 DISPATCH_BATCH 个,新连接涌入不瞬时打满 DNS
const DISPATCH_INTERVAL: Duration = Duration::from_millis(250);
const DISPATCH_BATCH: usize = 2;
/// 成功结果缓存时长
const HOST_TTL: Duration = Duration::from_secs(10 * 60);
/// 失败结果缓存时长(临时 DNS 故障到期重试)
const NEG_TTL: Duration = Duration::from_secs(2 * 60);

/// 缓存条目;name 为 None 表示已查无 PTR(负缓存)
struct Entry {
    name: Option<String>,
    at: Instant,
}

impl Entry {
    fn expired(&self, now: Instant) -> bool {
        let ttl = if self.name.is_some() {
            HOST_TTL
        } else {
            NEG_TTL
        };
        now.duration_since(self.at) >= ttl
    }
}

/// 异步 PTR 解析器:update() 喂入当前活跃连接并收割结果,lookup() 查缓存
pub struct Rdns {
    /// 查询线程回报通道(发送端克隆给每个查询线程)
    tx: mpsc::Sender<(Ipv4Addr, Option<String>)>,
    rx: Receiver<(Ipv4Addr, Option<String>)>,
    /// 已派发未返回的 IP(getnameinfo 可能阻塞数秒,防重复派发)
    pending: HashSet<Ipv4Addr>,
    /// 待派发队列(活跃且缓存过期的 IP)
    queue: VecDeque<Ipv4Addr>,
    /// 队列成员集合(与 queue 同步维护,查重免线性扫描)
    queued: HashSet<Ipv4Addr>,
    cache: HashMap<Ipv4Addr, Entry>,
    last_dispatch: Instant,
}

impl Rdns {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Rdns {
            tx,
            rx,
            pending: HashSet::new(),
            queue: VecDeque::new(),
            queued: HashSet::new(),
            cache: HashMap::new(),
            last_dispatch: Instant::now(),
        }
    }

    /// 每帧调用:收割完成结果,为仍活跃但缓存过期的 IP 重新派发查询
    pub fn update(&mut self, conns: &[Connection]) {
        while let Ok((ip, name)) = self.rx.try_recv() {
            self.pending.remove(&ip);
            // 负缓存(失败 2min)天然节流:同一 IP 在 TTL 内不重复查询
            if name.is_none() {
                tracing::debug!("[Rdns] PTR 解析失败:{ip}");
            }
            self.cache.insert(
                ip,
                Entry {
                    name,
                    at: Instant::now(),
                },
            );
        }

        let now = Instant::now();
        let wanted: HashSet<Ipv4Addr> = conns
            .iter()
            .map(|c| c.remote_ip)
            .filter(is_queryable)
            .collect();

        // 连接已消失的不再派发;不活跃且过期的缓存释放,防止无限积累
        self.queue.retain(|ip| wanted.contains(ip));
        self.queued.retain(|ip| wanted.contains(ip));
        self.cache
            .retain(|ip, e| wanted.contains(ip) || !e.expired(now));

        for ip in &wanted {
            let stale = match self.cache.get(ip) {
                Some(e) => e.expired(now),
                None => true,
            };
            if stale && !self.pending.contains(ip) && !self.queued.contains(ip) {
                self.queued.insert(*ip);
                self.queue.push_back(*ip);
            }
        }

        if self.last_dispatch.elapsed() < DISPATCH_INTERVAL {
            return;
        }
        let mut dispatched = 0;
        while self.pending.len() < MAX_INFLIGHT
            && dispatched < DISPATCH_BATCH
            && let Some(ip) = self.queue.pop_front()
        {
            ensure_winsock();
            self.queued.remove(&ip);
            self.pending.insert(ip);
            dispatched += 1;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send((ip, ptr_query(ip)));
            });
        }
        if dispatched > 0 {
            self.last_dispatch = now;
        }
    }

    /// 已知域名;未查/无 PTR 返回 None(调用方回退裸 IP 显示)
    pub fn lookup(&self, ip: Ipv4Addr) -> Option<&str> {
        self.cache.get(&ip).and_then(|e| e.name.as_deref())
    }
}

/// 域名 + 端口的显示文本;超长 PTR 记录(CNAME 链可达数十级)按
/// max_chars 截断,避免撑爆列表列宽与信息卡
pub fn display(host: &str, port: u16, max_chars: usize) -> String {
    if host.chars().count() <= max_chars {
        format!("{host}:{port}")
    } else {
        let cut: String = host.chars().take(max_chars).collect();
        format!("{cut}…:{port}")
    }
}

/// 可发起 PTR 查询的地址:排除回环/私网/链路本地等非公网段
/// (240/4 保留段手工判断,Ipv4Addr::is_reserved 尚未稳定)
fn is_queryable(ip: &Ipv4Addr) -> bool {
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.octets()[0] >= 240)
}

/// WinSock 初始化(getnameinfo 前置);进程内仅一次,失败显式报错
fn ensure_winsock() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let mut data = WSADATA::default();
        let rc = unsafe { WSAStartup(0x0202, &mut data) };
        if rc != 0 {
            panic!("[Rdns] WSAStartup 失败: code {rc}");
        }
    });
}

/// PTR 查询:无 PTR/查询失败返回 None(经系统 DNS 客户端,自带缓存)
fn ptr_query(ip: Ipv4Addr) -> Option<String> {
    let addr = SOCKADDR_IN {
        sin_family: AF_INET,
        sin_port: 0,
        sin_addr: IN_ADDR {
            S_un: IN_ADDR_0 {
                S_addr: u32::from_be_bytes(ip.octets()),
            },
        },
        sin_zero: [0; 8],
    };
    // NI_NAMEREQD:解析失败不回退返回点分 IP,由调用方统一回退显示
    let mut host = [0u8; 1025];
    let rc = unsafe {
        getnameinfo(
            &addr as *const SOCKADDR_IN as *const SOCKADDR,
            socklen_t(std::mem::size_of::<SOCKADDR_IN>() as i32),
            Some(&mut host),
            None,
            NI_NAMEREQD as i32,
        )
    };
    if rc != 0 {
        return None;
    }
    let len = host.iter().position(|&b| b == 0).unwrap_or(host.len());
    let name = String::from_utf8_lossy(&host[..len]).into_owned();
    if name.is_empty() { None } else { Some(name) }
}
