//! 汇总/用量视图的统计表快路径(proc_stats/usage_stats,物化见
//! storage/history/stats.rs)。命中条件:筛选可下推(键列覆盖)且查询
//! 覆盖的已完结天全部物化;任一不满足返回 None,调用方回退明细全扫。
//!
//! 结果三段合并:统计表段(完整历史日桶)+ 头部补段(窗口起点未对齐
//! 桶边界时的部分桶,明细实时算)+ 今天段(今天永不物化,明细实时算,
//! 单天走 last_seen 索引)。三段聚合口径同构,数值直接相加。
//! 窗口对齐桶起点(本月档 = 本地月初零点)时头部补段为空。

use std::collections::HashMap;

use super::QUERY_LIMIT;
use super::filter::{Filter, SummaryRow, SummarySort, UsageRow, escape_like};
use super::fmt::local_tz_offset_secs;
use crate::storage::history::{Db, unix_now};

/// 汇总视图快路径:统计表 + 明细补段按进程合并,内存排序取前
/// QUERY_LIMIT。None = 不满足命中条件(remote 筛选无法下推到统计表 /
/// 覆盖天有缺块),调用方回退明细全扫
pub(super) fn try_query_summary_stats(
    db: &Db,
    f: &Filter,
    sort: SummarySort,
    asc: bool,
) -> Option<Vec<SummaryRow>> {
    if f.remote.is_some() {
        return None;
    }
    let tz = local_tz_offset_secs();
    let (seg_day, today) = window_days(db, f, tz)?;
    let seg_start = (seg_day * 86400 - tz).max(0) as u64;
    let today_start = (today * 86400 - tz).max(0) as u64;

    let mut rows = proc_stats_rows(db, f, seg_day, today)?;
    let mut index: HashMap<String, usize> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| (r.process.clone(), i))
        .collect();
    // 头部补段:窗口起点所在部分桶(明细实时算,窗口不足一天)。
    // 上界钳到今天零点:窗口起点落在今天非零点时 seg_day 已越过 today
    // (近 1 小时恒触发),head 与今天段的重叠部分必须让给 tail
    if f.start < seg_start {
        let head = summary_seg(db, f, f.start, Some(seg_start.min(today_start)))?;
        merge_summary(&mut rows, &mut index, head);
    }
    // 今天段:从本地零点起(明细实时算);窗口起点更晚时以窗口起点为下界
    let tail = summary_seg(db, f, today_start.max(f.start), None)?;
    merge_summary(&mut rows, &mut index, tail);

    rows.sort_by(|a, b| {
        let prim = match sort {
            SummarySort::BytesOut => b.bytes_out.cmp(&a.bytes_out),
            SummarySort::BytesIn => b.bytes_in.cmp(&a.bytes_in),
            SummarySort::Count => b.count.cmp(&a.count),
            SummarySort::TotalSecs => b.total_secs.cmp(&a.total_secs),
        };
        (if asc { prim.reverse() } else { prim }).then_with(|| a.process.cmp(&b.process))
    });
    rows.truncate(QUERY_LIMIT);
    tracing::debug!("[History] [query] 汇总统计表快路径命中");
    Some(rows)
}

