//! 本机公网 IP 探测:并发请求多个知名公共回显接口(IPv4 与 IPv6 端点
//! 并行),取最先返回的合法 IPv4,全部失败时回退首个 IPv6 结果(本机
//! 地图点位以 v4 为准,与连接主流族一致;任一接口失效不影响探测)。
//! 手写最小 HTTP/1.1 GET(std TcpStream,不走系统代理),全部接口为
//! http 明文端点以避免引入 TLS 依赖;探测结果为公网视角的出口 IP
//! (代理环境即代理出口)。全部失败时调用方回退默认点位。
//! NETOWL_IP 环境变量可指定固定本机 IP(v4/v6 均可;演示/截图场景
//! 隐藏真实位置):非空合法地址生效,全程不发起任何真实探测;
//! 非法值告警后按未指定处理。

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

/// 探测接口(知名公共回显服务,http 明文)
const PROBE_URLS: &[&str] = &[
    "http://api.ipify.org",
    "http://ipv4.icanhazip.com",
    "http://checkip.amazonaws.com",
    "http://myip.ipip.net",
    "http://cip.cc",
    "http://members.3322.org/dyndns/getip",
    "http://api6.ipify.org",
    "http://ipv6.icanhazip.com",
    "http://v6.ident.me",
];

/// 单请求连接/读写超时
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);

/// NETOWL_IP 指定固定本机 IP 时,poll 送达结果的来源标注
const PINNED_SOURCE: &str = "NETOWL_IP";

