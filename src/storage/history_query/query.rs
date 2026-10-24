//! 历史只读查询(明细/聚合/汇总/用量)与批量删除,全部参数绑定。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use rusqlite::params;

use super::QUERY_LIMIT;
use super::filter::{
    AggregateRow, AggregateSort, DetailRow, Filter, SummaryRow, SummarySort, UsageRow,
};
use crate::model::Protocol;
use crate::storage::history::Db;

/// 用量查询:按 (last_seen + 时区偏移) 整除桶宽分桶,桶内收发字节求和;
/// 一条跨桶连接计入其最后活跃桶(与聚合视图按行口径一致)。
/// bucket_secs = 3600(小时)或 86400(天)。
/// 参数编号约定:?1..?9 归 Filter::where_clause/bind(其中 ?8/?9 是
/// v6 远端范围),时区偏移必须另起编号 ?10——SQLite 的 ?N 按编号寻址,
/// 复用 ?8 会让追加的时区参数成为第 10 个实参,rusqlite 校验数量即
/// InvalidParameterCount 失败,用量视图整表空白
pub(super) fn query_usage(
    db: &Db,
    f: &Filter,
    bucket_secs: u64,
    tz_off: i64,
) -> rusqlite::Result<Vec<UsageRow>> {
    let sql = format!(
        "SELECT (last_seen + ?10) / {bucket_secs} AS bucket,
                SUM(bytes_in) AS in_total, SUM(bytes_out) AS out_total
         FROM conn_events {} GROUP BY bucket ORDER BY bucket DESC LIMIT {QUERY_LIMIT}",
        Filter::where_clause()
    );
    let mut stmt = db.prepare_cached(&sql)?;
    let tz = Box::new(tz_off) as Box<dyn rusqlite::ToSql>;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(f.bind().into_iter().chain(std::iter::once(tz))),
        |row| {
            let bucket: i64 = row.get(0)?;
            Ok(UsageRow {
                bucket_start: (bucket * bucket_secs as i64 - tz_off).max(0) as u64,
                bytes_in: row.get::<_, i64>(1)?.max(0) as u64,
                bytes_out: row.get::<_, i64>(2)?.max(0) as u64,
            })
        },
    )?;
    // 倒序取 LIMIT(超上限时保留最近的桶),还原为时间正序供绘制
    let mut out: Vec<UsageRow> = rows.collect::<Result<_, _>>()?;
    out.reverse();
    Ok(out)
}

/// 明细查询:按最后活动倒序,最多 QUERY_LIMIT 行
pub(super) fn query_detail(db: &Db, f: &Filter) -> rusqlite::Result<Vec<DetailRow>> {
    let sql = format!(
        "SELECT event_id, first_seen, last_seen, pid, process, proc_path, proto, remote_ip, remote_port,
                bytes_in, bytes_out, remote_ip6
         FROM conn_events {} ORDER BY last_seen DESC LIMIT {QUERY_LIMIT}",
        Filter::where_clause()
    );
    let mut stmt = db.prepare_cached(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(f.bind()), |row| {
        Ok(DetailRow {
            event_id: row.get::<_, i64>(0)? as u64,
            first_seen: row.get::<_, i64>(1)? as u64,
            last_seen: row.get::<_, i64>(2)? as u64,
            pid: row.get::<_, i64>(3)? as u32,
            process: row.get(4)?,
            proc_path: row.get(5)?,
            proto: parse_proto(&row.get::<_, String>(6)?),
            remote_ip: Ipv4Addr::from(row.get::<_, i64>(7)? as u32),
            remote_ip6: parse_ip6(row.get::<_, Option<Vec<u8>>>(11)?),
            remote_port: row.get::<_, i64>(8)? as u16,
            bytes_in: row.get::<_, i64>(9)?.max(0) as u64,
            bytes_out: row.get::<_, i64>(10)?.max(0) as u64,
        })
    })?;
    rows.collect()
}

