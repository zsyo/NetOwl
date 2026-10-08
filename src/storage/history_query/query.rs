//! 历史只读查询(明细/聚合/汇总/用量)与批量删除,全部参数绑定。

use std::net::Ipv4Addr;

use rusqlite::params;

use super::QUERY_LIMIT;
use super::filter::{
    AggregateRow, AggregateSort, DetailRow, Filter, SummaryRow, SummarySort, UsageRow,
};
use crate::model::Protocol;
use crate::storage::history::Db;

/// 用量查询:按 (last_seen + 时区偏移) 整除桶宽分桶,桶内收发字节求和;
/// 一条跨桶连接计入其最后活跃桶(与聚合视图按行口径一致)。
/// bucket_secs = 3600(小时)或 86400(天)
pub(super) fn query_usage(
    db: &Db,
    f: &Filter,
    bucket_secs: u64,
    tz_off: i64,
) -> rusqlite::Result<Vec<UsageRow>> {
    let sql = format!(
        "SELECT (last_seen + ?8) / {bucket_secs} AS bucket,
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
                bytes_in, bytes_out
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
        "SELECT process, proto, remote_ip, COUNT(*) AS n,
                SUM(last_seen - first_seen) AS total, MAX(last_seen) AS last_active,
                SUM(bytes_out) AS out_total, SUM(bytes_in) AS in_total
         FROM conn_events {} GROUP BY process, proto, remote_ip
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
            count: row.get::<_, i64>(3)? as u64,
            total_secs: row.get::<_, i64>(4)?.max(0) as u64,
            last_active: row.get::<_, i64>(5)? as u64,
            bytes_out: row.get::<_, i64>(6)?.max(0) as u64,
            bytes_in: row.get::<_, i64>(7)?.max(0) as u64,
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
    pub remote_ip: Option<Ipv4Addr>,
}

/// 明细行删除:事件身份 + 首末时刻定位唯一行(端口复用产生同 event_id
/// 的多次连接,由 first_seen 区分)
pub fn delete_detail(db: &Db, r: &DetailRow) -> rusqlite::Result<usize> {
    db.execute(
        "DELETE FROM conn_events WHERE event_id = ?1 AND first_seen = ?2 AND last_seen = ?3",
        params![r.event_id as i64, r.first_seen as i64, r.last_seen as i64],
    )
}

/// 聚合组删除:同进程 + 协议 + 远端的全部记录
pub fn delete_aggregate_group(
    db: &Db,
    process: &str,
    proto: Protocol,
    remote_ip: Ipv4Addr,
) -> rusqlite::Result<usize> {
    db.execute(
        "DELETE FROM conn_events WHERE process = ?1 AND proto = ?2 AND remote_ip = ?3",
        params![process, proto.as_str(), u32::from(remote_ip) as i64],
    )
}

/// 进程全部记录删除
pub fn delete_process(db: &Db, process: &str) -> rusqlite::Result<usize> {
    db.execute(
        "DELETE FROM conn_events WHERE process = ?1",
        params![process],
    )
}

fn parse_proto(s: &str) -> Protocol {
    if s == Protocol::Udp.as_str() {
        Protocol::Udp
    } else {
        Protocol::Tcp
    }
}

/// 当月(本地时区)累计收发字节总和:用量配额告警的数据源
/// (配额口径 = 全部落库流量,不套用历史页筛选)
pub fn query_month_bytes(db: &Db) -> u64 {
    match db.query_row(
        "SELECT COALESCE(SUM(bytes_in + bytes_out), 0) FROM conn_events WHERE last_seen >= ?1",
        [super::fmt::month_start() as i64],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(n) => n.max(0) as u64,
        Err(e) => {
            tracing::warn!("[History] 月度用量查询失败: {e}");
            0
        }
    }
}

/// 当日(本地时区)累计收发字节,拆 (入站, 出站) 两列:悬浮窗浮窗
/// "今日总量"的数据源(口径同上,不套历史页筛选;活跃连接的实时
/// 字节由调用方叠加)
pub fn query_today_bytes(db: &Db) -> (u64, u64) {
    match db.query_row(
        "SELECT COALESCE(SUM(bytes_in), 0), COALESCE(SUM(bytes_out), 0)
         FROM conn_events WHERE last_seen >= ?1",
        [super::fmt::day_start() as i64],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
    ) {
        Ok((inn, out)) => (inn.max(0) as u64, out.max(0) as u64),
        Err(e) => {
            tracing::warn!("[History] 当日用量查询失败: {e}");
            (0, 0)
        }
    }
}
