//! 规则页显示文案:方向/协议/动作/远端类型的词条映射与 "*" 占位显示。

use crate::i18n::I18n;
use crate::model::Protocol;
use crate::rules::{Action, Direction, RemoteKind, Rule};

pub(super) fn direction_name(i18n: &I18n, d: Direction) -> String {
    match d {
        Direction::Any => i18n.t("rule-direction-any"),
        Direction::Out => i18n.t("rule-direction-out"),
        Direction::In => i18n.t("rule-direction-in"),
    }
}

pub(super) fn proto_name(i18n: &I18n, p: Option<Protocol>) -> String {
    match p {
        None => i18n.t("rule-proto-any"),
        Some(v) => v.as_str().to_owned(),
    }
}

pub(super) fn action_text(i18n: &I18n, a: Action) -> String {
    match a {
        Action::Allow => i18n.t("rule-action-allow"),
        Action::Block => i18n.t("rule-action-block"),
    }
}

pub(super) fn remote_kind_name(i18n: &I18n, k: RemoteKind) -> String {
    match k {
        RemoteKind::Any => i18n.t("rule-remote-any"),
        RemoteKind::Ip => i18n.t("rule-remote-ip"),
        RemoteKind::Domain => i18n.t("rule-remote-domain"),
    }
}

/// "*" 表示该维度不限定
pub(super) fn process_display(rule: &Rule) -> String {
    if rule.process.is_empty() {
        "*".to_owned()
    } else {
        rule.process.clone()
    }
}

pub(super) fn remote_display(rule: &Rule) -> String {
    if rule.remote_kind == RemoteKind::Any {
        "*".to_owned()
    } else {
        rule.remote_value.clone()
    }
}

pub(super) fn port_display(port: u16) -> String {
    if port == 0 {
        "-".to_owned()
    } else {
        port.to_string()
    }
}