/// 聚合查询:按进程 x 协议 x 远端汇总(次数/累计时长/最近活动/字节总量)。
/// ORDER BY 列与方向来自排序枚举的白名单映射,不含外部输入
pub(super) fn query_aggregate(
    db: &Db,
    f: &Filter,
    sort: AggregateSort,
    ascending: bool,
) -> rusqlite::Result<Vec<AggregateRow>> {
    let dir = if ascending { "ASC" } else { "DESC" };
    let sql = format!(
        "SELECT process, proto, remote_ip, remote_ip6, COUNT(*) AS n,
                SUM(last_seen - first_seen) AS total, MAX(last_seen) AS last_active,
                SUM(bytes_out) AS out_total, SUM(bytes_in) AS in_total
         FROM conn_events {} GROUP BY process, proto, remote_ip, remote_ip6
         ORDER BY {} {dir} LIMIT {QUERY_LIMIT}",
        Filter::where_clause(),
        sort.order_col()
    );
    let mut stmt = db.prepare_cached(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(f.bind()), |row| {
        Ok(AggregateRow {
            process: row.get(0)?,
            proto: parse_proto(&row.get::<_, String>(1)?),
            remote_ip: Ipv4Addr::from(row.get::<_, i64>(2)? as u32),
            remote_ip6: parse_ip6(row.get::<_, Option<Vec<u8>>>(3)?),
            count: row.get::<_, i64>(4)? as u64,
            total_secs: row.get::<_, i64>(5)?.max(0) as u64,
            last_active: row.get::<_, i64>(6)? as u64,
            bytes_out: row.get::<_, i64>(7)?.max(0) as u64,
            bytes_in: row.get::<_, i64>(8)?.max(0) as u64,
        })
    })?;
    rows.collect()
}

/// 汇总查询:按进程聚合(上/下行总量、次数、累计时长)。
/// ORDER BY 列与方向来自排序枚举的白名单映射,不含外部输入
pub(super) fn query_summary(
    db: &Db,
    f: &Filter,
    sort: SummarySort,
    ascending: bool,
) -> rusqlite::Result<Vec<SummaryRow>> {
    let dir = if ascending { "ASC" } else { "DESC" };
    let sql = format!(
        "SELECT process, MAX(proc_path) AS proc_path,
                SUM(bytes_out) AS out_total, SUM(bytes_in) AS in_total,
                COUNT(*) AS n, SUM(last_seen - first_seen) AS total
         FROM conn_events {} GROUP BY process
         ORDER BY {} {dir} LIMIT {QUERY_LIMIT}",
        Filter::where_clause(),
        sort.order_col()
    );
    let mut stmt = db.prepare_cached(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(f.bind()), |row| {
        Ok(SummaryRow {
            process: row.get(0)?,
            proc_path: row.get(1)?,
            bytes_out: row.get::<_, i64>(2)?.max(0) as u64,
            bytes_in: row.get::<_, i64>(3)?.max(0) as u64,
            count: row.get::<_, i64>(4)? as u64,
            total_secs: row.get::<_, i64>(5)?.max(0) as u64,
        })
    })?;
    rows.collect()
}

/// 待确认的批量删除(右键菜单发起,确认弹窗执行)。
/// proto/remote_ip 均为 Some 时删除聚合组,否则删除整个进程
#[derive(Clone)]
pub struct PendingDelete {
    pub process: String,
    pub proto: Option<Protocol>,
    pub remote_ip: Option<IpAddr>,
}

/// 明细行删除:事件身份 + 首末时刻定位唯一行(端口复用产生同 event_id
/// 的多次连接,由 first_seen 区分)。统计表联动:该行所在日桶失效待重算
pub fn delete_detail(
    db: &Db,
    event_id: u64,
    first_seen: u64,
    last_seen: u64,
) -> rusqlite::Result<usize> {
    let n = db.execute(
        "DELETE FROM conn_events WHERE event_id = ?1 AND first_seen = ?2 AND last_seen = ?3",
        params![event_id as i64, first_seen as i64, last_seen as i64],
    )?;
    if n > 0 {
        invalidate_stats(db, [last_seen as i64]);
    }
    Ok(n)
}