/// 用量视图快路径:统计表按小时行聚合到查询桶宽(按天 = 24 小时行
/// 合并),与明细补段按桶起点合并。process 筛选无法下推(统计表无
/// 进程维度)时返回 None
pub(super) fn try_query_usage_stats(
    db: &Db,
    f: &Filter,
    bucket_secs: u64,
    tz: i64,
) -> Option<Vec<UsageRow>> {
    if f.remote.is_some() || !f.process.is_empty() {
        return None;
    }
    if bucket_secs != 86400 && bucket_secs != 3600 {
        return None;
    }
    let (seg_day, today) = window_days(db, f, tz)?;
    let seg_start = (seg_day * 86400 - tz).max(0) as u64;
    let today_start = (today * 86400 - tz).max(0) as u64;

    let mut buckets: HashMap<i64, (u64, u64)> = HashMap::new();
    // 统计表段:hour 桶聚合到查询桶宽,桶起点还原与明细段同口径
    let group = if bucket_secs == 86400 {
        "hour / 24"
    } else {
        "hour"
    };
    let sql = format!(
        "SELECT {group}, SUM(bytes_in), SUM(bytes_out)
         FROM usage_stats WHERE hour >= ?1 * 24 AND hour < ?2 * 24 {cond}
         GROUP BY {group}",
        cond = class_cond(f)
    );
    let mut stmt = db.prepare(&sql).ok()?;
    // ?1/?2 绑日桶号,SQL 侧统一 ×24 换算小时桶(勿在两侧各乘一次,
    // 下界会放大 24 倍致统计表段恒空,用量只剩今天段)
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(seg_day), Box::new(today)];
    if let Some(p) = f.proto {
        args.push(Box::new(p.as_str().to_owned()));
    }
    let rows = stmt
        .query_map(rusqlite::params_from_iter(args), |row| {
            let bucket: i64 = row.get(0)?;
            let inn = row.get::<_, i64>(1)?.max(0) as u64;
            let out = row.get::<_, i64>(2)?.max(0) as u64;
            let start = (bucket * bucket_secs as i64 - tz).max(0) as u64;
            Ok((start, inn, out))
        })
        .ok()?;
    for r in rows {
        let (start, inn, out) = r.ok()?;
        let e = buckets.entry(start as i64).or_default();
        e.0 += inn;
        e.1 += out;
    }
    // 头部补段 + 今天段(明细实时算,分桶口径与统计表一致);
    // 段界钳制与 try_query_summary_stats 同理:窗口起点落在今天非零点时
    // 两段不得重叠,今天段以窗口起点为下界
    if f.start < seg_start {
        for r in usage_seg(
            db,
            f,
            f.start,
            Some(seg_start.min(today_start)),
            bucket_secs,
            tz,
        )? {
            let e = buckets.entry(r.bucket_start as i64).or_default();
            e.0 += r.bytes_in;
            e.1 += r.bytes_out;
        }
    }
    for r in usage_seg(db, f, today_start.max(f.start), None, bucket_secs, tz)? {
        let e = buckets.entry(r.bucket_start as i64).or_default();
        e.0 += r.bytes_in;
        e.1 += r.bytes_out;
    }

    let mut out: Vec<UsageRow> = buckets
        .into_iter()
        .map(|(start, (inn, out))| UsageRow {
            bucket_start: start as u64,
            bytes_in: inn,
            bytes_out: out,
        })
        .collect();
    out.sort_by_key(|r| r.bucket_start);
    // 桶数超上限时保留最近的(与明细段 ORDER BY bucket DESC LIMIT 对齐)
    if out.len() > QUERY_LIMIT {
        out.drain(..out.len() - QUERY_LIMIT);
    }
    tracing::debug!("[History] [query] 用量统计表快路径命中");
    Some(out)
}

/// 窗口日桶分解:(统计表段起始桶, 今天桶)。
/// seg_day = 窗口起点所在桶(对齐桶起点)或其下一桶(未对齐,部分桶
/// 留给明细补段);None = 覆盖的已完结天有缺块(回退)
fn window_days(db: &Db, f: &Filter, tz: i64) -> Option<(i64, i64)> {
    let today = (unix_now() as i64 + tz).div_euclid(86400);
    let start = f.start as i64;
    let aligned = (start + tz).rem_euclid(86400) == 0;
    let seg_day = if aligned {
        (start + tz).div_euclid(86400)
    } else {
        (start + tz).div_euclid(86400) + 1
    };
    if !days_covered(db, seg_day, today, tz) {
        return None;
    }
    Some((seg_day, today))
}

/// 检查 [lo, hi) 的已完结天是否全部物化(库中无数据的天也已标记;
/// 窗口起点早于库中最早数据时以最早数据起算)
fn days_covered(db: &Db, lo: i64, hi: i64, tz: i64) -> bool {
    if lo >= hi {
        return true;
    }
    let first: Option<i64> = db
        .query_row("SELECT MIN(last_seen) FROM conn_events", [], |r| r.get(0))
        .unwrap_or(None);
    let Some(first) = first else { return true };
    let lo = lo.max((first + tz).div_euclid(86400));
    if lo >= hi {
        return true;
    }
    let got: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM stats_done WHERE day >= ?1 AND day < ?2",
            [lo, hi],
            |r| r.get(0),
        )
        .unwrap_or(0);
    got == hi - lo
}

