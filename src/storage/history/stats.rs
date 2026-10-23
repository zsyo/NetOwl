//! 历史统计预聚合(proc_stats/usage_stats,db v10):汇总与用量视图的
//! 加速缓存,非权威数据——任何缺块查询侧都回退明细全扫,这里负责在
//! 写线程空闲时逐天物化,删除/清理后失效重算。
//!
//! 口径:day/hour 为本地时区桶号 (last_seen + tz) / 86400|3600,与
//! history_query 分桶一致;net_class 三分(0=回环 1=私网 2=公网含保留
//! 段),与筛选 hide_local/hide_lan 的 SQL 位运算判定等价;聚合值
//! (COUNT/SUM 时长/SUM 字节)与汇总、用量视图的明细 SQL 同构,查询侧
//! 两段结果可直接相加。
//!
//! 物化时机 = 写线程每轮循环末尾(消息间隙,每轮至多 MATERIALIZE_PER_LOOP
//! 天):覆盖启动补齐缺失天、跨天后物化昨天、失效重算三个触发点;"今天"
//! 永不物化(行还在持续写入),查询侧从明细实时算。

use std::collections::HashSet;

use rusqlite::params;

use super::{ClosedConn, Db, unix_now};
use crate::storage::history_query::local_tz_offset_secs;

/// 每轮循环物化的天数上限:单天约 0.2s,4 天 < 1s 不至于阻塞批量落库
const MATERIALIZE_PER_LOOP: usize = 4;

/// 本地日桶号(unix 秒 -> (t + 时区偏移) / 86400)
fn day_bucket(t: i64, tz: i64) -> i64 {
    (t + tz).div_euclid(86400)
}

/// net_class 判定 SQL(与 filter.rs 的 hide_local/hide_lan 位运算同口径):
/// 0=回环(127/8、::1) 1=私网(RFC1918 + v6 ULA/链路本地) 2=公网含保留段。
/// SELECT 与 GROUP BY 各用一次,定义为常量
const NET_CLASS_SQL: &str = "CASE
    WHEN (remote_ip >> 24) = 127
      OR remote_ip6 = X'00000000000000000000000000000001' THEN 0
    WHEN (remote_ip >> 24) = 10 OR (remote_ip >> 20) = 2753
      OR (remote_ip >> 16) = 49320
      OR (remote_ip6 IS NOT NULL AND (
            (substr(remote_ip6, 1, 2) >= X'fc00' AND substr(remote_ip6, 1, 2) <= X'fdff')
         OR (substr(remote_ip6, 1, 2) >= X'fe80' AND substr(remote_ip6, 1, 2) <= X'febf'))) THEN 1
    ELSE 2 END";

/// 物化缺失的已完结天(昨天及以前;无数据的天也标记,查询侧依赖
/// stats_done 连续覆盖判定)。每天单事务:清旧统计行 -> 聚合写入 ->
/// 标记完成,失败告警后本轮放弃(下轮重试)
pub(super) fn materialize_pending(conn: &mut Db) {
    let tz = local_tz_offset_secs();
    let today = day_bucket(unix_now() as i64, tz);
    // 覆盖范围从库中最早数据起(走 last_seen 索引,O(1))
    let first_seen: Option<i64> =
        match conn.query_row("SELECT MIN(last_seen) FROM conn_events", [], |r| r.get(0)) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("[Stats] 读取最早历史时刻失败: {e}");
                return;
            }
        };
    let Some(first) = first_seen else { return };
    let done: HashSet<i64> = match conn.prepare("SELECT day FROM stats_done") {
        Ok(mut stmt) => match stmt.query_map([], |r| r.get::<_, i64>(0)) {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                tracing::warn!("[Stats] 读取已物化天失败: {e}");
                return;
            }
        },
        Err(e) => {
            tracing::warn!("[Stats] 读取已物化天失败: {e}");
            return;
        }
    };
    let first_day = day_bucket(first, tz);
    let pending: Vec<i64> = (first_day..today)
        .filter(|d| !done.contains(d))
        .take(MATERIALIZE_PER_LOOP)
        .collect();
    for day in pending {
        match materialize_day(conn, day, tz) {
            Ok(n) => tracing::debug!("[Stats] 物化日桶 {day} 完成: {n} 组"),
            Err(e) => {
                tracing::warn!("[Stats] 物化日桶 {day} 失败: {e}");
                return;
            }
        }
    }
}