/// 聚合组删除:同进程 + 协议 + 远端(按族落到对应地址列)的全部记录。
/// RETURNING 收集受影响日桶做统计表失效
pub fn delete_aggregate_group(
    db: &Db,
    process: &str,
    proto: Protocol,
    remote: IpAddr,
) -> rusqlite::Result<usize> {
    let sql = match remote {
        IpAddr::V4(_) => {
            "DELETE FROM conn_events WHERE process = ?1 AND proto = ?2
             AND remote_ip6 IS NULL AND remote_ip = ?3 RETURNING last_seen"
        }
        IpAddr::V6(_) => {
            "DELETE FROM conn_events WHERE process = ?1 AND proto = ?2
             AND remote_ip6 = ?3 RETURNING last_seen"
        }
    };
    let mut stmt = db.prepare(sql)?;
    let v4 = match remote {
        IpAddr::V4(v4) => u32::from(v4) as i64,
        IpAddr::V6(_) => 0,
    };
    let v6 = match remote {
        IpAddr::V4(_) => None,
        IpAddr::V6(v6) => Some(u128::from(v6).to_be_bytes().to_vec()),
    };
    let mut n = 0usize;
    let mut days: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let rows = stmt.query_map(params![process, proto.as_str(), v4or6_value(v4, v6)], |r| {
        r.get::<_, i64>(0)
    })?;
    for ls in rows.flatten() {
        n += 1;
        days.insert(day_bucket(ls));
    }
    if !days.is_empty() {
        crate::storage::history::invalidate_days(db, &days);
    }
    Ok(n)
}

/// 进程全部记录删除(RETURNING 收集受影响日桶做统计表失效)
pub fn delete_process(db: &Db, process: &str) -> rusqlite::Result<usize> {
    let mut stmt = db.prepare("DELETE FROM conn_events WHERE process = ?1 RETURNING last_seen")?;
    let mut n = 0usize;
    let mut days: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let rows = stmt.query_map(params![process], |r| r.get::<_, i64>(0))?;
    for ls in rows.flatten() {
        n += 1;
        days.insert(day_bucket(ls));
    }
    if !days.is_empty() {
        crate::storage::history::invalidate_days(db, &days);
    }
    Ok(n)
}

/// v4/v6 远端参数:族对应的列绑值,另一列绑占位(同一 ?3 位两种族)
fn v4or6_value(v4: i64, v6: Option<Vec<u8>>) -> Box<dyn rusqlite::ToSql> {
    match v6 {
        Some(b) => Box::new(b),
        None => Box::new(v4),
    }
}

/// 本地日桶号(与统计表口径一致)
fn day_bucket(last_seen: i64) -> i64 {
    (last_seen + super::fmt::local_tz_offset_secs()).div_euclid(86400)
}

/// 单行删除的统计表失效
fn invalidate_stats(db: &Db, times: impl IntoIterator<Item = i64>) {
    let days: std::collections::HashSet<i64> = times.into_iter().map(day_bucket).collect();
    crate::storage::history::invalidate_days(db, &days);
}

/// remote_ip6 列读回:16 字节大端 BLOB,长度不符视为损坏行(NULL 语义)
fn parse_ip6(b: Option<Vec<u8>>) -> Option<Ipv6Addr> {
    let bytes: [u8; 16] = b?.try_into().ok()?;
    Some(Ipv6Addr::from(bytes))
}

fn parse_proto(s: &str) -> Protocol {
    if s == Protocol::Udp.as_str() {
        Protocol::Udp
    } else {
        Protocol::Tcp
    }
}

/// 落库用量字节快照:今日(入站, 出站)与当月总量
pub struct DbBytes {
    pub today: (u64, u64),
    pub month: u64,
}

