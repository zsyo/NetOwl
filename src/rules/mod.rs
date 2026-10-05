//! 规则引擎:连接匹配规则(进程/远端/端口/方向/协议/动作)的模型、
//! 求值与 SQLite 持久化;规则页 UI 见 ui_rules。
//!
//! 求值按 priority 升序(数值小者优先)取首个命中的启用规则;未命中
//! 任何规则时默认放行。表快照无方向语义,方向按远端端口近似判定,
//! ETW 事件源落地后以真实方向替换。
//!
//! RuleSet 单点定义在本模块,impl 块按功能域分散:profile(配置档管理
//! 与行加载)、eval(求值与静默兜底)、store(CRUD 与临时规则)、
//! specs(WFP 过滤器翻译)。

pub mod io;
pub mod wfp;

mod eval;
mod profile;
mod specs;
mod store;

use std::collections::HashMap;
use std::net::Ipv4Addr;

use crate::model::{Connection, Protocol};

pub use profile::Profile;

/// Windows 默认动态端口范围下界:远端端口位于临时端口区间时,
/// 对端更可能是主动连入的客户端
const EPHEMERAL_MIN: u16 = 49152;

/// 静默拒绝兜底规则的保留 id(负数段之外,规则页不显示、不可编辑删除;
/// 该规则只存在于内存,由 app 层按配置同步)
pub const SILENT_FALLBACK_ID: i64 = i64::MIN;

pub use specs::{WEIGHT_FALLBACK, WEIGHT_RESERVED_HIGH};

/// 规则动作
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Allow,
    Block,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Allow => "allow",
            Action::Block => "block",
        }
    }
}

/// 匹配方向
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    Any,
    Out,
    In,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Any => "any",
            Direction::Out => "out",
            Direction::In => "in",
        }
    }
}

/// 远端匹配类型
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RemoteKind {
    Any,
    Ip,
    Domain,
}

impl RemoteKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RemoteKind::Any => "any",
            RemoteKind::Ip => "ip",
            RemoteKind::Domain => "domain",
        }
    }
}

/// 一条匹配规则
#[derive(Clone, Debug)]
pub struct Rule {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    /// 数值小者优先评估;新建规则追加为最低优先级
    pub priority: i64,
    pub action: Action,
    pub direction: Direction,
    /// None = 任意协议
    pub proto: Option<Protocol>,
    /// 进程映像名或完整路径结尾(空 = 任意;大小写不敏感)
    pub process: String,
    pub remote_kind: RemoteKind,
    /// 网段(CIDR/前缀/单 IP)或域名(精确或子域名后缀)
    pub remote_value: String,
    /// 远端端口(0 = 任意)
    pub port: u16,
    /// 本地端口(0 = 任意;仅弹窗"仅本次"临时规则使用,精确锁定
    /// 单条连接,持久规则恒为 0)
    pub local_port: u16,
}

impl Rule {
    /// 构造持久阻断规则(地图面板一键阻断):`remote` 传 None 阻断整个
    /// 进程,传目标 IP 只阻断该进程到此远端(不限端口);方向/协议任意,
    /// process 填映像名(规则语义:映像名或路径结尾)
    pub fn block(process: &str, remote: Option<Ipv4Addr>) -> Rule {
        Rule {
            id: 0,
            name: match remote {
                Some(ip) => format!("{process} -> {ip}"),
                None => process.to_owned(),
            },
            enabled: true,
            priority: 0,
            action: Action::Block,
            direction: Direction::Any,
            proto: None,
            process: process.to_owned(),
            remote_kind: remote.map_or(RemoteKind::Any, |_| RemoteKind::Ip),
            remote_value: remote.map_or(String::new(), |ip| ip.to_string()),
            port: 0,
            local_port: 0,
        }
    }

    /// 构造持久放行规则(静默拒绝兜底的撤销 = 为目标建允许规则覆盖):
    /// 条件构造与 [`Rule::block`] 完全一致,仅动作相反
    pub fn permit(process: &str, remote: Option<Ipv4Addr>) -> Rule {
        let mut rule = Rule::block(process, remote);
        rule.action = Action::Allow;
        rule
    }
}

/// 求值输入:从连接与 rDNS 域名构造。进程/路径/域名的比较基准在构造时
/// 小写化一次(每连接一次,供全部规则复用),求值路径零分配
pub struct MatchReq {
    /// 进程映像名小写
    pub process_lower: String,
    /// 完整路径小写
    pub proc_path_lower: Option<String>,
    /// rDNS 域名小写(未解析/无 PTR 为 None)
    pub domain_lower: Option<String>,
    pub proto: Protocol,
    pub remote_ip: u32,
    pub remote_port: u16,
    /// 本地端口(临时规则的连接级精确匹配用)
    pub local_port: u16,
    pub direction: Direction,
}