/// 汇总统计表段:按进程聚合(键列下推 process/proto/net_class 过滤)
fn proc_stats_rows(db: &Db, f: &Filter, lo: i64, hi: i64) -> Option<Vec<SummaryRow>> {
    let mut sql = String::from(
        "SELECT process, MAX(rep_path), SUM(bytes_out), SUM(bytes_in), SUM(n), SUM(total_secs)
         FROM proc_stats WHERE day >= ?1 AND day < ?2",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(lo), Box::new(hi)];
    // 参数编号按实际存在的条件递增分配:固定 ?3/?4 会在"无进程筛选
    // 但有协议筛选"时空悬,快路径静默 miss
    let mut idx = 2;
    if !f.process.is_empty() {
        idx += 1;
        sql.push_str(&format!(
            " AND process LIKE '%' || ?{idx} || '%' ESCAPE '\\'"
        ));
        args.push(Box::new(escape_like(&f.process)));
    }
    if let Some(p) = f.proto {
        idx += 1;
        sql.push_str(&format!(" AND proto = ?{idx}"));
        args.push(Box::new(p.as_str().to_owned()));
    }
    if f.hide_local {
        sql.push_str(" AND net_class != 0");
    }
    if f.hide_lan {
        sql.push_str(" AND net_class != 1");
    }
    sql.push_str(" GROUP BY process");
    let mut stmt = db.prepare(&sql).ok()?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(args), |row| {
            let path: String = row.get(1)?;
            Ok(SummaryRow {
                process: row.get(0)?,
                proc_path: (!path.is_empty()).then_some(path),
                bytes_out: row.get::<_, i64>(2)?.max(0) as u64,
                bytes_in: row.get::<_, i64>(3)?.max(0) as u64,
                count: row.get::<_, i64>(4)?.max(0) as u64,
                total_secs: row.get::<_, i64>(5)?.max(0) as u64,
            })
        })
        .ok()?;
    Some(rows.filter_map(|r| r.ok()).collect())
}

/// 用量统计表段的筛选条件段(proto 参数化绑定,net_class 键列字面)
fn class_cond(f: &Filter) -> String {
    let mut s = String::new();
    if f.proto.is_some() {
        s.push_str(" AND proto = ?3");
    }
    if f.hide_local {
        s.push_str(" AND net_class != 0");
    }
    if f.hide_lan {
        s.push_str(" AND net_class != 1");
    }
    s
}

/// 汇总补段:明细窗口 [lo, hi) 按进程聚合(头部部分桶与今天段共用)。
/// 参数:?1=窗口下界,?2..?9=筛选段(见 Filter::cond_clause),
/// ?10=窗口上界(None = 开区间)
fn summary_seg(db: &Db, f: &Filter, lo: u64, hi: Option<u64>) -> Option<Vec<SummaryRow>> {
    let sql = format!(
        "SELECT process, MAX(proc_path), SUM(bytes_out), SUM(bytes_in),
                COUNT(*), SUM(last_seen - first_seen)
         FROM conn_events
         WHERE last_seen >= ?1 AND (?10 IS NULL OR last_seen < ?10)
         {}
         GROUP BY process",
        Filter::cond_clause()
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(lo as i64)];
    args.extend(f.bind_tail());
    args.push(Box::new(hi.map(|v| v as i64)));
    let mut stmt = db.prepare(&sql).ok()?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(args), |row| {
            Ok(SummaryRow {
                process: row.get(0)?,
                proc_path: row.get(1)?,
                bytes_out: row.get::<_, i64>(2)?.max(0) as u64,
                bytes_in: row.get::<_, i64>(3)?.max(0) as u64,
                count: row.get::<_, i64>(4)?.max(0) as u64,
                total_secs: row.get::<_, i64>(5)?.max(0) as u64,
            })
        })
        .ok()?;
    Some(rows.filter_map(|r| r.ok()).collect())
}

