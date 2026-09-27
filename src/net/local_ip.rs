//! 本机公网 IP 探测:并发请求多个知名公共回显接口,取最先返回的合法 IPv4
//! (任一接口失效不影响探测)。手写最小 HTTP/1.1 GET(std TcpStream,不走
//! 系统代理),全部接口为 http 明文端点以避免引入 TLS 依赖;探测结果为公网
//! 视角的出口 IP(代理环境即代理出口)。全部失败时调用方回退默认点位。

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream, ToSocketAddrs};
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
];

/// 单请求连接/读写超时
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);

/// 周期探测器:每轮 begin_round 后,首个到达的结果经 poll 送达,其余丢弃
pub struct Probe {
    rx: Receiver<(Ipv4Addr, &'static str)>,
    round_done: bool,
}

impl Probe {
    /// 创建并立即发起第一轮探测
    pub fn new() -> Self {
        let probe = Probe {
            rx: mpsc::channel().1,
            round_done: false,
        };
        let mut probe = probe;
        probe.begin_round();
        probe
    }

    /// 发起一轮探测:每个接口一个线程并发请求
    pub fn begin_round(&mut self) {
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

    /// 非阻塞读取本轮探测结果:返回首个成功结果,后续结果静默丢弃
    pub fn poll(&mut self) -> Option<(Ipv4Addr, &'static str)> {
        loop {
            match self.rx.try_recv() {
                Ok(hit) => {
                    if !self.round_done {
                        self.round_done = true;
                        return Some(hit);
                    }
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return None,
            }
        }
    }
}

/// 单接口探测:GET -> 响应体提取首个合法 IPv4
fn probe_url(url: &'static str) -> Option<Ipv4Addr> {
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
    stream.read_to_end(&mut buf).ok()?;

    // 跳过 HTTP 头取响应体(头中可能出现无关数字)
    let text = String::from_utf8_lossy(&buf);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or(&text);
    extract_ipv4(body)
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
