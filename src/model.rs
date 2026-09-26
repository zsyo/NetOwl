//! 数据模型:collector 产出、UI 绘制的最小完整单元。

use std::net::Ipv4Addr;
use std::time::Instant;

/// 传输层协议
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
}

impl Protocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Protocol::Tcp => "TCP",
            Protocol::Udp => "UDP",
        }
    }
}

/// 一条网络连接(进程 -> 远端),地理归属由 `city` 键索引到 [`crate::world`]。
#[derive(Clone, Debug)]
pub struct Connection {
    pub id: u64,
    pub pid: u32,
    pub process: String,
    pub proto: Protocol,
    pub remote_ip: Ipv4Addr,
    pub remote_port: u16,
    /// [`crate::world::CITIES`] 中的城市键
    pub city: &'static str,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub first_seen: Instant,
}

impl Connection {
    /// 累计流量(入 + 出)
    pub fn total_bytes(&self) -> u64 {
        self.bytes_in + self.bytes_out
    }

    /// 入站流量占主导时返回 true,用于连线配色
    pub fn inbound_dominant(&self) -> bool {
        self.bytes_in > self.bytes_out
    }
}

/// 字节数格式化(人类可读的 B/KB/MB/GB/TB)
pub fn fmt_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
