//! 规则 CRUD 与会话临时规则:落库插入/更新/删除/启停/优先级交换,
//! 以及询问"仅本次"的内存临时规则(负数 id,不落库)。

use rusqlite::Connection as Db;
use rusqlite::params;

use super::Rule;
use super::RuleSet;
use crate::storage::history;

impl RuleSet {
    /// 插入会话内临时规则(不落库;负数 id,优先于全部持久规则)。
    /// 新连接询问的"仅本次"拒绝决策走此入口
    pub fn insert_temp(&mut self, mut rule: Rule) {
        rule.id = self.next_temp_id;
        self.next_temp_id -= 1;
        rule.priority = self.rules.first().map_or(0, |r| r.priority - 10);
        self.rules.insert(0, rule);
    }

    /// 删除会话临时规则(仅内存,不涉及库);随其绑定的连接消失调用
    pub fn delete_temp(&mut self, id: i64) {
        self.rules.retain(|r| r.id != id);
    }

    /// 新建规则并落库,追加为最低优先级(归属当前配置档)
    pub fn insert(&mut self, db: &Db, mut rule: Rule) -> rusqlite::Result<()> {
        rule.priority = self.rules.last().map_or(10, |r| r.priority + 10);
        db.execute(
            "INSERT INTO rules (name, enabled, priority, action, direction, proto,
                                process, remote_kind, remote_value, port, created_at, profile_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                rule.name,
                rule.enabled as i64,
                rule.priority,
                rule.action.as_str(),
                rule.direction.as_str(),
                rule.proto.map(|p| p.as_str()).unwrap_or_default(),
                rule.process,
                rule.remote_kind.as_str(),
                rule.remote_value,
                rule.port as i64,
                history::unix_now() as i64,
                self.active_profile,
            ],
        )?;
        rule.id = db.last_insert_rowid();
        self.rules.push(rule);
        Ok(())
    }

    /// 覆盖更新规则并落库(含启停;重排走 move_rule)。
    /// 进程条件变化时丢弃该规则的路径粘滞缓存
    pub fn update(&mut self, db: &Db, rule: &Rule) -> rusqlite::Result<()> {
        db.execute(
            "UPDATE rules SET name=?1, enabled=?2, priority=?3, action=?4, direction=?5,
                              proto=?6, process=?7, remote_kind=?8, remote_value=?9, port=?10
             WHERE id=?11",
            params![
                rule.name,
                rule.enabled as i64,
                rule.priority,
                rule.action.as_str(),
                rule.direction.as_str(),
                rule.proto.map(|p| p.as_str()).unwrap_or_default(),
                rule.process,
                rule.remote_kind.as_str(),
                rule.remote_value,
                rule.port as i64,
                rule.id,
            ],
        )?;
        let old_process = self
            .rules
            .iter()
            .find(|r| r.id == rule.id)
            .map(|r| r.process.clone());
        if old_process.as_deref() != Some(rule.process.as_str()) {
            self.sticky_paths.remove(&rule.id);
        }
        if let Some(slot) = self.rules.iter_mut().find(|r| r.id == rule.id) {
            *slot = rule.clone();
        }
        self.rules.sort_by_key(|r| (r.priority, r.id));
        Ok(())
    }

    pub fn delete(&mut self, db: &Db, id: i64) -> rusqlite::Result<()> {
        db.execute("DELETE FROM rules WHERE id = ?1", [id])?;
        self.rules.retain(|r| r.id != id);
        self.sticky_paths.remove(&id);
        Ok(())
    }

    /// 仅切换启停(不改变优先级排序)
    pub fn set_enabled(&mut self, db: &Db, id: i64, enabled: bool) -> rusqlite::Result<()> {
        db.execute(
            "UPDATE rules SET enabled=?1 WHERE id=?2",
            params![enabled as i64, id],
        )?;
        if let Some(r) = self.rules.iter_mut().find(|r| r.id == id) {
            r.enabled = enabled;
        }
        Ok(())
    }

    /// 上移/下移:与相邻规则交换 priority(delta -1 上移 / +1 下移)
    pub fn move_rule(&mut self, db: &Db, id: i64, delta: i64) -> rusqlite::Result<()> {
        let idx = match self.rules.iter().position(|r| r.id == id) {
            Some(i) => i,
            None => return Ok(()),
        };
        let target = idx as i64 + delta;
        if target < 0 || target as usize >= self.rules.len() {
            return Ok(());
        }
        let t = target as usize;
        let (a, b) = (self.rules[idx].clone(), self.rules[t].clone());
        self.rules.swap(idx, t);
        db.execute(
            "UPDATE rules SET priority=?1 WHERE id=?2",
            params![b.priority, b.id],
        )?;
        db.execute(
            "UPDATE rules SET priority=?1 WHERE id=?2",
            params![a.priority, a.id],
        )?;
        Ok(())
    }
}
