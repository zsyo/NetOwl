//! SQLite 数据库:连接历史、规则等持久化的基础存储。
//! 使用 rusqlite bundled(内置 SQLite,免系统依赖),WAL 模式。
//! 所有含外部输入的查询必须使用参数绑定(rusqlite `params!`),禁止字符串拼接 SQL。

use rusqlite::Connection;

use crate::paths::DATA_DIR;

/// 打开(或创建)数据库并执行迁移
pub fn open() -> Connection {
    let path = std::path::Path::new(DATA_DIR).join("netowl.db");
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        panic!("[Db] 创建数据目录失败: {e}");
    }
    let conn = Connection::open(&path).unwrap_or_else(|e| panic!("[Db] 打开数据库失败: {e}"));
    conn.pragma_update(None, "journal_mode", "WAL")
        .unwrap_or_else(|e| panic!("[Db] 设置 WAL 模式失败: {e}"));
    migrate(&conn);
    conn
}

/// 结构迁移:按序执行,当前版本记录在 schema_version
fn migrate(conn: &Connection) {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        )",
        [],
    )
    .unwrap_or_else(|e| panic!("[Db] 创建 schema_version 表失败: {e}"));

    let current: i64 = conn
        .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |row| {
            row.get(0)
        })
        .unwrap_or_else(|e| panic!("[Db] 读取 schema 版本失败: {e}"));

    // 后续迁移在此按 current 版本追加
    if current < 1 {
        // 版本 1:初始化基线,暂无业务表(规则表/连接历史表随对应功能点建立)
        conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])
            .unwrap_or_else(|e| panic!("[Db] 写入 schema 版本失败: {e}"));
    }
    if current < 2 {
        // 版本 2:连接历史事件表(每条已完结连接一行,写入见 history.rs)。
        // remote_ip 存主机序 u32 便于网段 BETWEEN;归属地不落库(geoip 数据
        // 会随重建漂移,渲染时实时反查);流量字节待 ETW 后加列
        conn.execute(
            "CREATE TABLE IF NOT EXISTS conn_events (
                event_id INTEGER NOT NULL,
                first_seen INTEGER NOT NULL,
                last_seen INTEGER NOT NULL,
                pid INTEGER NOT NULL,
                process TEXT NOT NULL,
                proc_path TEXT,
                signed TEXT NOT NULL,
                proto TEXT NOT NULL,
                remote_ip INTEGER NOT NULL,
                remote_port INTEGER NOT NULL
            )",
            [],
        )
        .unwrap_or_else(|e| panic!("[Db] 创建 conn_events 表失败: {e}"));
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conn_events_last ON conn_events(last_seen)",
            [],
        )
        .unwrap_or_else(|e| panic!("[Db] 创建 conn_events 索引失败: {e}"));
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conn_events_first ON conn_events(first_seen)",
            [],
        )
        .unwrap_or_else(|e| panic!("[Db] 创建 conn_events 索引失败: {e}"));
        conn.execute("INSERT INTO schema_version (version) VALUES (2)", [])
            .unwrap_or_else(|e| panic!("[Db] 写入 schema 版本失败: {e}"));
    }
    if current < 3 {
        // 版本 3:开启增量 auto_vacuum,使清理删除的空间可归还文件系统
        // (INCREMENTAL 回收由 history.rs 清理后显式触发,写放大小于 FULL)。
        // auto_vacuum 只能对空库直接生效,已有数据的库须 VACUUM 重建一次;
        // main 先于其余连接 open,本迁移天然串行且仅执行一次
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")
            .unwrap_or_else(|e| panic!("[Db] 设置 auto_vacuum 失败: {e}"));
        conn.execute("VACUUM", [])
            .unwrap_or_else(|e| panic!("[Db] auto_vacuum 迁移重建失败: {e}"));
        conn.execute("INSERT INTO schema_version (version) VALUES (3)", [])
            .unwrap_or_else(|e| panic!("[Db] 写入 schema 版本失败: {e}"));
    }
}
