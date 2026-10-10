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

/// 完整路径取映像名(Windows 反斜杠或 Unix 斜杠分隔的最后一段);
/// 表快照与 ETW 短命连接共用
pub fn image_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
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
    /// ETW 真实发起方向:Some(true) = 本机发起(connect)、Some(false) =
    /// 对端连入(accept);None = 未知(未提权/未合并)——方向判定
    /// 优先取本字段,未知时按远端端口近似(rules::conn_direction)
    pub initiated_out: Option<bool>,
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

/// 大数字计数单位制:中文四位分节(万/亿/万亿/亿亿)或国际三位分节
/// (K/M/B/T)。中文用户普遍不习惯 K/M/B 三位计数,按界面语言默认,
/// 设置页可覆盖
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CountUnits {
    /// 国际三位分节:K/M/B/T
    Western,
    /// 中文四位分节:万/亿/万亿/亿亿
    Chinese,
}

/// 计数紧凑格式(命中计数等定宽数字列用):最小单位以下(西文 1000 /
/// 中文 10000)精确,以上按单位制紧凑——100 以下一位小数(去尾零),
/// 100 以上取整。档位顶部四舍五入会产出 5 位整数(如 9_999_999_999_999_999
/// 万亿 -> "10000万亿"),此时进位到更大单位(值略小于 1,显示 "1亿亿");
/// 最大单位仍超宽(仅西文 T 档的不可达值)则封顶。任何输出不超过
/// 4 半角数字 + 2 全角单位;精确值由调用方悬停展示
pub fn fmt_count(n: u64, units: CountUnits) -> String {
    let tiers: &[(u64, &str)] = match units {
        CountUnits::Western => &[
            (1_000_000_000_000, "T"),
            (1_000_000_000, "B"),
            (1_000_000, "M"),
            (1_000, "K"),
        ],
        CountUnits::Chinese => &[
            (10_000_000_000_000_000, "亿亿"),
            (1_000_000_000_000, "万亿"),
            (100_000_000, "亿"),
            (10_000, "万"),
        ],
    };
    // 最小单位以下:精确原数
    let base = tiers.last().map_or(u64::MAX, |&(d, _)| d);
    if n < base {
        return n.to_string();
    }
    for (i, &(div, unit)) in tiers.iter().enumerate() {
        if n < div {
            continue;
        }
        let v = n as f64 / div as f64;
        let value = if v >= 100.0 {
            format!("{v:.0}")
        } else {
            // 去尾零必须在拼单位之前(单位字符在尾部,对整个串
            // trim_end_matches(".0") 永远匹配不到)
            format!("{v:.1}").trim_end_matches(".0").to_owned()
        };
        if value.len() <= 4 {
            return format!("{value}{unit}");
        }
        // 5 位整数:进位到更大单位(tiers 降序,前一项即更大单位;其值
        // 略小于 1,一位小数后为 "1X");已是最大单位则封顶防爆宽
        if i > 0 {
            let (bigger_div, bigger_unit) = tiers[i - 1];
            let v2 = n as f64 / bigger_div as f64;
            let s2 = format!("{v2:.1}").trim_end_matches(".0").to_owned();
            return format!("{s2}{bigger_unit}");
        }
        return format!("9999{unit}+");
    }
    n.to_string()
}
