//! 编辑弹窗草稿:规则表单的可变状态载体,与 Rule 互转。

use crate::model::Protocol;
use crate::rules::{Action, Direction, RemoteKind, Rule};

/// 编辑弹窗草稿;id < 0 表示新建
pub struct Draft {
    pub id: i64,
    pub name: String,
    /// 编辑已有规则时保留原启停状态(启停在表格开关操作)
    enabled: bool,
    pub action: Action,
    pub direction: Direction,
    pub proto: Option<Protocol>,
    pub process: String,
    pub remote_kind: RemoteKind,
    pub remote_value: String,
    pub port: u16,
    /// 原规则命中累计(编辑时原样带回,保存不清零)
    hit_count: u64,
    /// 保存校验错误(词条键)
    pub error: Option<&'static str>,
}

impl Draft {
    pub(super) fn new_rule() -> Draft {
        Draft {
            id: -1,
            name: String::new(),
            enabled: true,
            action: Action::Block,
            direction: Direction::Any,
            proto: None,
            process: String::new(),
            remote_kind: RemoteKind::Any,
            remote_value: String::new(),
            port: 0,
            hit_count: 0,
            error: None,
        }
    }

    pub(super) fn from_rule(r: &Rule) -> Draft {
        Draft {
            id: r.id,
            name: r.name.clone(),
            enabled: r.enabled,
            action: r.action,
            direction: r.direction,
            proto: r.proto,
            process: r.process.clone(),
            remote_kind: r.remote_kind,
            remote_value: r.remote_value.clone(),
            port: r.port,
            // 命中计数随编辑保留:改规则条件不应把历史累计清零
            hit_count: r.hit_count,
            error: None,
        }
    }

    pub(super) fn to_rule(&self, priority: i64) -> Rule {
        Rule {
            id: self.id,
            name: self.name.trim().to_owned(),
            enabled: self.enabled,
            priority,
            action: self.action,
            direction: self.direction,
            proto: self.proto,
            process: self.process.trim().to_owned(),
            remote_kind: self.remote_kind,
            remote_value: self.remote_value.trim().to_owned(),
            port: self.port,
            local_port: 0,
            hit_count: self.hit_count,
        }
    }
}
