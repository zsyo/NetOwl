//! 网络质量采样:对活跃连接的公网远端做 ICMP 延迟探测(IcmpSendEcho,
//! 系统代发 raw ICMP,普通用户权限即可),供地图节点健康度着色与端点
//! 详情展示。探测在独立线程执行,update() 非阻塞收割;限流节奏与
//! rDNS/域名解析一致,目标 60s TTL 内复用结果。
//!
//! 延迟口径:ICMP RTT 的 EMA 平滑 + 最近窗口丢包率。ICMP 被目标禁用时
//! 无数据(节点按未知显示);带 TCP 端口的目标(用户显式添加的自定义
//! 目标)单轮内 ICMP 无响应即回退 TCP 握手计时补测——connect 全程计时
//! 含协议栈开销,数值略高于 ICMP RTT,且会向目标发起一次真实连接。

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use windows::Win32::NetworkManagement::IpHelper::{
    ICMP_ECHO_REPLY, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho,
};

/// 并发探测上限:极端情况线程滞留占满槽位即暂停派发,不拖累界面
const MAX_INFLIGHT: usize = 4;
/// 派发节奏:每个间隔最多派发 DISPATCH_BATCH 个
const DISPATCH_INTERVAL: Duration = Duration::from_millis(250);
const DISPATCH_BATCH: usize = 2;
/// 结果复用时长(到期后由调用方下一轮 enqueue 触发重探);展示侧以
/// 两倍时长判定样本是否过期
pub(crate) const SAMPLE_TTL: Duration = Duration::from_secs(60);
/// 丢包统计窗口(最近 N 轮探测)
const LOSS_WINDOW: usize = 20;
/// 单轮探测超时;超时计入丢包
const PROBE_TIMEOUT: Duration = Duration::from_secs(1);
/// ICMP RTT 的 EMA 平滑系数(新样本权重)
const EMA_ALPHA: f32 = 0.3;

/// 单目标采样快照(对外只读视图)
#[derive(Clone, Copy)]
pub struct Sample {
    /// 平滑 RTT(毫秒);None = 窗口内尚无成功样本
    pub rtt: Option<u32>,
    /// 最近窗口丢包率(0-100)
    pub loss: u8,
    /// 最近探测时刻(调用方据此判读新鲜度)
    pub at: Instant,
}

impl Sample {
    /// 健康档位(1 = 良好 <100ms,2 = 较慢 <300ms,3 = 拥塞;丢包率
    /// >10% 降一档);None = 无有效延迟样本(从未成功,按未知显示,
    /// 不参与聚合取最差——禁 ping 目标不应抹掉同端点的正常样本)
    pub fn rank(&self) -> Option<u8> {
        let ms = self.rtt?;
        let base = if ms < 100 {
            1
        } else if ms < 300 {
            2
        } else {
            3
        };
        Some((base + u8::from(self.loss > 10)).min(3))
    }
}

/// 单目标采样状态(内部)
struct Inner {
    rtt: Option<u32>,
    window: VecDeque<bool>,
    at: Instant,
}

impl Inner {
    fn loss(&self) -> u8 {
        let n = self.window.len();
        if n == 0 {
            return 0;
        }
        let ok = self.window.iter().filter(|&&ok| ok).count();
        ((n - ok) * 100 / n) as u8
    }
}

/// 异步质量采样器:enqueue() 入队目标、update() 收割派发、lookup() 查快照
pub struct Quality {
    /// 探测线程回报通道(发送端克隆给每个探测线程)
    tx: mpsc::Sender<(Ipv4Addr, Option<u32>)>,
    rx: Receiver<(Ipv4Addr, Option<u32>)>,
    /// 已派发未返回的目标(探测可能阻塞到超时,防重复派发)
    pending: HashSet<Ipv4Addr>,
    /// 待派发队列
    queue: VecDeque<Ipv4Addr>,
    /// 队列成员集合(与 queue 同步维护,查重免线性扫描)
    queued: HashSet<Ipv4Addr>,
    /// 各目标的 TCP 补测端口(None = 仅 ICMP);自动目标恒 None,
    /// 自定义目标覆盖,同 IP 双重入队时后者胜
    tcp_ports: HashMap<Ipv4Addr, Option<u16>>,
    samples: HashMap<Ipv4Addr, Inner>,
    last_dispatch: Instant,
}

