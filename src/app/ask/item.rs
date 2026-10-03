//! 询问条目:待询问连接的身份快照、弹窗展示文本、等待期临时阻断
//! 过滤器与决策结果转规则;含可询问目标与身份键判定。

use std::hash::{Hash, Hasher};
use std::net::Ipv4Addr;
use std::time::Instant;

use crate::model::Protocol;
use crate::rules::{Action, Direction, RemoteKind, Rule};

/// 询问等待期间的临时阻断 weight(最高,压过用户规则;与静默自身放行
/// 互斥——静默拒绝模式下询问关闭,pending spec 不存在)
const PENDING_WEIGHT: u8 = crate::rules::WEIGHT_RESERVED_HIGH;
/// 系统进程(PID 4)持有内核级 socket,不询问
pub(super) const SYSTEM_PID: u32 = 4;

/// 决策的作用范围
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// 仅本次:拒绝时为会话内临时精确规则(进程+目标+端口+协议+本地
    /// 端口,锁定当前连接,连接结束即清理);允许时直接放行不产生规则;
    /// 两者决策后解除身份去重,同目标新连接重新询问
    Once,
    /// 永久·仅此目标:进程 + 目标 IP(任意端口)
    Target,
    /// 永久·整个程序:仅进程(任意远端)
    Process,
}

/// 弹窗决策 = 动作 + 范围
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub allow: bool,
    pub scope: Scope,
}

/// 一次待询问的连接(进程 + 远端身份快照)
#[derive(Clone, Debug)]
pub struct AskItem {
    /// 进程映像完整路径(未知进程为 None)
    pub proc_path: Option<String>,
    pub process: String,
    pub pid: u32,
    pub local_port: u16,
    pub remote_ip: Ipv4Addr,
    pub remote_port: u16,
    pub proto: Protocol,
    /// 首见时的 rDNS 域名(未解析为 None)
    pub domain: Option<String>,
    pub deadline: Instant,
    /// 弹窗内当前选择的生效范围(跨帧持久;决策时读取)
    pub scope: Scope,
}

impl AskItem {
    /// 弹窗展示的主文本远端(域名优先)
    pub fn remote_display(&self) -> String {
        match &self.domain {
            Some(d) => format!("{}:{}", d, self.remote_port),
            None => format!("{}:{}", self.remote_ip, self.remote_port),
        }
    }

    /// 询问等待期间的临时阻断过滤器(精确锁定该连接身份)
    pub fn pending_block_spec(&self) -> crate::rules::wfp::Spec {
        let ip = u32::from(self.remote_ip);
        crate::rules::wfp::Spec {
            layer: crate::rules::wfp::Layer::Out,
            weight: PENDING_WEIGHT,
            block: true,
            app_path: self.proc_path.clone(),
            remote: Some((ip, ip)),
            proto: Some(proto_num(self.proto)),
            port: Some(self.remote_port),
        }
    }

    /// 决策结果转规则;process 填映像名(规则语义:映像名或路径结尾)。
    /// 范围决定持久规则粒度与名称:Target = 进程+目标IP(不限端口,
    /// 名称只带目标主机),Process = 仅进程(名称即程序名);
    /// Once 精确锁定当前连接四元组(含本地端口,连接结束即清理)
    pub fn to_rule(&self, action: Action) -> Rule {
        let (remote_kind, remote_value, port, local_port) = match self.scope {
            Scope::Once => (
                RemoteKind::Ip,
                self.remote_ip.to_string(),
                self.remote_port,
                self.local_port,
            ),
            Scope::Target => (RemoteKind::Ip, self.remote_ip.to_string(), 0, 0),
            Scope::Process => (RemoteKind::Any, String::new(), 0, 0),
        };
        let name = match self.scope {
            Scope::Once => format!("{} -> {}", self.process, self.remote_display()),
            Scope::Target => format!("{} -> {}", self.process, self.remote_host()),
            Scope::Process => self.process.clone(),
        };
        Rule {
            id: 0,
            name,
            enabled: true,
            priority: 0,
            action,
            direction: Direction::Any,
            proto: Some(self.proto),
            process: self.process.clone(),
            remote_kind,
            remote_value,
            port,
            local_port,
        }
    }

    /// 远端主机显示名(域名优先,不带端口)
    fn remote_host(&self) -> String {
        match &self.domain {
            Some(d) => d.clone(),
            None => self.remote_ip.to_string(),
        }
    }
}

fn proto_num(p: Protocol) -> u8 {
    match p {
        Protocol::Tcp => 6,
        Protocol::Udp => 17,
    }
}

/// 可询问的目标:仅公网段(回环/私网/链路本地/组播/保留段静默放行,
/// 与 rDNS 的可查询口径一致)
pub(super) fn is_askable(ip: &Ipv4Addr) -> bool {
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.octets()[0] >= 240)
}

/// 连接身份:(进程路径/名, 目标IP)。端口/协议不参与去重,
/// 同一目标多端口只询问一次
pub(super) fn identity_key(proc_path: Option<&str>, process: &str, ip: Ipv4Addr) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    proc_path.unwrap_or(process).hash(&mut h);
    ip.hash(&mut h);
    h.finish()
}

/// 临时规则是否仍绑定着活跃连接:决策时连接的四元组 + 进程精确匹配
/// (连接结束/超时消失后规则即视为过期)
pub fn temp_rule_holds(rule: &Rule, c: &crate::model::Connection) -> bool {
    rule.local_port != 0
        && rule.proto == Some(c.proto)
        && rule.remote_kind == RemoteKind::Ip
        && rule.remote_value == c.remote_ip.to_string()
        && rule.port == c.remote_port
        && rule.local_port == c.local_port
        && rule.process == c.process
}
