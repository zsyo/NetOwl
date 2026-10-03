//! 历史库维护:按保留期自动清理、手动清空/按天数清理与删除后空间回收。

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use super::Db;
use super::unix_now;

/// 自动清理检查间隔
const CLEANUP_INTERVAL: Duration = Duration::from_secs(3600);

/// 自动清理:到达检查间隔且 retention > 0 时删除超期行
pub(super) fn cleanup(conn: &Db, retention: &AtomicU32, last_cleanup: &mut Instant) {
    let days = retention.load(Ordering::Relaxed);
    if days == 0 || last_cleanup.elapsed() < CLEANUP_INTERVAL {
        return;
    }
    *last_cleanup = Instant::now();
    let before = unix_now().saturating_sub(days as u64 * 86400);
    match conn.execute(
        "DELETE FROM conn_events WHERE last_seen < ?1",
        [before as i64],
    ) {
        Ok(n) if n > 0 => {
            tracing::info!("[History] 自动清理 {days} 天前历史 {n} 行");
            reclaim_space(conn);
        }
        Ok(_) => {}
        Err(e) => tracing::warn!("[History] 自动清理失败: {e}"),
    }
}

/// 清理 N 天前的数据(0 = 清空全部),随后归还释放的空间
pub(super) fn purge_before(conn: &Db, days: u32) {
    let result = if days == 0 {
        conn.execute("DELETE FROM conn_events", [])
    } else {
        let before = unix_now().saturating_sub(days as u64 * 86400);
        conn.execute(
            "DELETE FROM conn_events WHERE last_seen < ?1",
            [before as i64],
        )
    };
    match result {
        Ok(n) => {
            if days == 0 {
                tracing::info!("[History] 已清空全部历史 {n} 行");
            } else {
                tracing::info!("[History] 已清理 {days} 天前历史 {n} 行");
            }
            reclaim_space(conn);
        }
        Err(e) => tracing::warn!("[History] 清理历史失败: {e}"),
    }
}

/// 把 DELETE 释放的空闲页归还文件系统并截断 WAL,库文件尺寸随之回落;
/// 空闲页不存在时近零开销。文件可收缩依赖 INCREMENTAL auto_vacuum(见 db.rs)。
/// incremental_vacuum 每归还一批页产生一行,须消费完全部行才完成
/// (execute_batch 内部只 step 一次,会中途放弃,只归还首批)
fn reclaim_space(conn: &Db) {
    let reclaim = || -> rusqlite::Result<()> {
        let mut stmt = conn.prepare("PRAGMA incremental_vacuum")?;
        let mut rows = stmt.query([])?;
        while rows.next()?.is_some() {}
        Ok(())
    };
    if let Err(e) = reclaim() {
        tracing::warn!("[History] 归还空闲页失败: {e}");
    }
    if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
        tracing::warn!("[History] 截断 WAL 失败: {e}");
    }
}

/// netowl.db(+wal)当前字节数;文件尚未创建时为 0
pub fn db_size() -> u64 {
    let base = Path::new(crate::platform::paths::DATA_DIR).join("netowl.db");
    let mut size = std::fs::metadata(&base).map(|m| m.len()).unwrap_or(0);
    let wal = base.with_file_name("netowl.db-wal");
    size += std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
    size
}
