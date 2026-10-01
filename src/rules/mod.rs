//! 规则引擎:连接匹配规则(进程/远端/端口/方向/协议/动作)的模型、
//! 求值与 SQLite 持久化;规则页 UI 见 ui_rules。
//!
//! 求值按 priority 升序(数值小者优先)取首个命中的启用规则;未命中
//! 任何规则时默认放行。表快照无方向语义,方向按远端端口近似判定,
//! ETW 事件源落地后以真实方向替换。

pub mod io;
pub mod wfp;

use std::collections::HashMap;
use std::net::Ipv4Addr;

use rusqlite::Connection as Db;
use rusqlite::params;

use crate::model::{Connection, Protocol};
use crate::storage::history;

/// Windows 默认动态端口范围下界:远端端口位于临时端口区间时,
/// 对端更可能是主动连入的客户端
const EPHEMERAL_MIN: u16 = 49152;

/// 静默拒绝兜底规则的保留 id(负数段之外,规则页不显示、不可编辑删除;
/// 该规则只存在于内存,由 app 层按配置同步)
pub const SILENT_FALLBACK_ID: i64 = i64::MIN;

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
    /// 全部条件均满足才命中;非法网段值永不命中(UI 侧已拦截)
    pub fn matches(&self, req: &MatchReq) -> bool {
        if self.direction != Direction::Any && self.direction != req.direction {
            return false;
        }
        if let Some(p) = self.proto
            && p != req.proto
        {
            return false;
        }
        if self.port != 0 && self.port != req.remote_port {
            return false;
        }
        if self.local_port != 0 && self.local_port != req.local_port {
            return false;
        }
        if !self.process.is_empty() && !match_process(&self.process, req.process, req.proc_path) {
            return false;
        }
        match self.remote_kind {
            RemoteKind::Any => {}
            RemoteKind::Ip => {
                let Some((lo, hi)) = parse_net(&self.remote_value) else {
                    return false;
                };
                if req.remote_ip < lo || req.remote_ip > hi {
                    return false;
                }
            }
            RemoteKind::Domain => match req.domain {
                Some(d) if match_domain(&self.remote_value, d) => {}
                _ => return false,
            },
        }
        true
    }

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

/// 求值输入:从连接与 rDNS 域名构造
pub struct MatchReq<'a> {
    pub process: &'a str,
    pub proc_path: Option<&'a str>,
    pub proto: Protocol,
    pub remote_ip: u32,
    pub remote_port: u16,
    /// 本地端口(临时规则的连接级精确匹配用)
    pub local_port: u16,
    /// rDNS 域名(未解析/无 PTR 为 None)
    pub domain: Option<&'a str>,
    pub direction: Direction,
}

