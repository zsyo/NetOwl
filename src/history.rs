//! 连接历史落盘(conn_events 事件表,见 db.rs v2 迁移)。
//!
//! 写入模型:连接消失时整行落盘(每行自含 first/last_seen),后台写线程
//! 批量事务执行,UI 线程不受磁盘波动影响;运行中的连接不落盘,托盘退出
//! 时统一补写,崩溃最多丢最近活跃连接。归属地不落库(geoip 数据会随重建
//! 漂移),渲染时实时反查。查询与历史页状态见 history_query。

use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, UNIX_EPOCH};

use rusqlite::params;

pub use rusqlite::Connection as Db;

use crate::model::{Connection, Protocol, Signing};

/// 自动清理检查间隔
const CLEANUP_INTERVAL: Duration = Duration::from_secs(3600);

/// 当前 unix 秒
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 一条已完结连接的历史事件(Tracker 跟踪期间持有全部字段,关闭时补 last_seen)
pub struct ClosedConn {
    pub event_id: u64,
    pub first_seen: u64,
    pub last_seen: u64,
    pub pid: u32,
    pub process: String,
    pub proc_path: Option<String>,
    pub signed: Signing,
    pub proto: Protocol,
    pub remote_ip: Ipv4Addr,
    pub remote_port: u16,
}

/// 连接快照对比器:跟踪活跃连接,消失时生成完结事件。
/// mock 等非真实数据源不入库(切换时清空跟踪,切回后全量重新登记)
pub struct Tracker {
    index: HashMap<u64, ClosedConn>,
}

impl Tracker {
    pub fn new() -> Self {
        Tracker {
            index: HashMap::new(),
        }
    }

    /// 对比当前活跃快照,返回本轮完结的连接
    pub fn diff(&mut self, real: bool, conns: &[Connection], now: u64) -> Vec<ClosedConn> {
        if !real {
            self.index.clear();
            return Vec::new();
        }
        let mut current: HashSet<u64> = HashSet::with_capacity(conns.len());
        for c in conns {
            current.insert(c.id);
            self.index.entry(c.id).or_insert_with(|| ClosedConn {
                event_id: c.id,
                first_seen: now,
                last_seen: now,
                pid: c.pid,
                process: c.process.clone(),
                proc_path: c.proc_path.clone(),
                signed: c.signed,
                proto: c.proto,
                remote_ip: c.remote_ip,
                remote_port: c.remote_port,
            });
        }
        let gone: Vec<u64> = self
            .index
            .keys()
            .filter(|id| !current.contains(id))
            .copied()
            .collect();
        gone.into_iter()
            .filter_map(|id| self.index.remove(&id))
            .map(|mut e| {
                e.last_seen = now;
                e
            })
            .collect()
    }

    /// 退出收尾:把仍被跟踪的连接全部按已完结落盘
    pub fn flush(&mut self, now: u64) -> Vec<ClosedConn> {
        self.index
            .drain()
            .map(|(_, mut e)| {
                e.last_seen = now;
                e
            })
            .collect()
    }
}

enum Msg {
    Events(Vec<ClosedConn>),
    Purge(u32),
}

/// 历史写线程句柄;退出前调用 shutdown 确保队列清空
pub struct Writer {
    tx: Option<mpsc::Sender<Msg>>,
    handle: Option<JoinHandle<()>>,
    /// 自动清理天数(0 = 不清理),app 侧同步 config
    retention: std::sync::Arc<AtomicU32>,
}

impl Writer {
    pub fn spawn(retention_days: u32) -> Writer {
        let (tx, rx) = mpsc::channel();
        let retention = std::sync::Arc::new(AtomicU32::new(retention_days));
        let r = std::sync::Arc::clone(&retention);
        let handle = std::thread::Builder::new()
            .name("history-writer".into())
            .spawn(move || run(rx, r))
            .expect("启动历史写线程");
        Writer {
            tx: Some(tx),
            handle: Some(handle),
            retention,
        }
    }

