//! 求值与静默兜底:优先级首个命中、未命中回落全通配兜底规则(静默
//! 拒绝模式),阻断状态查询供地图面板标注联动。比较基准取 eval_cache
//! 预计算(见 RuleMatch),求值路径零分配。

use super::{
    Action, Direction, MatchReq, RemoteKind, Rule, RuleSet, SILENT_FALLBACK_ID, is_subdomain,
    process_hit,
};
use crate::model::Connection;

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
        });
        if want.as_ref().map(|r| r.id) != self.fallback.as_ref().map(|r| r.id) {
            self.fallback = want;
        }
    }

    /// 求值:按优先级首个命中的启用规则;无命中时返回静默拒绝兜底(若开启);
    /// 仍无则 None(默认放行)
    pub fn evaluate(&self, req: &MatchReq) -> Option<&Rule> {
        self.rules
            .iter()
            .find(|r| r.enabled && self.rule_matches(r, req))
            .or(self.fallback.as_ref())
    }

    /// 规则命中判定(全部条件均满足才命中;非法网段值永不命中):
    /// 比较基准取预计算缓存,零分配
    pub(super) fn rule_matches(&self, r: &Rule, req: &MatchReq) -> bool {
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
        if !m.process_lower.is_empty()
            && !process_hit(
                &m.process_lower,
                &req.process_lower,
                req.proc_path_lower.as_deref(),
            )
        {
            return false;
        }
        match r.remote_kind {
            RemoteKind::Any => {}
            RemoteKind::Ip => match m.remote_range {
                Some((lo, hi)) if req.remote_ip >= lo && req.remote_ip <= hi => {}
                _ => return false,
            },
            RemoteKind::Domain => match (&req.domain_lower, &m.remote_lower) {
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
        let name_lower = process.to_lowercase();
        let path_lower = proc_path.map(str::to_lowercase);
        self.rules.iter().find(|r| {
            r.enabled
                && r.action == Action::Block
                && r.remote_kind == RemoteKind::Any
                && r.port == 0
                && r.local_port == 0
                && !r.process.is_empty()
                && self.eval_cache.get(&r.id).is_some_and(|m| {
                    process_hit(&m.process_lower, &name_lower, path_lower.as_deref())
                })
        })
    }
}