/// 周期探测器:每轮 begin_round 后,首个到达的 IPv4 结果经 poll 送达;
/// v4 全部失败(接口线程全部收尾)时回退首个 IPv6 结果,其余丢弃
pub struct Probe {
    rx: Receiver<(IpAddr, &'static str)>,
    round_done: bool,
    /// IPv6 结果暂存:v4 优先送达,接口线程全部收尾仍无 v4 才回退
    v6_pending: Option<(IpAddr, &'static str)>,
    /// NETOWL_IP 固定 IP 模式:不发起任何真实探测(含 10 分钟周期重探)
    pinned: bool,
}

impl Probe {
    /// 创建并立即发起第一轮探测;NETOWL_IP 为非空合法 IPv4 时改为固定 IP
    /// 模式:指定值经通道送达一次即断流,不发起任何真实探测
    pub fn new() -> Self {
        let mut probe = Probe {
            rx: mpsc::channel().1,
            // 初值 true:首轮 begin_round 不做"上轮全失败"判定
            round_done: true,
            v6_pending: None,
            pinned: false,
        };
        match env_pinned_ip() {
            Some(ip) => {
                let (tx, rx) = mpsc::channel();
                // 接收端随本结构存活,发送必达;round_done 复位 false 让
                // poll 可送达该值(送达后通道断流,后续 poll 恒 None)
                let _ = tx.send((ip, PINNED_SOURCE));
                probe.rx = rx;
                probe.pinned = true;
                probe.round_done = false;
            }
            None => probe.begin_round(),
        }
        probe
    }

    /// 发起一轮探测:每个接口一个线程并发请求;固定 IP 模式下为 no-op
    pub fn begin_round(&mut self) {
        if self.pinned {
            return;
        }
        if !self.round_done {
            tracing::debug!(
                "[LocalIp] 上一轮 {} 个回显接口全部探测失败,本机点位维持不变",
                PROBE_URLS.len()
            );
        }
        // 先 drain 旧通道再替换:上一轮 6 个接口都慢(超过 10 分钟周期)时,
        // 直接换 Receiver 会丢弃全部在途结果,round_done 永远为 false,
        // 本机点位无限期停留在旧值且每轮都报"全部失败"
        while self.rx.try_recv().is_ok() {}
        let (tx, rx) = mpsc::channel();
        self.rx = rx;
        self.round_done = false;
        for url in PROBE_URLS {
            let tx = tx.clone();
            std::thread::spawn(move || {
                if let Some(ip) = probe_url(url) {
                    let _ = tx.send((ip, url));
                }
            });
        }
    }

    /// 非阻塞读取本轮探测结果:首个 IPv4 结果优先送达,后续结果静默
    /// 丢弃;接口线程全部收尾后仍无 v4 才回退暂存的 v6 结果
    pub fn poll(&mut self) -> Option<(IpAddr, &'static str)> {
        loop {
            match self.rx.try_recv() {
                Ok(hit) => {
                    if !self.round_done {
                        if hit.0.is_ipv4() {
                            self.round_done = true;
                            return Some(hit);
                        }
                        self.v6_pending.get_or_insert(hit);
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    if !self.round_done {
                        self.round_done = true;
                        return self.v6_pending.take();
                    }
                    return None;
                }
                Err(TryRecvError::Empty) => return None,
            }
        }
    }
}

/// 单接口探测:GET -> 响应体提取首个合法地址(v4 端点返回 v4,v6
/// 端点返回 v6;族由端点决定,不在本函数区分)
fn probe_url(url: &'static str) -> Option<IpAddr> {
    let rest = url.strip_prefix("http://")?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    // 域名解析后逐地址尝试连接,任一成功即用
    let addrs: Vec<_> = (host, 80).to_socket_addrs().ok()?.collect();
    let mut stream = None;
    for addr in addrs {
        if let Ok(s) = TcpStream::connect_timeout(&addr, REQUEST_TIMEOUT) {
            stream = Some(s);
            break;
        }
    }
    let mut stream = stream?;
    let _ = stream.set_read_timeout(Some(REQUEST_TIMEOUT));
    let _ = stream.set_write_timeout(Some(REQUEST_TIMEOUT));
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: NetOwl/0.1\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).ok()?;
    let mut buf = Vec::new();
    // 响应体只是单行 IP,64 KiB 上限防异常大响应;读失败(超时/提前
    // 关闭)时已读部分仍尝试解析,Connection: close 下常为发完即断
    let _ = stream.take(64 * 1024).read_to_end(&mut buf);
    if buf.is_empty() {
        return None;
    }

    // 跳过 HTTP 头取响应体(头中可能出现无关数字)
    let text = String::from_utf8_lossy(&buf);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or(&text);
    extract_ip(body)
}

/// 读取 NETOWL_IP(启动时一次):非空合法 IPv4 视作固定本机 IP;
/// 非法值告警并视作未指定,空值/未设置静默按未指定处理
fn env_pinned_ip() -> Option<IpAddr> {
    let Ok(raw) = std::env::var("NETOWL_IP") else {
        return None;
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    match raw.parse::<IpAddr>() {
        Ok(ip) => Some(ip),
        Err(_) => {
            tracing::warn!("[LocalIp] NETOWL_IP=\"{raw}\" 不是合法 IP 地址,忽略,继续真实探测");
            None
        }
    }
}

/// 从文本中提取首个合法地址:v6 裸回显整体即地址,逐 token 找 v6 形状
/// (含冒号且可解析),最后回退 v4 点分提取
fn extract_ip(text: &str) -> Option<IpAddr> {
    if let Ok(v6) = text.trim().parse::<Ipv6Addr>() {
        return Some(IpAddr::V6(v6));
    }
    for token in text.split(|c: char| c.is_whitespace() || c == ',') {
        if token.contains(':')
            && let Ok(v6) = token.parse::<Ipv6Addr>()
        {
            return Some(IpAddr::V6(v6));
        }
    }
    extract_ipv4(text).map(IpAddr::V4)
}

/// 从文本中提取第一个形如 a.b.c.d 且各段 0-255 的 IPv4
fn extract_ipv4(text: &str) -> Option<Ipv4Addr> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
            i += 1;
        }
        let token = &text[start..i];
        let octets: Vec<Option<u8>> = token.split('.').map(|o| o.parse().ok()).collect();
        let valid = octets.len() == 4
            && octets.iter().all(|o| o.is_some())
            && !token.contains("..")
            && !token.ends_with('.');
        if let (true, Some(parts)) = (valid, octets.into_iter().collect::<Option<Vec<u8>>>()) {
            return Some(Ipv4Addr::new(parts[0], parts[1], parts[2], parts[3]));
        }
    }
    None
}