impl<'a> MatchReq<'a> {
    pub fn from_conn(conn: &'a Connection, domain: Option<&'a str>) -> Self {
        MatchReq {
            process: &conn.process,
            proc_path: conn.proc_path.as_deref(),
            proto: conn.proto,
            remote_ip: u32::from(conn.remote_ip),
            remote_port: conn.remote_port,
            local_port: conn.local_port,
            domain,
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

/// 进程匹配:完整路径结尾(前带分隔符,避免误匹配同级前缀名)或映像名精确相等
fn match_process(value: &str, process: &str, proc_path: Option<&str>) -> bool {
    let v = value.trim().to_lowercase();
    if let Some(path) = proc_path {
        let p = path.to_lowercase();
        if p == v || p.ends_with(&format!("\\{v}")) {
            return true;
        }
    }
    process.to_lowercase() == v
}

/// 域名匹配:精确相等或子域名后缀(.value)
fn match_domain(value: &str, host: &str) -> bool {
    let v = value.trim().to_lowercase();
    let h = host.to_lowercase();
    h == v || h.ends_with(&format!(".{v}"))
}

/// 内存规则集(priority 升序、同值按 id),变更同步落库
pub struct RuleSet {
    pub rules: Vec<Rule>,
    /// 静默拒绝兜底(全通配 Block):仅内存,不入 rules 列表(规则页不显示),
    /// evaluate 在用户规则未命中时返回它——连接标注/地图阻断状态自动联动
    fallback: Option<Rule>,
    /// 进程规则的路径粘滞缓存(规则 id -> 已命中过的完整路径):
    /// 连接被阻断后快照可能抓不到进程行,已展开路径保持,避免
    /// 拦截窗口抖动;规则删除/改进程条件时清理
    sticky_paths: HashMap<i64, std::collections::BTreeSet<String>>,
    /// 会话内临时规则(询问"仅本次")的下一个负数 id
    next_temp_id: i64,
}

impl RuleSet {
    pub fn load(db: &Db) -> RuleSet {
        let mut rules = Vec::new();
        let result = (|| -> rusqlite::Result<()> {
            let mut stmt = db.prepare(
                "SELECT id, name, enabled, priority, action, direction, proto, process,
                        remote_kind, remote_value, port
                 FROM rules ORDER BY priority ASC, id ASC",
            )?;
            let rows = stmt.query_map([], |row| {
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
        RuleSet {
            rules,
            fallback: None,
            sticky_paths: HashMap::new(),
            next_temp_id: -1,
        }
    }

    /// 同步静默拒绝兜底(全通配 Block,deny = 开):app 层按配置每帧调用,
    /// 状态不变时零开销
    pub fn set_fallback(&mut self, deny: bool) {
        let want = deny.then(|| Rule {
            id: SILENT_FALLBACK_ID,
            name: "silent-deny".to_owned(),
            enabled: true,
            priority: i64::MAX,
            action: Action::Block,
            direction: Direction::Any,
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
            .find(|r| r.enabled && r.matches(req))
            .or(self.fallback.as_ref())
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
                && match_process(&r.process, process, proc_path)
        })
    }

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

    /// 新建规则并落库,追加为最低优先级
    pub fn insert(&mut self, db: &Db, mut rule: Rule) -> rusqlite::Result<()> {
        rule.priority = self.rules.last().map_or(10, |r| r.priority + 10);
        db.execute(
            "INSERT INTO rules (name, enabled, priority, action, direction, proto,
                                process, remote_kind, remote_value, port, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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

/// WFP 的 TCP/UDP 协议号(IPPROTO)
const PROTO_TCP: u8 = 6;
const PROTO_UDP: u8 = 17;
/// 过滤器 weight 上限(FWP_UINT8 有效范围 0..=15)
const MAX_WEIGHT: usize = 15;

/// 预留最高 weight:询问 pending 阻断与静默自身放行(二者互斥,静默拒绝
/// 模式下询问关闭,pending 不存在)
pub const WEIGHT_RESERVED_HIGH: u8 = MAX_WEIGHT as u8;
/// 静默兜底阻断 weight(低于全部用户规则,与子层基线同级)
pub const WEIGHT_FALLBACK: u8 = 0;

impl RuleSet {
    /// 把启用规则翻译为 WFP 过滤器目标集合(语义见 wfp::Spec;域名规则
    /// 不参与翻译)。与求值引擎一致:优先级高者 weight 大。进程规则按
    /// 映像名展开为命中过的完整路径集合(粘滞缓存,见字段注释)
    pub fn wfp_specs(&mut self, conns: &[Connection]) -> Vec<crate::rules::wfp::Spec> {
        let mut applicable: Vec<&Rule> = self
            .rules
            .iter()
            .filter(|r| r.enabled && r.remote_kind != RemoteKind::Domain)
            .collect();
        applicable.sort_by_key(|r| (r.priority, r.id));

        let mut specs = Vec::new();
        for (rank, r) in applicable.iter().enumerate() {
            // weight 布局:15 = 询问 pending / 静默自身放行,1..=14 = 用户规则,
            // 0 = 静默兜底阻断(与子层基线同级);用户规则超过 14 条后钳制到 1,
            // 恒高于兜底,避免同 weight 时 WFP 动作未定义
            let weight = (MAX_WEIGHT - 1 - rank.min(MAX_WEIGHT - 2)) as u8;
            let remote = match r.remote_kind {
                RemoteKind::Any => None,
                RemoteKind::Ip => match parse_net(&r.remote_value) {
                    Some(v) => Some(v),
                    None => continue,
                },
                RemoteKind::Domain => unreachable!("filtered above"),
            };
            let paths: Vec<Option<String>> = if r.process.is_empty() {
                self.sticky_paths.remove(&r.id);
                vec![None]
            } else {
                {
                    let needle = format!("\\{}", r.process.trim().to_lowercase());
                    let exact = r.process.trim().to_lowercase();
                    let known = self.sticky_paths.entry(r.id).or_default();
                    for p in conns.iter().filter_map(|c| c.proc_path.as_ref()) {
                        let lp = p.to_lowercase();
                        if lp.ends_with(&needle) || lp == exact {
                            known.insert(p.clone());
                        }
                    }
                }
                self.sticky_paths
                    .get(&r.id)
                    .map(|s| s.iter().cloned().map(Some).collect())
                    .unwrap_or_default()
            };
            let proto = r.proto.map(|p| match p {
                Protocol::Tcp => PROTO_TCP,
                Protocol::Udp => PROTO_UDP,
            });
            let port = (r.port != 0).then_some(r.port);
            let layers: &[crate::rules::wfp::Layer] = match r.direction {
                Direction::Any => &[crate::rules::wfp::Layer::Out, crate::rules::wfp::Layer::In],
                Direction::Out => &[crate::rules::wfp::Layer::Out],
                Direction::In => &[crate::rules::wfp::Layer::In],
            };
            for path in &paths {
                for &layer in layers {
                    specs.push(crate::rules::wfp::Spec {
                        layer,
                        weight,
                        block: r.action == Action::Block,
                        app_path: path.clone(),
                        remote,
                        proto,
                        port,
                    });
                }
            }
        }
        specs
    }
}

fn parse_action(s: &str) -> Action {
    if s == Action::Allow.as_str() {
        Action::Allow
    } else {
        Action::Block
    }
}

fn parse_direction(s: &str) -> Direction {
    match s {
        "out" => Direction::Out,
        "in" => Direction::In,
        _ => Direction::Any,
    }
}

fn parse_remote_kind(s: &str) -> RemoteKind {
    match s {
        "ip" => RemoteKind::Ip,
        "domain" => RemoteKind::Domain,
        _ => RemoteKind::Any,
    }
}

/// 空串 = 任意协议
fn parse_proto(s: &str) -> Option<Protocol> {
    if s == Protocol::Udp.as_str() {
        Some(Protocol::Udp)
    } else if s == Protocol::Tcp.as_str() {
        Some(Protocol::Tcp)
    } else {
        None
    }
}
