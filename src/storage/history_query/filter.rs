//! 历史筛选条件与查询结果行/排序类型。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::model::Protocol;

/// 远端网段筛选:v4 为主机序 u32 闭区间;v6 为 u128 闭区间(经 16 字节
/// 大端 BLOB 绑定,等长 BLOB 的 memcmp 字典序即数值序,BETWEEN 语义一致)
#[derive(Clone, Copy)]
pub enum RemoteFilter {
    V4(u32, u32),
    V6(u128, u128),
}

/// 远端筛选输入解析:v4 点分前缀("142.250.")/CIDR/单 IP 与 v6
/// CIDR/单 IP(含 ':' 判族)
pub fn parse_remote_filter(input: &str) -> Option<RemoteFilter> {
    let s = input.trim();
    if let Some((addr, mask)) = s.split_once('/') {
        let prefix: u32 = mask.trim().parse().ok()?;
        if let Ok(ip) = addr.trim().parse::<Ipv6Addr>() {
            if prefix > 128 {
                return None;
            }
            let v = u128::from(ip);
            let m = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            return Some(RemoteFilter::V6(v & m, v | !m));
        }
        let ip: Ipv4Addr = addr.trim().parse().ok()?;
        if prefix > 32 {
            return None;
        }
        let v = u32::from(ip);
        let lo = if prefix == 0 {
            0
        } else {
            v & (u32::MAX << (32 - prefix))
        };
        let hi = if prefix == 0 {
            u32::MAX
        } else {
            v | u32::MAX.checked_shr(prefix).unwrap_or(0)
        };
        return Some(RemoteFilter::V4(lo, hi));
    }
    if let Ok(ip) = s.parse::<Ipv6Addr>() {
        let v = u128::from(ip);
        return Some(RemoteFilter::V6(v, v));
    }
    if let Ok(ip) = s.parse::<Ipv4Addr>() {
        return Some(RemoteFilter::V4(u32::from(ip), u32::from(ip)));
    }
    super::fmt::parse_ip_prefix(s).map(|(lo, hi)| RemoteFilter::V4(lo, hi))
}

/// 历史筛选条件;时间窗为 [start, +inf):连接存活期与窗口相交即命中
pub struct Filter {
    pub start: u64,
    /// 进程名模糊匹配(空 = 不过滤)
    pub process: String,
    /// 远端 IP 网段范围(None = 不过滤)
    pub remote: Option<RemoteFilter>,
    pub proto: Option<Protocol>,
    /// 隐藏回环远端(127.0.0.0/8、::1)
    pub hide_local: bool,
    /// 隐藏私网远端(RFC1918 + IPv6 ULA fc00::/7 与链路本地 fe80::/10)
    pub hide_lan: bool,
}

