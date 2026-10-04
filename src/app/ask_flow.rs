//! 新连接询问编排:入队检测、倒计时超时默认拒绝、决策落地(永久落库 /
//! 仅本次临时规则)与临时规则生命周期管理。引擎状态机见 super::ask。

use super::NetOwlApp;
use super::ask::{Decision, Scope, temp_rule_holds};
use crate::collector::CollectorKind;
use crate::rules;

impl NetOwlApp {
    /// 新连接询问:静默模式关闭(ask_connections 开启)时检测未命中规则
    /// 的公网连接并入队;倒计时超时执行默认动作(拒绝·仅本次);
    /// 静默放行/拒绝模式下与询问互斥,队列与弹窗一并清空
    pub(super) fn poll_ask(&mut self) {
        if self.collector.kind() != CollectorKind::Real
            || !self.config.general.ask_connections
            || matches!(self.config.general.silent_mode.as_str(), "allow" | "deny")
        {
            self.asker.clear();
            return;
        }
        self.asker.update(&self.conns, &self.rules, &self.rdns);
        self.asker.poll();
        if self.asker.expired() {
            self.apply_decision(Decision {
                allow: false,
                scope: Scope::Once,
            });
        }
    }

    /// 应用询问决策:永久选项落库;拒绝·仅本次写入内存临时规则;
    /// 允许·仅本次不产生规则(询问的连接未命中任何规则,默认即放行);
    /// 仅本次决策后解除身份去重标记,同目标新连接重新询问
    pub(super) fn apply_decision(&mut self, d: Decision) {
        let Some(item) = self.asker.take() else {
            return;
        };
        tracing::info!(
            "[Ask] 决策:{}({:?}) {}({}) -> {}",
            if d.allow { "允许" } else { "拒绝" },
            d.scope,
            item.process,
            item.pid,
            item.remote_display()
        );
        if d.scope == Scope::Once {
            self.asker.unask(&item);
            if d.allow {
                return;
            }
        }
        let action = if d.allow {
            rules::Action::Allow
        } else {
            rules::Action::Block
        };
        let rule = item.to_rule(action);
        match d.scope {
            Scope::Once => self.rules.insert_temp(rule),
            Scope::Target | Scope::Process => {
                if let Err(e) = self.rules.insert(&self.history_db, rule) {
                    tracing::warn!("[Ask] 决策规则落库失败({}): {e}", item.process);
                }
            }
        }
    }

    /// 临时规则生命周期:仅绑定决策时的那条连接(含本地端口的四元组
    /// 精确匹配),快照中不再有匹配连接时删除;同目标重连不会撞上旧
    /// 决策,而是重新弹窗询问
    pub(super) fn poll_temp_rules(&mut self) {
        let expired: Vec<i64> = self
            .rules
            .rules
            .iter()
            .filter(|r| r.id < 0)
            .filter(|r| !self.conns.iter().any(|c| temp_rule_holds(r, c)))
            .map(|r| r.id)
            .collect();
        if expired.is_empty() {
            return;
        }
        tracing::debug!("[Ask] 清理 {} 条随连接消失的临时规则", expired.len());
        for id in expired {
            self.rules.delete_temp(id);
        }
    }
}