    /// 同步自动清理天数(config 变更时调用)
    pub fn set_retention(&self, days: u32) {
        self.retention.store(days, Ordering::Relaxed);
    }

    pub fn send(&self, events: Vec<ClosedConn>) {
        if events.is_empty() {
            return;
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::Events(events));
        }
    }

    /// 请求清空 N 天前的历史(写线程执行,完成后由调用方刷新展示)
    pub fn purge(&self, days: u32) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::Purge(days));
        }
    }

    /// 关闭通道并等待写线程处理完剩余消息
    pub fn shutdown(&mut self) {
        self.tx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run(rx: Receiver<Msg>, retention: std::sync::Arc<AtomicU32>) {
    let mut conn = crate::db::open();
    let mut last_cleanup = Instant::now();
    loop {
        // 先等第一条消息,再把已排队的全部收齐,单轮统一处理
        let first = match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(m) => m,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                cleanup(&conn, &retention, &mut last_cleanup);
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let mut events = Vec::new();
        let mut purge = None;
        match first {
            Msg::Events(mut e) => events.append(&mut e),
            Msg::Purge(days) => purge = Some(days),
        }
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Events(mut e) => events.append(&mut e),
                Msg::Purge(days) => purge = Some(days),
            }
        }
        write_batch(&mut conn, &events);
        if let Some(days) = purge {
            purge_before(&conn, days);
        }
        cleanup(&conn, &retention, &mut last_cleanup);
    }
}

/// 批量写入(单事务);失败输出错误,不中断写线程
fn write_batch(conn: &mut Db, events: &[ClosedConn]) {
    if events.is_empty() {
        return;
    }
    let tx = match conn.transaction() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[History] 开启事务失败: {e}");
            return;
        }
    };
    for e in events {
        if let Err(e) = tx.execute(
            "INSERT INTO conn_events (event_id, first_seen, last_seen, pid, process, proc_path, signed, proto, remote_ip, remote_port)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                e.event_id as i64,
                e.first_seen as i64,
                e.last_seen as i64,
                e.pid,
                e.process,
                e.proc_path,
                e.signed.as_str(),
                e.proto.as_str(),
                u32::from(e.remote_ip),
                e.remote_port
            ],
        ) {
            eprintln!("[History] 写入连接历史失败: {e}");
        }
    }
    if let Err(e) = tx.commit() {
        eprintln!("[History] 提交历史事务失败: {e}");
    }
}

/// 自动清理:到达检查间隔且 retention > 0 时删除超期行
fn cleanup(conn: &Db, retention: &AtomicU32, last_cleanup: &mut Instant) {
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
            eprintln!("[History] 自动清理 {days} 天前历史 {n} 行");
            reclaim_space(conn);
        }
        Ok(_) => {}
        Err(e) => eprintln!("[History] 自动清理失败: {e}"),
    }
}

/// 清理 N 天前的数据(0 = 清空全部),随后归还释放的空间
fn purge_before(conn: &Db, days: u32) {
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
                eprintln!("[History] 已清空全部历史 {n} 行");
            } else {
                eprintln!("[History] 已清理 {days} 天前历史 {n} 行");
            }
            reclaim_space(conn);
        }
        Err(e) => eprintln!("[History] 清理历史失败: {e}"),
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
        eprintln!("[History] 归还空闲页失败: {e}");
    }
    if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
        eprintln!("[History] 截断 WAL 失败: {e}");
    }
}

/// netowl.db(+wal)当前字节数;文件尚未创建时为 0
pub fn db_size() -> u64 {
    let base = Path::new(crate::paths::DATA_DIR).join("netowl.db");
    let mut size = std::fs::metadata(&base).map(|m| m.len()).unwrap_or(0);
    let wal = base.with_file_name("netowl.db-wal");
    size += std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
    size
}