impl Filter {
    /// WHERE 条件公共段(时间窗 + 筛选项,参数绑定)。
    /// v4 私网判定用位运算:10/8 = 前 8 位 10;172.16/12 = 前 12 位 0xAC1;
    /// 192.168/16 = 前 16 位 0xC0A8。v6 为等长 BLOB 的 memcmp 比较:
    /// ULA 首 2 字节在 [fc00, fdff],链路本地首 2 字节在 [fe80, febf]
    pub(super) fn where_clause() -> &'static str {
        "WHERE last_seen >= ?1
             AND (?2 = '' OR process LIKE '%' || ?2 || '%' ESCAPE '\\')
             AND (?3 IS NULL OR (remote_ip6 IS NULL AND remote_ip >= ?3 AND remote_ip <= ?4))
             AND (?5 = '' OR proto = ?5)
             AND (?6 = 0 OR NOT ((remote_ip >> 24) = 127
                      OR remote_ip6 = X'00000000000000000000000000000001'))
             AND (?7 = 0 OR NOT ((remote_ip >> 24) = 10
                      OR (remote_ip >> 20) = 2753
                      OR (remote_ip >> 16) = 49320
                      OR (remote_ip6 IS NOT NULL
                          AND ((substr(remote_ip6, 1, 2) >= X'fc00'
                                AND substr(remote_ip6, 1, 2) <= X'fdff')
                           OR (substr(remote_ip6, 1, 2) >= X'fe80'
                                AND substr(remote_ip6, 1, 2) <= X'febf')))))
             AND (?8 IS NULL OR (remote_ip6 IS NOT NULL
                      AND remote_ip6 >= ?8 AND remote_ip6 <= ?9))"
    }

    /// 筛选条件段(?2..?9,不含时间窗):统计表快路径的明细补段查询
    /// 复用,与 where_clause 的对应行**必须同步维护**
    pub(super) fn cond_clause() -> &'static str {
        "AND (?2 = '' OR process LIKE '%' || ?2 || '%' ESCAPE '\\')
             AND (?3 IS NULL OR (remote_ip6 IS NULL AND remote_ip >= ?3 AND remote_ip <= ?4))
             AND (?5 = '' OR proto = ?5)
             AND (?6 = 0 OR NOT ((remote_ip >> 24) = 127
                      OR remote_ip6 = X'00000000000000000000000000000001'))
             AND (?7 = 0 OR NOT ((remote_ip >> 24) = 10
                      OR (remote_ip >> 20) = 2753
                      OR (remote_ip >> 16) = 49320
                      OR (remote_ip6 IS NOT NULL
                          AND ((substr(remote_ip6, 1, 2) >= X'fc00'
                                AND substr(remote_ip6, 1, 2) <= X'fdff')
                           OR (substr(remote_ip6, 1, 2) >= X'fe80'
                                AND substr(remote_ip6, 1, 2) <= X'febf')))))
             AND (?8 IS NULL OR (remote_ip6 IS NOT NULL
                      AND remote_ip6 >= ?8 AND remote_ip6 <= ?9))"
    }

    /// 与 where_clause 参数位一一对应(全部 owned);
    /// proto 为 None 绑空串(SQL 侧以 ?5 = '' 判定不过滤,NULL 比较恒假);
    /// 进程名经 escape_like 转义,用户输入的 %/_ 按字面匹配
    pub(super) fn bind(&self) -> [Box<dyn rusqlite::ToSql>; 9] {
        let (r4lo, r4hi, r6lo, r6hi) = match self.remote {
            Some(RemoteFilter::V4(lo, hi)) => (Some(lo as i64), Some(hi as i64), None, None),
            Some(RemoteFilter::V6(lo, hi)) => (
                None,
                None,
                Some(lo.to_be_bytes().to_vec()),
                Some(hi.to_be_bytes().to_vec()),
            ),
            None => (None, None, None, None),
        };
        [
            Box::new(self.start as i64),
            Box::new(escape_like(&self.process)),
            Box::new(r4lo),
            Box::new(r4hi),
            Box::new(
                self.proto
                    .map(|p| p.as_str().to_owned())
                    .unwrap_or_default(),
            ),
            Box::new(self.hide_local as i64),
            Box::new(self.hide_lan as i64),
            Box::new(r6lo),
            Box::new(r6hi),
        ]
    }

    /// ?2..?9 的筛选项绑定(不含时间窗起点):统计表快路径的补段
    /// 查询拼接自有窗口时复用,与 bind 的后 8 项一致
    pub(super) fn bind_tail(&self) -> Vec<Box<dyn rusqlite::ToSql>> {
        let mut tail: Vec<Box<dyn rusqlite::ToSql>> = self.bind().into_iter().skip(1).collect();
        tail.truncate(8);
        tail
    }
}