impl Quality {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Quality {
            tx,
            rx,
            pending: HashSet::new(),
            queue: VecDeque::new(),
            queued: HashSet::new(),
            tcp_ports: HashMap::new(),
            samples: HashMap::new(),
            last_dispatch: Instant::now(),
        }
    }

    /// 入队一个目标(幂等:60s 内已有采样或在途/在队即跳过)。
    /// tcp_port 传 Some 的目标(ICMP 无响应时)单轮回退 TCP 握手计时;
    /// 调用方按节流全量入队应持续采样的目标即可
    pub fn enqueue(&mut self, ip: Ipv4Addr, tcp_port: Option<u16>) {
        let now = Instant::now();
        if let Some(s) = self.samples.get(&ip)
            && now.duration_since(s.at) < SAMPLE_TTL
        {
            return;
        }
        if self.pending.contains(&ip) || self.queued.contains(&ip) {
            self.tcp_ports.insert(ip, tcp_port);
            return;
        }
        self.queued.insert(ip);
        self.queue.push_back(ip);
        self.tcp_ports.insert(ip, tcp_port);
    }

    /// 每帧调用:收割完成结果,按限流节奏派发探测线程
    pub fn update(&mut self) {
        while let Ok((ip, rtt)) = self.rx.try_recv() {
            self.pending.remove(&ip);
            let s = self.samples.entry(ip).or_insert(Inner {
                rtt: None,
                window: VecDeque::new(),
                at: Instant::now(),
            });
            s.at = Instant::now();
            s.window.push_back(rtt.is_some());
            if s.window.len() > LOSS_WINDOW {
                s.window.pop_front();
            }
            if let Some(ms) = rtt {
                s.rtt = Some(match s.rtt {
                    Some(prev) => (prev as f32 * (1.0 - EMA_ALPHA) + ms as f32 * EMA_ALPHA) as u32,
                    None => ms,
                });
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
            self.queued.remove(&ip);
            self.pending.insert(ip);
            let tcp_port = self.tcp_ports.get(&ip).copied().flatten();
            dispatched += 1;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let (rtt, via) = match icmp_probe(ip) {
                    Some(ms) => (Some(ms), "icmp"),
                    None => match tcp_port.and_then(|port| tcp_probe(ip, port)) {
                        Some(ms) => (Some(ms), "tcp"),
                        None => (None, "icmp"),
                    },
                };
                match rtt {
                    Some(ms) => tracing::debug!("[Quality] {ip} {via} {ms}ms"),
                    None => tracing::debug!("[Quality] {ip} 无响应"),
                }
                let _ = tx.send((ip, rtt));
            });
        }
        if dispatched > 0 {
            self.last_dispatch = Instant::now();
        }
    }

    /// 目标的采样快照;从未探测返回 None(有样本即可读,rtt 可能为 None)
    pub fn lookup(&self, ip: Ipv4Addr) -> Option<Sample> {
        self.samples.get(&ip).map(|s| Sample {
            rtt: s.rtt,
            loss: s.loss(),
            at: s.at,
        })
    }

    /// 窗口尾部连续失败轮数(自定义目标告警判定:达到阈值触发一次,
    /// 尾部重新出现成功即归零)
    pub fn consec_fails(&self, ip: Ipv4Addr) -> usize {
        self.samples
            .get(&ip)
            .map(|s| s.window.iter().rev().take_while(|&&ok| !ok).count())
            .unwrap_or(0)
    }

    /// 清理不再需要的目标(队列/在途/采样同步收缩);wanted 须包含全部
    /// 应持续采样的目标(活跃连接远端 + 自定义目标)。在途探测无法取消,
    /// 结果回收后下一轮 prune 一并清除
    pub fn prune(&mut self, wanted: &HashSet<Ipv4Addr>) {
        self.queue.retain(|ip| wanted.contains(ip));
        self.queued.retain(|ip| wanted.contains(ip));
        self.pending.retain(|ip| wanted.contains(ip));
        self.tcp_ports.retain(|ip, _| wanted.contains(ip));
        self.samples.retain(|ip, _| wanted.contains(ip));
    }
}

/// ICMP echo 探测(同步,单轮);RTT 毫秒,超时/失败/禁 ping 返回 None
fn icmp_probe(ip: Ipv4Addr) -> Option<u32> {
    // 每轮独立创建/关闭句柄:频率受派发限流(约 8 次/秒),开销可忽略
    let handle = unsafe { IcmpCreateFile() }.ok()?;
    // IPAddr 为网络序内存表示:按 BE 字节组装
    let dest = u32::from_be_bytes(ip.octets());
    /// ICMP 请求负载(8 字节,与 ping 默认一致)
    const REQUEST_LEN: usize = 8;
    let request = [0u8; REQUEST_LEN];
    let mut reply = [0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + REQUEST_LEN];
    let n = unsafe {
        IcmpSendEcho(
            handle,
            dest,
            request.as_ptr().cast(),
            request.len() as u16,
            None,
            reply.as_mut_ptr().cast(),
            reply.len() as u32,
            PROBE_TIMEOUT.as_millis() as u32,
        )
    };
    // Status 0 = IP_SUCCESS;RoundTripTime 为内核记录的毫秒数
    let rtt = (n > 0)
        .then(|| unsafe { &*(reply.as_ptr() as *const ICMP_ECHO_REPLY) })
        .filter(|r| r.Status == 0)
        .map(|r| r.RoundTripTime);
    let _ = unsafe { IcmpCloseHandle(handle) };
    rtt
}

/// TCP 握手计时(connect 全程;会与目标建立并立即关闭一次真实连接)
fn tcp_probe(ip: Ipv4Addr, port: u16) -> Option<u32> {
    let t0 = Instant::now();
    TcpStream::connect_timeout(&SocketAddr::from((ip, port)), PROBE_TIMEOUT)
        .ok()
        .map(|_| t0.elapsed().as_millis() as u32)
}
