//! 域名正向解析:对启用中的域名规则异步查询 A 记录(getaddrinfo),
//! 解析出的 IP 集供 WFP 翻译展开为内核过滤器——域名条件无法静态翻译,
//! 由"后台解析 + 变化时同步过滤器"补齐内核拦截(求值标注已另支持)。
//!
//! getaddrinfo 无超时且可能阻塞数秒,查询全部在独立线程执行;update()
//! 非阻塞收割并按限流节奏派发,不拖累 UI。enqueue() 幂等:已有未过期
//! 缓存或在途/在队即跳过,调用方每秒全量入队启用中的域名即可,TTL
//! 重解析由此自然发生,无需额外调度。

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{Ipv4Addr, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

/// 并发查询上限:极端情况线程滞留占满槽位即暂停派发,不拖累界面
const MAX_INFLIGHT: usize = 8;
/// 派发节奏:每个间隔最多派发 DISPATCH_BATCH 个,域名集变化不瞬时打满 DNS
const DISPATCH_INTERVAL: Duration = Duration::from_millis(250);
const DISPATCH_BATCH: usize = 2;
/// 成功结果缓存时长(到期后由调用方下一轮 enqueue 触发重解析)
const HOST_TTL: Duration = Duration::from_secs(10 * 60);
/// 失败结果缓存时长(域名不存在/无 A 记录/临时 DNS 故障,到期重试)
const NEG_TTL: Duration = Duration::from_secs(2 * 60);

/// 缓存条目;Some 且非空才算成功,空集与 None 都是"无可用 IP"
/// (None = 尚未解析,空集 = 已解析但无结果),二者 TTL 不同
struct Entry {
    ips: Option<Vec<Ipv4Addr>>,
    at: Instant,
}

impl Entry {
    fn expired(&self, now: Instant) -> bool {
        let ok = self.ips.as_ref().is_some_and(|v| !v.is_empty());
        let ttl = if ok { HOST_TTL } else { NEG_TTL };
        now.duration_since(self.at) >= ttl
    }
}

/// 异步域名解析器:enqueue() 入队、update() 收割派发、lookup() 查缓存
pub struct Resolver {
    /// 查询线程回报通道(发送端克隆给每个查询线程)
    tx: mpsc::Sender<(String, Vec<Ipv4Addr>)>,
    rx: Receiver<(String, Vec<Ipv4Addr>)>,
    /// 已派发未返回的域名(getaddrinfo 可能阻塞数秒,防重复派发)
    pending: HashSet<String>,
    /// 待派发队列
    queue: VecDeque<String>,
    /// 队列成员集合(与 queue 同步维护,查重免线性扫描)
    queued: HashSet<String>,
    cache: HashMap<String, Entry>,
    last_dispatch: Instant,
}

impl Resolver {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Resolver {
            tx,
            rx,
            pending: HashSet::new(),
            queue: VecDeque::new(),
            queued: HashSet::new(),
            cache: HashMap::new(),
            last_dispatch: Instant::now(),
        }
    }

    /// 入队一个域名(键统一小写,DNS 大小写不敏感)。幂等:已有未过期
    /// 缓存或在途/在队即跳过;缓存过期后重复入队即触发重解析
    pub fn enqueue(&mut self, domain: &str) {
        let domain = domain.to_lowercase();
        let now = Instant::now();
        if let Some(e) = self.cache.get(&domain)
            && !e.expired(now)
        {
            return;
        }
        if self.pending.contains(&domain) || self.queued.contains(&domain) {
            return;
        }
        self.queued.insert(domain.clone());
        self.queue.push_back(domain);
    }

    /// 每帧调用:收割完成结果,按限流节奏派发查询线程
    pub fn update(&mut self) {
        while let Ok((domain, ips)) = self.rx.try_recv() {
            self.pending.remove(&domain);
            if ips.is_empty() {
                tracing::debug!("[Resolve] {domain} 无 A 记录/解析失败");
            } else {
                tracing::debug!("[Resolve] {domain} -> {} 个 IP", ips.len());
            }
            self.cache.insert(
                domain,
                Entry {
                    ips: Some(ips),
                    at: Instant::now(),
                },
            );
        }

        if self.last_dispatch.elapsed() < DISPATCH_INTERVAL {
            return;
        }
        // 按实际经过的间隔数补偿批量(与 rdns::update 同理),消化速率
        // 不受重绘节奏钳制;在途上限仍由 MAX_INFLIGHT 兜底
        let budget = DISPATCH_BATCH
            * (self.last_dispatch.elapsed().as_millis() / DISPATCH_INTERVAL.as_millis()) as usize;
        let mut dispatched = 0;
        while self.pending.len() < MAX_INFLIGHT
            && dispatched < budget
            && let Some(domain) = self.queue.pop_front()
        {
            self.queued.remove(&domain);
            self.pending.insert(domain.clone());
            dispatched += 1;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let ips = a_records(&domain);
                let _ = tx.send((domain, ips));
            });
        }
        if dispatched > 0 {
            self.last_dispatch = Instant::now();
        }
    }

    /// 已解析的 IP 集;None = 尚未有结果(在途/未入队),空切片 = 已知
    /// 无 A 记录或解析失败(负缓存)。调用方对两者同样跳过 WFP 展开
    pub fn lookup(&self, domain: &str) -> Option<&[Ipv4Addr]> {
        self.cache
            .get(&domain.to_lowercase())
            .and_then(|e| e.ips.as_deref())
    }

    /// 清理不再需要的域名:规则删除/停用后队列与缓存同步收缩(在途
    /// 查询无法取消,结果回收后下一轮 prune 一并清除),防跨配置档积累
    pub fn prune(&mut self, wanted: &HashSet<String>) {
        self.queue.retain(|d| wanted.contains(d));
        self.queued.retain(|d| wanted.contains(d));
        self.pending.retain(|d| wanted.contains(d));
        self.cache.retain(|d, _| wanted.contains(d));
    }
}

/// A 记录查询:经系统解析器(getaddrinfo,自带 OS 级缓存),收集全部
/// IPv4 结果并排序去重;失败返回空集(负缓存统一处理)。IPv6 结果
/// 忽略,AAAA 支持随 IPv6 全链路另加
fn a_records(domain: &str) -> Vec<Ipv4Addr> {
    let Ok(addrs) = (domain, 0).to_socket_addrs() else {
        return Vec::new();
    };
    let mut ips: Vec<Ipv4Addr> = addrs
        .filter_map(|a| match a.ip() {
            std::net::IpAddr::V4(v4) => Some(v4),
            std::net::IpAddr::V6(_) => None,
        })
        .collect();
    ips.sort_unstable();
    ips.dedup();
    ips
}