impl MatchReq {
    pub fn from_conn(conn: &Connection, domain: Option<&str>) -> Self {
        MatchReq {
            process_lower: conn.process.to_lowercase(),
            proc_path_lower: conn.proc_path.as_deref().map(str::to_lowercase),
            domain_lower: domain.map(str::to_lowercase),
            proto: conn.proto,
            remote_ip: u32::from(conn.remote_ip),
            remote_port: conn.remote_port,
            local_port: conn.local_port,
            direction: conn_direction(conn),
        }
    }
}

/// 表快照无方向语义,按远端端口近似:远端端口在临时端口范围视为对端
/// 主动连入(入站),否则视为本机出站;UDP 表行无远端语义统一按出站
pub fn conn_direction(conn: &Connection) -> Direction {
    if conn.proto == Protocol::Udp || conn.remote_port < EPHEMERAL_MIN {
        Direction::Out
    } else {
        Direction::In
    }
}

/// 网段解析:CIDR "10.0.0.0/8"、前缀 "142.250."、单 IP "1.2.3.4";
/// 返回区间 [min, max],前缀风格与历史页远端筛选一致
pub fn parse_net(input: &str) -> Option<(u32, u32)> {
    let s = input.trim();
    if let Some((addr, mask)) = s.split_once('/') {
        let ip: Ipv4Addr = addr.trim().parse().ok()?;
        let prefix: u32 = mask.trim().parse().ok()?;
        if prefix > 32 {
            return None;
        }
        let v = u32::from(ip);
        let lo = if prefix == 0 {
            0
        } else {
            v & (u32::MAX << (32 - prefix))
        };
        let hi = if prefix == 0 {
            u32::MAX
        } else {
            v | (u32::MAX >> prefix)
        };
        Some((lo, hi))
    } else {
        crate::storage::history_query::parse_ip_prefix(s)
    }
}

/// 进程条件命中:规则值与连接侧小写基准比较——完整路径结尾(前带分隔符,
/// 避免误匹配同级前缀名)或映像名精确相等
pub(super) fn process_hit(v: &str, name_lower: &str, path_lower: Option<&str>) -> bool {
    if let Some(p) = path_lower
        && (p == v || ends_with_path(p, v))
    {
        return true;
    }
    name_lower == v
}

/// 路径结尾匹配:p 以分隔符 + v 结尾(字节判定,零分配)
pub(super) fn ends_with_path(p: &str, v: &str) -> bool {
    p.len() > v.len() && p.ends_with(v) && p.as_bytes()[p.len() - v.len() - 1] == b'\\'
}

/// 子域名匹配:精确相等或以 .v 结尾(字节判定,零分配)
pub(super) fn is_subdomain(h: &str, v: &str) -> bool {
    h == v || (h.len() > v.len() && h.ends_with(v) && h.as_bytes()[h.len() - v.len() - 1] == b'.')
}

/// 规则匹配条件的预计算派生数据(经 RuleSet::eval_cache 随规则增删改
/// 同步刷新,求值路径零分配)
#[derive(Clone, Debug)]
struct RuleMatch {
    process_lower: String,
    remote_lower: String,
    /// RemoteKind::Ip 的网段区间;None = 非法值永不命中
    remote_range: Option<(u32, u32)>,
}

impl RuleMatch {
    fn of(r: &Rule) -> Self {
        RuleMatch {
            process_lower: r.process.trim().to_lowercase(),
            remote_lower: r.remote_value.trim().to_lowercase(),
            remote_range: (r.remote_kind == RemoteKind::Ip)
                .then(|| parse_net(&r.remote_value))
                .flatten(),
        }
    }
}

/// 内存规则集(priority 升序、同值按 id),变更同步落库。
/// 规则按配置档(profile_id)隔离:load/insert 作用于当前档,
/// 切换档经 [`RuleSet::switch_profile`] 重载
pub struct RuleSet {
    pub rules: Vec<Rule>,
    /// 当前配置档 id(新增规则归属;切换经 switch_profile)
    pub active_profile: i64,
    /// 静默拒绝兜底(全通配 Block):仅内存,不入 rules 列表(规则页不显示),
    /// evaluate 在用户规则未命中时返回它——连接标注/地图阻断状态自动联动
    fallback: Option<Rule>,
    /// 规则求值预计算(id -> 小写化条件与网段区间)
    eval_cache: HashMap<i64, RuleMatch>,
    /// 进程规则的路径粘滞缓存(规则 id -> 已命中过的完整路径):
    /// 连接被阻断后快照可能抓不到进程行,已展开路径保持,避免
    /// 拦截窗口抖动;规则删除/改进程条件时清理
    sticky_paths: HashMap<i64, std::collections::BTreeSet<String>>,
    /// 会话内临时规则(询问"仅本次")的下一个负数 id
    next_temp_id: i64,
}