/// 用量补段:明细窗口 [lo, hi) 按查询桶宽分桶(与统计表/慢路径分桶
/// 同口径:桶号 = (last_seen + tz) / 桶宽,行计入最后活跃桶)。
/// 参数:?1=下界,?2..?9=筛选段,?10=上界,?11=tz
fn usage_seg(
    db: &Db,
    f: &Filter,
    lo: u64,
    hi: Option<u64>,
    bucket_secs: u64,
    tz: i64,
) -> Option<Vec<UsageRow>> {
    let sql = format!(
        "SELECT (last_seen + ?11) / {bucket_secs} AS bucket,
                SUM(bytes_in), SUM(bytes_out)
         FROM conn_events
         WHERE last_seen >= ?1 AND (?10 IS NULL OR last_seen < ?10)
         {}
         GROUP BY bucket ORDER BY bucket DESC LIMIT {QUERY_LIMIT}",
        Filter::cond_clause()
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(lo as i64)];
    args.extend(f.bind_tail());
    args.push(Box::new(hi.map(|v| v as i64)));
    args.push(Box::new(tz));
    let mut stmt = db.prepare(&sql).ok()?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(args), |row| {
            let bucket: i64 = row.get(0)?;
            Ok(UsageRow {
                bucket_start: (bucket * bucket_secs as i64 - tz).max(0) as u64,
                bytes_in: row.get::<_, i64>(1)?.max(0) as u64,
                bytes_out: row.get::<_, i64>(2)?.max(0) as u64,
            })
        })
        .ok()?;
    let mut out: Vec<UsageRow> = rows.filter_map(|r| r.ok()).collect();
    out.reverse();
    Some(out)
}

