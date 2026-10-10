//! 配置档管理:profiles 表 CRUD、按档加载持久规则、档切换(保留会话
//! 临时规则)与数据库行 -> Rule 的反序列化。

use std::collections::HashMap;

use rusqlite::Connection as Db;
use rusqlite::params;

use super::{Action, Direction, RemoteKind, Rule, RuleMatch, RuleSet};
use crate::model::Protocol;
use crate::storage::history;

/// 一条配置档(profiles 表行)
pub struct Profile {
    pub id: i64,
    pub name: String,
    pub created_at: u64,
}

pub(super) fn parse_action(s: &str) -> Action {
    if s == Action::Allow.as_str() {
        Action::Allow
    } else {
        Action::Block
    }
}

pub(super) fn parse_direction(s: &str) -> Direction {
    match s {
        "out" => Direction::Out,
        "in" => Direction::In,
        _ => Direction::Any,
    }
}

pub(super) fn parse_remote_kind(s: &str) -> RemoteKind {
    match s {
        "ip" => RemoteKind::Ip,
        "domain" => RemoteKind::Domain,
        _ => RemoteKind::Any,
    }
}

/// 空串 = 任意协议
pub(super) fn parse_proto(s: &str) -> Option<Protocol> {
    if s == Protocol::Udp.as_str() {
        Some(Protocol::Udp)
    } else if s == Protocol::Tcp.as_str() {
        Some(Protocol::Tcp)
    } else {
        None
    }
}

