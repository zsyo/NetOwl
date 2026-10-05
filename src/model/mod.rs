//! 数据模型:collector 产出、UI 绘制的最小完整单元。

use std::net::Ipv4Addr;
use std::time::Instant;

/// 传输层协议
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

/// 连接归属地:地图节点与列表位置列的定位键
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Place {
    /// 静态演示城市([`crate::map::world::CITIES`] 键,模拟数据)
    City(&'static str),
    /// GeoIP 归属位置([`crate::net::geoip`] 位置表索引;中国到省级,其余到国家)
    Geo(u32),
}

/// 监听条目:TCP LISTEN / UDP 绑定端点(无远端语义,独立于连接快照)
#[derive(Clone, Debug)]
pub struct ListenEntry {
    pub pid: u32,
    pub process: String,
    /// 进程映像完整路径;受保护/系统进程等无法读取时为 None
    pub proc_path: Option<String>,
    pub proto: Protocol,
    pub local_addr: Ipv4Addr,
    pub local_port: u16,
}

/// 一条网络连接(进程 -> 远端)。
/// 地理归属由 `city` 键索引;归属未知(内网/保留段/未收录)时为 None,
/// 地图跳过该连接,连接列表位置列显示占位。
#[derive(Clone, Debug)]
pub struct Connection {
    pub id: u64,
    pub pid: u32,
    pub process: String,
    /// 进程映像完整路径;受保护/系统进程等无法读取时为 None
    pub proc_path: Option<String>,
    /// 进程映像签名状态(异步查询,回填前为 Unknown)
    pub signed: Signing,
    pub proto: Protocol,
    /// 本地地址(连接四元组的一部分;"结束连接"重建 MIB_TCPROW 用)
    pub local_addr: Ipv4Addr,
    /// 本地端口(连接四元组的一部分;弹窗"仅本次"临时规则用其精确
    /// 锁定单条连接)
    pub local_port: u16,
    pub remote_ip: Ipv4Addr,
    pub remote_port: u16,
    /// 归属地定位键;None 表示归属未知
    pub city: Option<Place>,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub first_seen: Instant,
}

/// 进程映像的 Authenticode 签名状态(WinVerifyTrust 校验)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signing {
    /// 签名有效
    Signed,
    /// 未签名
    Unsigned,
    /// 有签名但校验未通过(文件被篡改/证书失效等)
    Invalid,
    /// 无法确定:系统进程、文件不可访问或查询尚未完成
    Unknown,
}

impl Signing {
    /// 存储与比较用的规范字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            Signing::Signed => "signed",
            Signing::Unsigned => "unsigned",
            Signing::Invalid => "invalid",
            Signing::Unknown => "unknown",
        }
    }
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

    /// 远端地址展示文本;UDP 表行无远端语义(0.0.0.0:0)以 * 占位
    pub fn remote_display(&self) -> String {
        if self.proto == Protocol::Udp && self.remote_ip.is_unspecified() {
            "*:*".to_owned()
        } else {
            format!("{}:{}", self.remote_ip, self.remote_port)
        }
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