/// 汇总行按进程合并(统计表段与补段相加;proc_path 缺失方让位)
fn merge_summary(
    rows: &mut Vec<SummaryRow>,
    index: &mut HashMap<String, usize>,
    more: Vec<SummaryRow>,
) {
    for r in more {
        match index.get(&r.process) {
            Some(&i) => {
                let dst = &mut rows[i];
                dst.bytes_out += r.bytes_out;
                dst.bytes_in += r.bytes_in;
                dst.count += r.count;
                dst.total_secs += r.total_secs;
                if dst.proc_path.is_none() {
                    dst.proc_path = r.proc_path;
                }
            }
            None => {
                index.insert(r.process.clone(), rows.len());
                rows.push(r);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::query::{query_summary, query_usage};
    use super::*;
    use rusqlite::Connection;

    /// 快慢路径对拍环境:内存库 + 最小表集(conn_events + 统计三表),
    /// 表结构取自 storage/db.rs v2/v5/v9/v10 迁移
    fn setup_db() -> Connection {
        let db = Connection::open_in_memory().expect("内存库打开失败");
        db.execute_batch(
            "CREATE TABLE conn_events (
                event_id INTEGER NOT NULL,
                first_seen INTEGER NOT NULL,
                last_seen INTEGER NOT NULL,
                pid INTEGER NOT NULL,
                process TEXT NOT NULL,
                proc_path TEXT,
                signed TEXT NOT NULL,
                proto TEXT NOT NULL,
                remote_ip INTEGER NOT NULL,
                remote_port INTEGER NOT NULL,
                bytes_in INTEGER NOT NULL DEFAULT 0,
                bytes_out INTEGER NOT NULL DEFAULT 0,
                remote_ip6 BLOB
            );
            CREATE INDEX idx_conn_events_last ON conn_events(last_seen);
            CREATE TABLE proc_stats (
                day INTEGER NOT NULL, process TEXT NOT NULL, proto TEXT NOT NULL,
                net_class INTEGER NOT NULL, n INTEGER NOT NULL, total_secs INTEGER NOT NULL,
                bytes_in INTEGER NOT NULL, bytes_out INTEGER NOT NULL,
                rep_path TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (day, process, proto, net_class)
            );
            CREATE TABLE usage_stats (
                hour INTEGER NOT NULL, net_class INTEGER NOT NULL, proto TEXT NOT NULL,
                bytes_in INTEGER NOT NULL, bytes_out INTEGER NOT NULL,
                PRIMARY KEY (hour, net_class, proto)
            );
            CREATE TABLE stats_done (day INTEGER PRIMARY KEY);",
        )
        .expect("建表失败");
        db
    }

    /// 插入一条公网 TCP 明细并按写线程同口径物化其所属天(桶号 =
    /// (last_seen + tz)/86400|3600,net_class 公网 2;今天的事件留给
    /// 调用方按"今天永不物化"跳过)
    fn insert_event(db: &Db, event_id: i64, last_seen: u64, process: &str, tz: i64) {
        let first = (last_seen - 60) as i64;
        let last = last_seen as i64;
        db.execute(
            "INSERT INTO conn_events (event_id, first_seen, last_seen, pid, process,
                proc_path, signed, proto, remote_ip, remote_port, bytes_in, bytes_out, remote_ip6)
             VALUES (?1, ?2, ?3, 42, ?4, 'C:\\\\p.exe', 'signed', 'TCP', 16909060, 443, 100, 200, NULL)",
            rusqlite::params![event_id, first, last, process],
        )
        .unwrap();
        let day = (last + tz).div_euclid(86400);
        let hour = (last + tz).div_euclid(3600);
        db.execute(
            "INSERT INTO proc_stats (day, process, proto, net_class, n, total_secs,
                bytes_in, bytes_out, rep_path)
             VALUES (?1, ?2, 'TCP', 2, 1, 60, 100, 200, 'C:\\\\p.exe')
             ON CONFLICT (day, process, proto, net_class) DO UPDATE SET
                n = n + 1, total_secs = total_secs + 60,
                bytes_in = bytes_in + 100, bytes_out = bytes_out + 200",
            rusqlite::params![day, process],
        )
        .unwrap();
        db.execute(
            "INSERT INTO usage_stats (hour, net_class, proto, bytes_in, bytes_out)
             VALUES (?1, 2, 'TCP', 100, 200)
             ON CONFLICT (hour, net_class, proto) DO UPDATE SET
                bytes_in = bytes_in + 100, bytes_out = bytes_out + 200",
            [hour],
        )
        .unwrap();
        db.execute("INSERT OR IGNORE INTO stats_done (day) VALUES (?1)", [day])
            .unwrap();
    }

    fn f(start: u64) -> Filter {
        Filter {
            start,
            process: String::new(),
            remote: None,
            proto: None,
            hide_local: false,
            hide_lan: false,
        }
    }

    fn summary_tuples(rows: &[SummaryRow]) -> Vec<(String, u64, u64, u64, u64)> {
        let mut v: Vec<_> = rows
            .iter()
            .map(|r| {
                (
                    r.process.clone(),
                    r.bytes_out,
                    r.bytes_in,
                    r.count,
                    r.total_secs,
                )
            })
            .collect();
        v.sort();
        v
    }

    fn usage_tuples(rows: &[UsageRow]) -> Vec<(u64, u64, u64)> {
        rows.iter()
            .map(|r| (r.bucket_start, r.bytes_in, r.bytes_out))
            .collect()
    }

    /// 今天已流逝时间的 1/2 处:恒在今天且非零点(0 点刚过亦成立),
    /// 触发 window_days 的 seg_day 越过 today 分支
    fn mid_today(now: u64, tz: i64) -> u64 {
        let today_start = ((now as i64 + tz).div_euclid(86400) * 86400 - tz).max(0) as u64;
        today_start + (now - today_start) / 2
    }

    /// 回归:窗口起点落在今天非零点(近 1 小时恒触发)时,今天段以窗口
    /// 起点为下界,快慢路径对拍一致(修复前今天段从零点起算,窗口起点
    /// 之前的今天数据被双计且混入结果)
    #[test]
    fn fast_path_matches_slow_when_window_starts_mid_today() {
        let db = setup_db();
        let tz = local_tz_offset_secs();
        let now = unix_now();
        let start = mid_today(now, tz);
        // 窗口外今天数据(早于窗口起点)与窗口内数据,进程区分可辨
        insert_event(&db, 1, start - 600, "early.exe", tz);
        insert_event(&db, 2, now - 60, "late.exe", tz);
        let fa = f(start);
        let fast =
            try_query_summary_stats(&db, &fa, SummarySort::BytesOut, false).expect("应命中快路径");
        let slow = query_summary(&db, &fa, SummarySort::BytesOut, false).expect("慢路径");
        assert_eq!(summary_tuples(&fast), summary_tuples(&slow));
        assert_eq!(
            summary_tuples(&fast),
            vec![("late.exe".into(), 200, 100, 1, 60)]
        );

        for bucket in [86400u64, 3600] {
            let fast = try_query_usage_stats(&db, &fa, bucket, tz).expect("应命中快路径");
            let slow = query_usage(&db, &fa, bucket, tz).expect("慢路径");
            assert_eq!(usage_tuples(&fast), usage_tuples(&slow));
        }
    }

    /// 今天零点起(今天档)与多天对齐窗口(统计表段参与):快慢路径对拍一致
    #[test]
    fn fast_path_matches_slow_for_today_and_multi_day() {
        let db = setup_db();
        let tz = local_tz_offset_secs();
        let now = unix_now();
        let today = (now as i64 + tz).div_euclid(86400);
        let today_start = (today * 86400 - tz).max(0) as u64;
        insert_event(&db, 1, today_start + 3600, "t.exe", tz);
        insert_event(&db, 2, now - 2 * 86400, "h.exe", tz);
        insert_event(&db, 3, now - 3 * 86400, "out.exe", tz);

        // 今天档:窗口 = 今天零点
        let fa = f(today_start);
        let fast =
            try_query_summary_stats(&db, &fa, SummarySort::BytesIn, false).expect("应命中快路径");
        let slow = query_summary(&db, &fa, SummarySort::BytesIn, false).expect("慢路径");
        assert_eq!(summary_tuples(&fast), summary_tuples(&slow));
        assert_eq!(
            summary_tuples(&fast),
            vec![("t.exe".into(), 200, 100, 1, 60)]
        );

        // 多天对齐窗口(窗口起点 = 2 天前零点):统计表段 + 今天段。
        // 昨天无数据,补空天物化标记(写线程对无数据天也标记)
        db.execute(
            "INSERT OR IGNORE INTO stats_done (day) VALUES (?1)",
            [today - 1],
        )
        .unwrap();
        let fa = f(((today - 2) * 86400 - tz).max(0) as u64);
        let fast =
            try_query_summary_stats(&db, &fa, SummarySort::Count, false).expect("应命中快路径");
        let slow = query_summary(&db, &fa, SummarySort::Count, false).expect("慢路径");
        assert_eq!(summary_tuples(&fast), summary_tuples(&slow));
        assert_eq!(
            summary_tuples(&fast),
            vec![
                ("h.exe".into(), 200, 100, 1, 60),
                ("t.exe".into(), 200, 100, 1, 60)
            ]
        );

        for bucket in [86400u64, 3600] {
            let fast = try_query_usage_stats(&db, &fa, bucket, tz).expect("应命中快路径");
            let slow = query_usage(&db, &fa, bucket, tz).expect("慢路径");
            assert_eq!(usage_tuples(&fast), usage_tuples(&slow));
        }
    }

    /// 未对齐的历史窗口(头部补段非空,覆盖 [窗口起点, 次日零点) 部分桶)
    #[test]
    fn fast_path_matches_slow_for_unaligned_history_window() {
        let db = setup_db();
        let tz = local_tz_offset_secs();
        let now = unix_now();
        let today = (now as i64 + tz).div_euclid(86400);
        let yesterday_start = ((today - 1) * 86400 - tz).max(0) as u64;
        insert_event(&db, 1, yesterday_start + 12 * 3600, "h.exe", tz);
        insert_event(&db, 2, yesterday_start + 20 * 3600, "l.exe", tz);
        insert_event(&db, 3, now - 60, "t.exe", tz);

        // 窗口 = 昨天 18:00:head 补段含 20 点批,tail 含今天批
        let fa = f(yesterday_start + 18 * 3600);
        let fast =
            try_query_summary_stats(&db, &fa, SummarySort::BytesOut, false).expect("应命中快路径");
        let slow = query_summary(&db, &fa, SummarySort::BytesOut, false).expect("慢路径");
        assert_eq!(summary_tuples(&fast), summary_tuples(&slow));
        assert_eq!(
            summary_tuples(&fast),
            vec![
                ("l.exe".into(), 200, 100, 1, 60),
                ("t.exe".into(), 200, 100, 1, 60)
            ]
        );

        for bucket in [86400u64, 3600] {
            let fast = try_query_usage_stats(&db, &fa, bucket, tz).expect("应命中快路径");
            let slow = query_usage(&db, &fa, bucket, tz).expect("慢路径");
            assert_eq!(usage_tuples(&fast), usage_tuples(&slow));
        }
    }
}