impl RuleSet {
    /// 确保默认档(id 1)存在(新库/迁移后首次启动)并校验请求档位有效
    /// (不存在时回退默认档);返回有效档位 id。default_name 用启动语言
    /// 取词,可重命名
    pub fn ensure_default_profile(db: &Db, requested: i64, default_name: &str) -> i64 {
        let result: rusqlite::Result<i64> = (|| {
            let n: i64 = db.query_row("SELECT COUNT(*) FROM profiles", [], |r| r.get(0))?;
            if n == 0 {
                db.execute(
                    "INSERT INTO profiles (id, name, created_at) VALUES (1, ?1, ?2)",
                    params![default_name, history::unix_now() as i64],
                )?;
                tracing::info!("[Rules] 已创建默认配置档「{default_name}」(id 1)");
            }
            let known = db
                .query_row("SELECT 1 FROM profiles WHERE id = ?1", [requested], |_| {
                    Ok(())
                })
                .map(|_| ())
                .inspect_err(|e| {
                    tracing::debug!("[Rules] 查询配置档 {requested} 存在性失败: {e}");
                })
                .is_ok();
            Ok(if known { requested } else { 1 })
        })();
        match result {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!("[Rules] 配置档校验失败,回退默认档: {e}");
                1
            }
        }
    }

    /// 读取指定档的持久规则(priority 升序、同值按 id)
    fn load_rules(db: &Db, profile_id: i64) -> Vec<Rule> {
        let mut rules = Vec::new();
        let result = (|| -> rusqlite::Result<()> {
            let mut stmt = db.prepare(
                "SELECT id, name, enabled, priority, action, direction, proto, process,
                        remote_kind, remote_value, port
                 FROM rules WHERE profile_id = ?1 ORDER BY priority ASC, id ASC",
            )?;
            let rows = stmt.query_map([profile_id], |row| {
                Ok(Rule {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    enabled: row.get::<_, i64>(2)? != 0,
                    priority: row.get(3)?,
                    action: parse_action(&row.get::<_, String>(4)?),
                    direction: parse_direction(&row.get::<_, String>(5)?),
                    proto: parse_proto(&row.get::<_, String>(6)?),
                    process: row.get(7)?,
                    remote_kind: parse_remote_kind(&row.get::<_, String>(8)?),
                    remote_value: row.get(9)?,
                    port: row.get::<_, i64>(10)? as u16,
                    local_port: 0,
                })
            })?;
            for r in rows {
                rules.push(r?);
            }
            Ok(())
        })();
        if let Err(e) = result {
            tracing::warn!("[Rules] 规则加载失败: {e}");
        }
        rules
    }

    pub fn load(db: &Db, profile_id: i64) -> RuleSet {
        let rules = Self::load_rules(db, profile_id);
        RuleSet {
            eval_cache: rules.iter().map(|r| (r.id, RuleMatch::of(r))).collect(),
            rules,
            active_profile: profile_id,
            fallback: None,
            sticky_paths: HashMap::new(),
            next_temp_id: -1,
        }
    }

    /// 切换配置档:重载目标档持久规则;会话临时规则(负 id)绑定活跃
    /// 连接、与档无关,清掉会撤销询问决策,保留
    pub fn switch_profile(&mut self, db: &Db, id: i64) {
        self.active_profile = id;
        self.sticky_paths.clear();
        let persisted = Self::load_rules(db, id);
        // 会话临时规则(负 id)必须恒定位于全部持久规则之前:求值按数组序
        // 取首个命中(eval.rs),WFP weight 也按 priority 排名。切档后持久
        // 集更换,临时规则原有的 priority 基准(旧档首条规则)失效,按新档
        // 首条规则之下重盖,维持"数组顺序 = (priority, id) 升序"不变量
        let base = persisted.first().map_or(0, |r| r.priority);
        let mut temps: Vec<Rule> = self.rules.iter().filter(|r| r.id < 0).cloned().collect();
        let n = temps.len() as i64;
        for (i, t) in temps.iter_mut().enumerate() {
            t.priority = base - 10 * (n - i as i64);
        }
        tracing::info!(
            "[Rules] 切换配置档 -> id {id},加载 {} 条规则,保留 {} 条会话临时规则",
            persisted.len(),
            temps.len()
        );
        let mut merged = temps;
        merged.extend(persisted);
        self.eval_cache = merged.iter().map(|r| (r.id, RuleMatch::of(r))).collect();
        self.rules = merged;
    }

    /// 配置档列表(id 升序)
    pub fn list_profiles(db: &Db) -> Vec<Profile> {
        let mut out = Vec::new();
        let result = (|| -> rusqlite::Result<()> {
            let mut stmt = db.prepare("SELECT id, name, created_at FROM profiles ORDER BY id")?;
            let rows = stmt.query_map([], |row| {
                Ok(Profile {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: row.get::<_, i64>(2)?.max(0) as u64,
                })
            })?;
            for p in rows {
                out.push(p?);
            }
            Ok(())
        })();
        if let Err(e) = result {
            tracing::warn!("[Rules] 配置档列表读取失败: {e}");
        }
        out
    }

    /// 新建配置档,返回新 id
    pub fn create_profile(db: &Db, name: &str) -> rusqlite::Result<i64> {
        db.execute(
            "INSERT INTO profiles (name, created_at) VALUES (?1, ?2)",
            params![name, history::unix_now() as i64],
        )?;
        Ok(db.last_insert_rowid())
    }

    pub fn rename_profile(db: &Db, id: i64, name: &str) -> rusqlite::Result<()> {
        db.execute(
            "UPDATE profiles SET name = ?1 WHERE id = ?2",
            params![name, id],
        )?;
        Ok(())
    }

    /// 删除配置档连同其全部规则;当前档不允许删(调用方保证)
    pub fn delete_profile(db: &Db, id: i64) -> rusqlite::Result<()> {
        db.execute("DELETE FROM rules WHERE profile_id = ?1", [id])?;
        db.execute("DELETE FROM profiles WHERE id = ?1", [id])?;
        Ok(())
    }

    /// 复制配置档:src 档的全部规则复制到新档,返回新档 id
    pub fn copy_profile(db: &Db, src: i64, name: &str) -> rusqlite::Result<i64> {
        let dst = Self::create_profile(db, name)?;
        db.execute(
            "INSERT INTO rules (name, enabled, priority, action, direction, proto, process,
                                remote_kind, remote_value, port, created_at, profile_id)
             SELECT name, enabled, priority, action, direction, proto, process,
                    remote_kind, remote_value, port, created_at, ?1
             FROM rules WHERE profile_id = ?2",
            params![dst, src],
        )?;
        Ok(dst)
    }

    /// 当前档规则数
    pub fn count_rules(db: &Db, profile_id: i64) -> usize {
        db.query_row(
            "SELECT COUNT(*) FROM rules WHERE profile_id = ?1",
            [profile_id],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n as usize)
        .unwrap_or(0)
    }
}
