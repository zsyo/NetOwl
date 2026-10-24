//! 求值与静默兜底:优先级首个命中、未命中回落全通配兜底规则(静默
//! 拒绝模式),阻断状态查询供地图面板标注联动。比较基准取 eval_cache
//! 预计算(见 RuleMatch),求值路径零分配。

use std::collections::HashMap;
use std::net::IpAddr;

use rusqlite::params;

use super::{Action, Direction, MatchReq, NetRange, RemoteKind, Rule, RuleSet, SILENT_FALLBACK_ID};
use crate::model::Connection;

/// 大小写不敏感等值(流式 Unicode lowercase,零分配):与
/// str::to_lowercase 同一算法的逐字符版本;非 ASCII(如土耳其 İ)
/// 小写后长度可变,等长 ASCII 快路径失败后必须走流式兜底
fn eq_ignore_case(a: &str, b: &str) -> bool {
    if a.len() == b.len() && a.eq_ignore_ascii_case(b) {
        return true;
    }
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

/// 单字符忽略大小写等值
fn char_eq_ci(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// 尾部匹配(h 以分隔符 + v 结尾,忽略大小写,流式零分配):反向逐
/// 字符比较,对齐耗尽后 h 多出的前一字符须为分隔符(等长或 h 更短
/// 均不算)
fn ends_with_sep_ci(h: &str, v: &str, sep: char) -> bool {
    let mut hc = h.chars().rev();
    let mut vc = v.chars().rev();
    loop {
        match (vc.next(), hc.next()) {
            (None, Some(c)) => return c == sep,
            (None, None) => return false,
            (Some(_), None) => return false,
            (Some(a), Some(b)) => {
                if !char_eq_ci(a, b) {
                    return false;
                }
            }
        }
    }
}

/// 进程条件命中(忽略大小写,零分配):规则侧基准已预小写(eval_cache),
/// 连接侧直接用原始值流式比较;路径完整相等或以分隔符 + 映像名结尾均算
pub(super) fn process_hit(v: &str, name: &str, path: Option<&str>) -> bool {
    if let Some(p) = path
        && (eq_ignore_case(p, v) || ends_with_sep_ci(p, v, '\\'))
    {
        return true;
    }
    eq_ignore_case(name, v)
}

/// 子域名匹配(忽略大小写,流式零分配):精确相等或以 .v 结尾
fn is_subdomain(h: &str, v: &str) -> bool {
    eq_ignore_case(h, v) || ends_with_sep_ci(h, v, '.')
}

impl RuleSet {
    /// 同步静默拒绝兜底(全通配 Block,deny = 开):app 层按配置每帧调用,
    /// 状态不变时零开销
    pub fn set_fallback(&mut self, deny: bool) {
        let want = deny.then(|| Rule {
            id: SILENT_FALLBACK_ID,
            name: "silent-deny".to_owned(),
            enabled: true,
            priority: i64::MAX,
            action: Action::Block,
            direction: super::Direction::Any,
            proto: None,
            process: String::new(),
            remote_kind: RemoteKind::Any,
            remote_value: String::new(),
            port: 0,
            local_port: 0,
            hit_count: 0,
        });
        if want.as_ref().map(|r| r.id) != self.fallback.as_ref().map(|r| r.id) {
            self.fallback = want;
        }
    }

    /// 求值:按优先级首个命中的启用规则;无命中时返回静默拒绝兜底(若开启);
    /// 仍无则 None(默认放行)
    pub fn evaluate(&self, req: &MatchReq<'_>) -> Option<&Rule> {
        self.rules
            .iter()
            .find(|r| r.enabled && self.rule_matches(r, req))
            .or(self.fallback.as_ref())
    }

    /// 命中计数累计:对快照内每条连接求值,每连接首次命中某持久规则
    /// (或命中规则发生变化)计一次——长连接存活期间每秒命中不重复计,
    /// 口径为"累计命中连接数",落库跨重启保留。由 app 层按 1s 采集
    /// 节流调用(非渲染帧——连接页每帧渲染也会 evaluate,按帧计数会随
    /// 重绘节奏虚高)。静默兜底(负基准 id)与会话临时规则(负 id)不计:
    /// 前者非用户规则,后者是"仅本次"的会话决策,计数无调优意义
    pub fn note_hits(&mut self, conns: &[Connection], rdns: &crate::net::rdns::Rdns) {
        // 去重表每轮按快照重建:断连的连接自然淘汰,内存有界
        let mut counted = HashMap::with_capacity(conns.len());
        for c in conns {
            let req = MatchReq::from_conn(c, rdns.lookup(c.remote_ip));
            let hit_id = match self.evaluate(&req) {
                Some(r) if r.id > 0 => r.id,
                _ => continue,
            };
            counted.insert(c.id, hit_id);
            if self.hit_counted.get(&c.id) != Some(&hit_id)
                && let Some(rule) = self.rules.iter_mut().find(|r| r.id == hit_id)
            {
                rule.hit_count += 1;
                self.hit_dirty = true;
            }
        }
        self.hit_counted = counted;
    }

    /// 命中计数落盘(30s 批量 + 退出时):仅当前档的持久规则,无变更
    /// 零写入;任一条失败即整体返回 Err,dirty 保留待下轮重试。
    /// 整批一个 SAVEPOINT:逐条 execute 在 WAL 下每条一次隐式提交,
    /// 会与历史写线程争 WAL 写锁(见 move_rule 同款写法)
    pub fn flush_hits(&mut self, db: &crate::storage::history::Db) -> rusqlite::Result<()> {
        if !self.hit_dirty {
            return Ok(());
        }
        db.execute_batch("SAVEPOINT flush_hits")?;
        for r in &self.rules {
            if r.id <= 0 {
                continue;
            }
            let res = db.execute(
                "UPDATE rules SET hit_count = ?1 WHERE id = ?2",
                params![r.hit_count as i64, r.id],
            );
            if let Err(e) = res {
                let _ = db.execute_batch("ROLLBACK TO flush_hits");
                let _ = db.execute_batch("RELEASE flush_hits");
                return Err(e);
            }
        }
        db.execute_batch("RELEASE flush_hits")?;
        self.hit_dirty = false;
        Ok(())
    }

    /// 规则命中判定(全部条件均满足才命中;非法网段值永不命中):
    /// 规则侧比较基准取预计算缓存,连接侧借用原始值流式比较,零分配
    pub(super) fn rule_matches(&self, r: &Rule, req: &MatchReq<'_>) -> bool {
        if r.direction != Direction::Any && r.direction != req.direction {
            return false;
        }
        if let Some(p) = r.proto
            && p != req.proto
        {
            return false;
        }
        if r.port != 0 && r.port != req.remote_port {
            return false;
        }
        if r.local_port != 0 && r.local_port != req.local_port {
            return false;
        }
        let Some(m) = self.eval_cache.get(&r.id) else {
            return false;
        };
        if !m.process_lower.is_empty() && !process_hit(&m.process_lower, req.process, req.proc_path)
        {
            return false;
        }
        match r.remote_kind {
            RemoteKind::Any => {}
            // v4 闭区间数值比较;v6 按前缀掩码等值(网段语义)
            RemoteKind::Ip => match (m.remote_range, req.remote_ip) {
                (Some(NetRange::V4(lo, hi)), IpAddr::V4(v4))
                    if u32::from(v4) >= lo && u32::from(v4) <= hi => {}
                (Some(NetRange::V6 { addr, prefix }), IpAddr::V6(v6)) => {
                    let mask = NetRange::v6_mask(prefix);
                    if u128::from(v6) & mask != addr & mask {
                        return false;
                    }
                }
                _ => return false,
            },
            RemoteKind::Domain => match (req.domain, &m.remote_lower) {
                (Some(h), v) if is_subdomain(h, v) => {}
                _ => return false,
            },
        }
        true
    }

    /// 命中该连接的启用阻断规则(求值首个命中且动作为阻断);
    /// 地图面板的连接级阻断状态与撤销定位用
    pub fn blocking_rule(&self, conn: &Connection, domain: Option<&str>) -> Option<&Rule> {
        let req = MatchReq::from_conn(conn, domain);
        self.evaluate(&req).filter(|r| r.action == Action::Block)
    }

    /// 进程级(远端任意、不限端口)的启用阻断规则:进程条件命中该映像名
    /// 或已知完整路径即算;地图面板"阻断进程"的状态与撤销定位用
    pub fn process_block_rule(&self, process: &str, proc_path: Option<&str>) -> Option<&Rule> {
        self.rules.iter().find(|r| {
            r.enabled
                && r.action == Action::Block
                && r.remote_kind == RemoteKind::Any
                && r.port == 0
                && r.local_port == 0
                && !r.process.is_empty()
                && self
                    .eval_cache
                    .get(&r.id)
                    .is_some_and(|m| process_hit(&m.process_lower, process, proc_path))
        })
    }
}
