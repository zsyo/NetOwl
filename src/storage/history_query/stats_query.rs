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
    // 头部补段:窗口起点所在部分桶(明细实时算,窗口不足一天)
    if f.start < seg_start {
        let head = summary_seg(db, f, f.start, Some(seg_start))?;
        merge_summary(&mut rows, &mut index, head);
    }
    // 今天段:从本地零点起(明细实时算)
    let tail = summary_seg(db, f, today_start, None)?;
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
    // 头部补段 + 今天段(明细实时算,分桶口径与统计表一致)
    if f.start < seg_start {
        for r in usage_seg(db, f, f.start, Some(seg_start), bucket_secs, tz)? {
            let e = buckets.entry(r.bucket_start as i64).or_default();
            e.0 += r.bytes_in;
            e.1 += r.bytes_out;
        }
    }
    for r in usage_seg(db, f, today_start, None, bucket_secs, tz)? {
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