/// LIKE 通配符转义:进程名筛选是子串语义,输入里的 %/_/\ 按字面
/// 匹配(SQL 侧配 ESCAPE '\',见 where_clause),否则输 a_b 会多匹配
/// axb、输 % 等于不过滤。统计表快路径的 proc_stats 查询同款复用
pub(super) fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// 明细行(一条已完结连接)
pub struct DetailRow {
    pub event_id: u64,
    pub first_seen: u64,
    pub last_seen: u64,
    pub pid: u32,
    pub process: String,
    pub proc_path: Option<String>,
    pub proto: Protocol,
    /// IPv4 远端地址(v6 行为零值占位,族判定见 remote_ip6)
    pub remote_ip: Ipv4Addr,
    /// IPv6 远端地址(v4 行为 None)
    pub remote_ip6: Option<Ipv6Addr>,
    pub remote_port: u16,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

impl DetailRow {
    /// 远端地址(按族取有效列)
    pub fn remote_addr(&self) -> IpAddr {
        self.remote_ip6
            .map(IpAddr::V6)
            .unwrap_or(IpAddr::V4(self.remote_ip))
    }
}

/// 聚合行(进程 x 协议 x 远端)
pub struct AggregateRow {
    pub process: String,
    pub proto: Protocol,
    /// IPv4 远端地址(v6 行为零值占位,族判定见 remote_ip6)
    pub remote_ip: Ipv4Addr,
    /// IPv6 远端地址(v4 行为 None)
    pub remote_ip6: Option<Ipv6Addr>,
    pub count: u64,
    pub total_secs: u64,
    pub last_active: u64,
    pub bytes_out: u64,
    pub bytes_in: u64,
}

impl AggregateRow {
    /// 远端地址(按族取有效列)
    pub fn remote_addr(&self) -> IpAddr {
        self.remote_ip6
            .map(IpAddr::V6)
            .unwrap_or(IpAddr::V4(self.remote_ip))
    }
}

/// 聚合视图排序键
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AggregateSort {
    LastActive,
    BytesOut,
    BytesIn,
    Count,
    TotalSecs,
}

impl AggregateSort {
    /// SQL ORDER BY 列名映射(白名单常量,非外部输入)
    pub(super) fn order_col(self) -> &'static str {
        match self {
            AggregateSort::LastActive => "last_active",
            AggregateSort::BytesOut => "out_total",
            AggregateSort::BytesIn => "in_total",
            AggregateSort::Count => "n",
            AggregateSort::TotalSecs => "total",
        }
    }

    /// 日志用中文名
    pub fn label(self) -> &'static str {
        match self {
            AggregateSort::LastActive => "最近活动",
            AggregateSort::BytesOut => "上传",
            AggregateSort::BytesIn => "下载",
            AggregateSort::Count => "连接数",
            AggregateSort::TotalSecs => "时长",
        }
    }
}

/// 汇总行(按进程聚合;同一进程名的多路径/PID 合并,路径取代表用于图标)。
/// Clone 供汇总视图克隆 SQL 结果后叠加活跃连接实时字节(不写回查询缓存)
#[derive(Clone)]
pub struct SummaryRow {
    pub process: String,
    pub proc_path: Option<String>,
    pub bytes_out: u64,
    pub bytes_in: u64,
    pub count: u64,
    pub total_secs: u64,
}

/// 汇总视图排序键
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SummarySort {
    BytesOut,
    BytesIn,
    Count,
    TotalSecs,
}

impl SummarySort {
    /// SQL ORDER BY 列名映射(白名单常量,非外部输入)
    pub(super) fn order_col(self) -> &'static str {
        match self {
            SummarySort::BytesOut => "bytes_out",
            SummarySort::BytesIn => "bytes_in",
            SummarySort::Count => "n",
            SummarySort::TotalSecs => "total",
        }
    }

    /// 日志用中文名
    pub fn label(self) -> &'static str {
        match self {
            SummarySort::BytesOut => "上传",
            SummarySort::BytesIn => "下载",
            SummarySort::Count => "连接数",
            SummarySort::TotalSecs => "时长",
        }
    }
}

/// 用量行(时间桶聚合)
pub struct UsageRow {
    /// 桶起点 unix 秒(已还原回真实时间轴:桶号 * 桶宽 - 时区偏移)
    pub bucket_start: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
}