/// 今日与当月(本地时区)累计收发字节,悬浮球"今日总量"与用量配额
/// 告警的数据源(口径 = 全部落库流量,不套历史页筛选;活跃连接的实时
/// 字节由调用方叠加)。大库上月初窗口聚合秒级,须在工作线程执行。
/// 月初窗口的已完结天全部物化时走统计表快路径:[月初, 今天零点) 段
/// usage_stats 单次 SUM(毫秒级),今天段永不物化保持明细实时算(单天
/// 走 last_seen 索引);缺块(写线程未回填)回退明细单 SQL 全扫
pub fn query_db_bytes(db: &Db) -> DbBytes {
    let tz = super::fmt::local_tz_offset_secs();
    let today_start = super::fmt::day_start() as i64;
    let month_start = super::fmt::month_start() as i64;
    let today = (today_start + tz).div_euclid(86400);
    let seg_day = (month_start + tz).div_euclid(86400);
    if super::stats_query::days_covered(db, seg_day, today, tz) {
        if let Some(b) = query_db_bytes_stats(db, seg_day, today, today_start) {
            return b;
        }
        return DbBytes {
            today: (0, 0),
            month: 0,
        };
    }
    query_db_bytes_detail(db, today_start, month_start)
}

/// 统计表快路径:None = 任一段查询失败(调用方按全零处理,与明细
/// 路径失败语义一致,不返回部分和)
fn query_db_bytes_stats(db: &Db, seg_day: i64, today: i64, today_start: i64) -> Option<DbBytes> {
    let (hist_in, hist_out) = sum_bytes(
        db,
        "SELECT COALESCE(SUM(bytes_in), 0), COALESCE(SUM(bytes_out), 0)
         FROM usage_stats WHERE hour >= ?1 * 24 AND hour < ?2 * 24",
        params![seg_day, today],
    )?;
    let (day_in, day_out) = sum_bytes(
        db,
        "SELECT COALESCE(SUM(bytes_in), 0), COALESCE(SUM(bytes_out), 0)
         FROM conn_events WHERE last_seen >= ?1",
        params![today_start],
    )?;
    Some(DbBytes {
        today: (day_in, day_out),
        month: hist_in + hist_out + day_in + day_out,
    })
}

/// 明细回退:月初窗口单次扫描,今日是当月子集,CASE WHEN 同行取值,
/// 免两次全窗扫描
fn query_db_bytes_detail(db: &Db, today_start: i64, month_start: i64) -> DbBytes {
    match db.query_row(
        "SELECT COALESCE(SUM(CASE WHEN last_seen >= ?1 THEN bytes_in ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN last_seen >= ?1 THEN bytes_out ELSE 0 END), 0),
                COALESCE(SUM(bytes_in), 0),
                COALESCE(SUM(bytes_out), 0)
         FROM conn_events WHERE last_seen >= ?2",
        params![today_start, month_start],
        |r| {
            Ok(DbBytes {
                today: (
                    r.get::<_, i64>(0)?.max(0) as u64,
                    r.get::<_, i64>(1)?.max(0) as u64,
                ),
                month: (r.get::<_, i64>(2)?.max(0) as u64 + r.get::<_, i64>(3)?.max(0) as u64),
            })
        },
    ) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("[History] 用量字节查询失败: {e}");
            DbBytes {
                today: (0, 0),
                month: 0,
            }
        }
    }
}

/// 双列 SUM 取回(失败带日志返回 None)
fn sum_bytes(db: &Db, sql: &str, args: impl rusqlite::Params) -> Option<(u64, u64)> {
    match db.query_row(sql, args, |r| {
        Ok((
            r.get::<_, i64>(0)?.max(0) as u64,
            r.get::<_, i64>(1)?.max(0) as u64,
        ))
    }) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!("[History] 用量字节统计表快路径查询失败: {e}");
            None
        }
    }
}
