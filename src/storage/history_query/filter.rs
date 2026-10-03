//! 历史筛选条件与查询结果行/排序类型。

use std::net::Ipv4Addr;

use crate::model::Protocol;

/// 历史筛选条件;时间窗为 [start, +inf):连接存活期与窗口相交即命中
pub struct Filter {
    pub start: u64,
    /// 进程名模糊匹配(空 = 不过滤)
    pub process: String,
    /// 远端 IP 网段范围 [min, max](None = 不过滤)
    pub remote: Option<(u32, u32)>,
    pub proto: Option<Protocol>,
    /// 隐藏回环远端(127.0.0.0/8)
    pub hide_local: bool,
    /// 隐藏私网远端(RFC1918:10/8、172.16/12、192.168/16)
    pub hide_lan: bool,
}

impl Filter {
    /// WHERE 条件公共段(时间窗 + 筛选项,参数绑定)。
    /// 私网判定用位运算:10/8 = 前 8 位 10;172.16/12 = 前 12 位 0xAC1;
    /// 192.168/16 = 前 16 位 0xC0A8
    pub(super) fn where_clause() -> &'static str {
        "WHERE last_seen >= ?1
             AND (?2 = '' OR process LIKE '%' || ?2 || '%')
             AND (?3 IS NULL OR (remote_ip >= ?3 AND remote_ip <= ?4))
             AND (?5 = '' OR proto = ?5)
             AND (?6 = 0 OR NOT ((remote_ip >> 24) = 127))
             AND (?7 = 0 OR NOT ((remote_ip >> 24) = 10
                      OR (remote_ip >> 20) = 2753
                      OR (remote_ip >> 16) = 49320))"
    }

    /// 与 where_clause 参数位一一对应(全部 owned);
    /// proto 为 None 时绑空串(SQL 侧以 ?5 = '' 判定不过滤,NULL 比较恒假)
    pub(super) fn bind(&self) -> [Box<dyn rusqlite::ToSql>; 7] {
        [
            Box::new(self.start as i64),
            Box::new(self.process.clone()),
            Box::new(self.remote.map(|(lo, _)| lo as i64)),
            Box::new(self.remote.map(|(_, hi)| hi as i64)),
            Box::new(
                self.proto
                    .map(|p| p.as_str().to_owned())
                    .unwrap_or_default(),
            ),
            Box::new(self.hide_local as i64),
            Box::new(self.hide_lan as i64),
        ]
    }
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
    pub remote_ip: Ipv4Addr,
    pub remote_port: u16,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

/// 聚合行(进程 x 协议 x 远端)
pub struct AggregateRow {
    pub process: String,
    pub proto: Protocol,
    pub remote_ip: Ipv4Addr,
    pub count: u64,
    pub total_secs: u64,
    pub last_active: u64,
    pub bytes_out: u64,
    pub bytes_in: u64,
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
}

/// 用量行(时间桶聚合)
pub struct UsageRow {
    /// 桶起点 unix 秒(已还原回真实时间轴:桶号 * 桶宽 - 时区偏移)
    pub bucket_start: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
}