/// 物化单天:proc_stats 一天一组键(process × proto × net_class),
/// usage_stats 按小时桶;重算场景先清旧行再写,最后标记 stats_done
fn materialize_day(conn: &mut Db, day: i64, tz: i64) -> rusqlite::Result<usize> {
    let lo = day * 86400 - tz;
    let hi = lo + 86400;
    let hour_lo = day * 24;
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM proc_stats WHERE day = ?1", [day])?;
    tx.execute(
        "DELETE FROM usage_stats WHERE hour >= ?1 AND hour < ?2",
        params![hour_lo, hour_lo + 24],
    )?;
    let groups = tx.execute(
        &format!(
            "INSERT INTO proc_stats (day, process, proto, net_class, n, total_secs, bytes_in, bytes_out, rep_path)
             SELECT ?1, process, proto, nc, COUNT(*), SUM(last_seen - first_seen),
                    SUM(bytes_in), SUM(bytes_out), COALESCE(MAX(proc_path), '')
             FROM (SELECT process, proto, proc_path, first_seen, last_seen, bytes_in, bytes_out,
                          {NET_CLASS_SQL} AS nc
                   FROM conn_events WHERE last_seen >= ?2 AND last_seen < ?3)
             GROUP BY process, proto, nc"
        ),
        params![day, lo, hi],
    )?;
    tx.execute(
        &format!(
            "INSERT INTO usage_stats (hour, net_class, proto, bytes_in, bytes_out)
             SELECT (last_seen + ?1) / 3600, nc, proto, SUM(bytes_in), SUM(bytes_out)
             FROM (SELECT proto, last_seen, bytes_in, bytes_out, {NET_CLASS_SQL} AS nc
                   FROM conn_events WHERE last_seen >= ?2 AND last_seen < ?3)
             GROUP BY (last_seen + ?1) / 3600, nc, proto"
        ),
        params![tz, lo, hi],
    )?;
    tx.execute("INSERT OR REPLACE INTO stats_done (day) VALUES (?1)", [day])?;
    tx.commit()?;
    Ok(groups)
}

/// 落库后失效检查:完结行的 last_seen 落在已物化天(跨天瞬间的滞后
/// 落库)则该天统计已过期,删标记待重算——保证统计表永远不漏行
pub(super) fn invalidate_written(conn: &Db, events: &[ClosedConn]) {
    let tz = local_tz_offset_secs();
    let today = day_bucket(unix_now() as i64, tz);
    let stale: HashSet<i64> = events
        .iter()
        .map(|e| day_bucket(e.last_seen as i64, tz))
        .filter(|d| *d < today)
        .collect();
    invalidate_days(conn, &stale);
}

/// 失效指定日桶(删除联动/滞后落库共用):只删 stats_done 标记,
/// 查询侧即刻回退明细;旧统计行保留,重算时覆盖
pub fn invalidate_days(conn: &Db, days: &HashSet<i64>) {
    for day in days {
        if let Err(e) = conn.execute("DELETE FROM stats_done WHERE day = ?1", [*day]) {
            tracing::warn!("[Stats] 失效日桶 {day} 失败: {e}");
        }
    }
}

/// 清理联动:按保留截止同步删统计表(purge_before/cleanup 后,写线程
/// 连接执行)。days = 0 清空全部;否则删 cutoff(= now - days*86400)
/// 所在日桶之前的天。rusqlite execute 单语句,三条分开执行
pub(super) fn purge_stats(conn: &Db, days: u32) {
    let res = if days == 0 {
        conn.execute_batch(
            "DELETE FROM proc_stats;
             DELETE FROM usage_stats;
             DELETE FROM stats_done;",
        )
    } else {
        let tz = local_tz_offset_secs();
        let before = unix_now().saturating_sub(days as u64 * 86400) as i64;
        let cutoff = day_bucket(before, tz);
        conn.execute("DELETE FROM proc_stats WHERE day < ?1", [cutoff])
            .and_then(|_| conn.execute("DELETE FROM usage_stats WHERE hour < ?1", [cutoff * 24]))
            .and_then(|_| conn.execute("DELETE FROM stats_done WHERE day < ?1", [cutoff]))
            .map(|_| ())
    };
    if let Err(e) = res {
        tracing::warn!("[Stats] 清理统计表失败: {e}");
    }
}
