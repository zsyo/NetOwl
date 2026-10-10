//! 规则 CRUD 与会话临时规则:落库插入/更新/删除/启停/优先级交换,
//! 以及询问"仅本次"的内存临时规则(负数 id,不落库)。

use rusqlite::Connection as Db;
use rusqlite::params;

use super::{Rule, RuleMatch, RuleSet};
use crate::storage::history;

impl RuleSet {
    /// 插入会话内临时规则(不落库;负数 id,优先于全部持久规则)。
    /// 新连接询问的"仅本次"拒绝决策走此入口
    pub fn insert_temp(&mut self, mut rule: Rule) {
        rule.id = self.next_temp_id;
        self.next_temp_id -= 1;
        rule.priority = self.rules.first().map_or(0, |r| r.priority - 10);
        self.eval_cache.insert(rule.id, RuleMatch::of(&rule));
        self.rules.insert(0, rule);
    }

    /// 删除会话临时规则(仅内存,不涉及库);随其绑定的连接消失调用
    pub fn delete_temp(&mut self, id: i64) {
        self.rules.retain(|r| r.id != id);
    }

    /// 新建规则并落库,追加为最低优先级(归属当前配置档)。
    /// 基准取持久规则(正 id)的最大 priority:会话临时规则恒定位于数组
    /// 头部(见 switch_profile/insert_temp),不能用 last()
    pub fn insert(&mut self, db: &Db, mut rule: Rule) -> rusqlite::Result<()> {
        rule.priority = self
            .rules
            .iter()
            .filter(|r| r.id > 0)
            .map(|r| r.priority)
            .max()
            .map_or(10, |p| p + 10);
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
        self.eval_cache.insert(rule.id, RuleMatch::of(&rule));
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
        self.eval_cache.insert(rule.id, RuleMatch::of(rule));
        if let Some(slot) = self.rules.iter_mut().find(|r| r.id == rule.id) {
            *slot = rule.clone();
        }
        self.rules.sort_by_key(|r| (r.priority, r.id));
        Ok(())
    }

    pub fn delete(&mut self, db: &Db, id: i64) -> rusqlite::Result<()> {
        db.execute("DELETE FROM rules WHERE id = ?1", [id])?;
        self.rules.retain(|r| r.id != id);
        self.eval_cache.remove(&id);
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

    /// 上移/下移:与相邻规则交换 priority(delta -1 上移 / +1 下移)。
    /// 交换的是 priority 值本身,内存与库同步后按 priority 重排,
    /// 保证数组顺序与排序键(求值/加载顺序)一致。
    /// 会话临时规则(负 id)不参与:它们恒定位于持久规则之前且不落库,
    /// 操作行或目标行是临时规则时直接返回——否则会把临时规则的极负
    /// priority 通过 UPDATE 写进持久规则的库行
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
        if self.rules[idx].id < 0 || self.rules[t].id < 0 {
            return Ok(());
        }
        let (a_id, b_id) = (self.rules[idx].id, self.rules[t].id);
        let (a_pri, b_pri) = (self.rules[idx].priority, self.rules[t].priority);
        // 先库后内存(与 update/delete 一致):两条 UPDATE 包在同一 SAVEPOINT
        // 里,任一失败整体回滚,内存不动,避免库与内存分叉。
        // rusqlite 的 transaction() 需 &mut 连接,而 UI 层经 UiCtx 全页面
        // 共享 &Db,SAVEPOINT 经 execute_batch 在 &self 上完成同样语义
        db.execute_batch("SAVEPOINT move_rule")?;
        let r1 = db.execute(
            "UPDATE rules SET priority=?1 WHERE id=?2",
            params![b_pri, a_id],
        );
        let r2 = db.execute(
            "UPDATE rules SET priority=?1 WHERE id=?2",
            params![a_pri, b_id],
        );
        if r1.is_err() || r2.is_err() {
            let _ = db.execute_batch("ROLLBACK TO move_rule");
            let _ = db.execute_batch("RELEASE move_rule");
            return Err(r1.and(r2).unwrap_err());
        }
        db.execute_batch("RELEASE move_rule")?;
        self.rules[idx].priority = b_pri;
        self.rules[t].priority = a_pri;
        self.rules.sort_by_key(|r| (r.priority, r.id));
        Ok(())
    }
}
