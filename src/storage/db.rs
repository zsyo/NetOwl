//! SQLite 数据库:连接历史、规则等持久化的基础存储。
//! 使用 rusqlite bundled(内置 SQLite,免系统依赖),WAL 模式。
//! 所有含外部输入的查询必须使用参数绑定(rusqlite `params!`),禁止字符串拼接 SQL。

use rusqlite::{Connection, Transaction};

use crate::platform::paths::DATA_DIR;

/// 打开(或创建)数据库并执行迁移
pub fn open() -> Connection {
    let path = std::path::Path::new(DATA_DIR).join("netowl.db");
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        panic!("[Db] 创建数据目录失败: {e}");
    }
    let mut conn = Connection::open(&path).unwrap_or_else(|e| panic!("[Db] 打开数据库失败: {e}"));
    conn.pragma_update(None, "journal_mode", "WAL")
        .unwrap_or_else(|e| panic!("[Db] 设置 WAL 模式失败: {e}"));
    // 写者等待上限:WAL 下读者不互斥,但写线程批量提交与 UI 侧写操作
    // (规则 CRUD/历史删除/lan 合并)撞车时,无等待会立即 SQLITE_BUSY,
    // 写线程整批事件提交失败、历史静默丢失
    conn.busy_timeout(std::time::Duration::from_secs(3))
        .unwrap_or_else(|e| panic!("[Db] 设置 busy_timeout 失败: {e}"));
    migrate(&mut conn);
    let version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .expect("[Db] 读取 schema 版本失败");
    tracing::info!("[Db] 已打开 {},schema v{version}", path.display());
    conn
}

/// 执行一个版本步骤:DDL 与 schema_version 写入包在同一事务里,中途失败
/// 整体回滚。否则升级过程崩溃(断电/强杀)后,下次启动重跑 ALTER TABLE
/// 报 duplicate column 直接 panic,应用永远无法启动。
/// SQLite 的 DDL 是事务性的(VACUUM 除外,v3 单独处理)
fn step(
    conn: &mut Connection,
    version: i64,
    ddl: impl FnOnce(&Transaction) -> rusqlite::Result<()>,
) {
    let tx = conn
        .transaction()
        .unwrap_or_else(|e| panic!("[Db] 开启 v{version} 迁移事务失败: {e}"));
    ddl(&tx).unwrap_or_else(|e| panic!("[Db] 迁移到 v{version} 失败: {e}"));
    tx.execute(
        "INSERT INTO schema_version (version) VALUES (?1)",
        [version],
    )
    .unwrap_or_else(|e| panic!("[Db] 写入 schema 版本失败: {e}"));
    tx.commit()
        .unwrap_or_else(|e| panic!("[Db] 提交 v{version} 迁移事务失败: {e}"));
}

/// 结构迁移:按序执行,当前版本记录在 schema_version
fn migrate(conn: &mut Connection) {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        )",
        [],
    )
    .unwrap_or_else(|e| panic!("[Db] 创建 schema_version 表失败: {e}"));

    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
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
        // 会随重建漂移,渲染时实时反查);流量字节由版本 5 迁移补列
        step(conn, 2, |tx| {
            tx.execute(
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
            )?;
            tx.execute(
                "CREATE INDEX IF NOT EXISTS idx_conn_events_last ON conn_events(last_seen)",
                [],
            )?;
            tx.execute(
                "CREATE INDEX IF NOT EXISTS idx_conn_events_first ON conn_events(first_seen)",
                [],
            )?;
            Ok(())
        });
    }
    if current < 3 {
        // 版本 3:开启增量 auto_vacuum,使清理删除的空间可归还文件系统
        // (INCREMENTAL 回收由 history.rs 清理后显式触发,写放大小于 FULL)。
        // auto_vacuum 只能对空库直接生效,已有数据的库须 VACUUM 重建一次;
        // main 先于其余连接 open,本迁移天然串行且仅执行一次。
        // VACUUM 不可运行在事务中,本步不包事务:pragma 与 VACUUM 均幂等,
        // 崩溃重跑安全(版本行未写入则下次补写,已写入则整步跳过)
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")
            .unwrap_or_else(|e| panic!("[Db] 设置 auto_vacuum 失败: {e}"));
        conn.execute("VACUUM", [])
            .unwrap_or_else(|e| panic!("[Db] auto_vacuum 迁移重建失败: {e}"));
        conn.execute("INSERT INTO schema_version (version) VALUES (3)", [])
            .unwrap_or_else(|e| panic!("[Db] 写入 schema 版本失败: {e}"));
    }
    if current < 4 {
        // 版本 4:规则表(模型与求值见 rules.rs)。枚举列存规范字符串
        // (action/direction/remote_kind,proto 空串 = 任意,port 0 = 任意)
        step(conn, 4, |tx| {
            tx.execute(
                "CREATE TABLE IF NOT EXISTS rules (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    priority INTEGER NOT NULL,
                    action TEXT NOT NULL,
                    direction TEXT NOT NULL,
                    proto TEXT NOT NULL DEFAULT '',
                    process TEXT NOT NULL DEFAULT '',
                    remote_kind TEXT NOT NULL DEFAULT 'any',
                    remote_value TEXT NOT NULL DEFAULT '',
                    port INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL
                )",
                [],
            )?;
            Ok(())
        });
    }
    if current < 5 {
        // 版本 5:conn_events 补流量字节列(ETW 合并的连接级收发字节,
        // 连接完结时定稿)。存量行为 0:升级前无字节语义,显示按 0 B 处理
        step(conn, 5, |tx| {
            tx.execute(
                "ALTER TABLE conn_events ADD COLUMN bytes_in INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
            tx.execute(
                "ALTER TABLE conn_events ADD COLUMN bytes_out INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
            Ok(())
        });
    }
    if current < 6 {
        // 版本 6:局域网设备表(ARP 发现的邻居持久化;MAC 主键,首见/
        // 最近在线时间驱动"新设备"判定,ip 存主机序 u32)
        step(conn, 6, |tx| {
            tx.execute(
                "CREATE TABLE lan_devices (
                    mac TEXT PRIMARY KEY,
                    first_seen INTEGER NOT NULL,
                    last_seen INTEGER NOT NULL,
                    ip INTEGER NOT NULL
                )",
                [],
            )?;
            Ok(())
        });
    }
    if current < 7 {
        // 版本 7:规则 Profile 配置档。profiles 表(名称可重命名,默认档
        // 由 app 启动时按启动语言创建)与 rules 归属列;存量规则归默认档 1
        step(conn, 7, |tx| {
            tx.execute(
                "CREATE TABLE profiles (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                )",
                [],
            )?;
            tx.execute(
                "ALTER TABLE rules ADD COLUMN profile_id INTEGER NOT NULL DEFAULT 1",
                [],
            )?;
            Ok(())
        });
    }
}
