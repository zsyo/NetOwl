//! 新连接询问(Little Snitch 式):未命中任何规则的公网新连接弹窗询问
//! 允许/拒绝与生效范围,倒计时超时自动执行默认动作(拒绝·仅本次)。
//!
//! 触发粒度 = 进程 + 目标 IP(端口/协议不参与去重,同目标多端口只问一次);
//! 回环/局域网/保留段目标、系统进程、UDP 无远端行不询问(静默放行),
//! 待询问队列超上限时同样静默放行。询问等待期间该身份被临时阻断
//! (最高 weight 的 pending 过滤器,安全默认);决策结果转为临时规则
//! (仅本次,内存)或持久规则(永久,落库),WFP 拦截与列表标注随之生效。

use std::collections::{HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use crate::model::{Connection, Protocol};
use crate::rdns;
use crate::rules::{Action, Direction, MatchReq, RemoteKind, Rule, RuleSet};

/// 倒计时:超时自动执行默认动作(拒绝·仅本次)
pub const ASK_TIMEOUT: Duration = Duration::from_secs(30);
/// 待询问队列上限:超出后新身份静默放行,防瞬时连接风暴刷屏
const QUEUE_LIMIT: usize = 10;
/// 询问等待期间的临时阻断 weight(最高,压过用户规则)
const PENDING_WEIGHT: u8 = 15;
/// 系统进程(PID 4)持有内核级 socket,不询问
const SYSTEM_PID: u32 = 4;

/// 决策的作用范围
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// 仅本次:会话内临时精确规则(IP+端口+协议)
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
    pub fn pending_block_spec(&self) -> crate::wfp::Spec {
        let ip = u32::from(self.remote_ip);
        crate::wfp::Spec {
            layer: crate::wfp::Layer::Out,
            weight: PENDING_WEIGHT,
            block: true,
            app_path: self.proc_path.clone(),
            remote: Some((ip, ip)),
            proto: Some(proto_num(self.proto)),
            port: Some(self.remote_port),
        }
    }

    /// 决策结果转规则;process 填映像名(规则语义:映像名或路径结尾)。
    /// 范围决定持久规则粒度:Target = 进程+目标IP(不限端口),
    /// Process = 仅进程;Once 保持当前连接的精确身份
    pub fn to_rule(&self, action: Action) -> Rule {
        let (remote_kind, remote_value, port) = match self.scope {
            Scope::Once => (RemoteKind::Ip, self.remote_ip.to_string(), self.remote_port),
            Scope::Target => (RemoteKind::Ip, self.remote_ip.to_string(), 0),
            Scope::Process => (RemoteKind::Any, String::new(), 0),
        };
        Rule {
            id: 0,
            name: format!("{} -> {}", self.process, self.remote_display()),
            enabled: true,
            priority: 0,
            action,
            direction: Direction::Any,
            proto: Some(self.proto),
            process: self.process.clone(),
            remote_kind,
            remote_value,
            port,
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
fn is_askable(ip: &Ipv4Addr) -> bool {
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.octets()[0] >= 240)
}

/// 询问编排:新增连接检测、去重、队列与当前弹窗
pub struct Asker {
    pub queue: VecDeque<AskItem>,
    pub active: Option<AskItem>,
    /// 已询问过的连接身份(会话内不重复问)
    asked: HashSet<u64>,
    /// 上轮出现过的连接 id(conn.id 稳定)
    seen: HashSet<u64>,
}

impl Asker {
    pub fn new() -> Asker {
        Asker {
            queue: VecDeque::new(),
            active: None,
            asked: HashSet::new(),
            seen: HashSet::new(),
        }
    }

    /// 每轮喂入连接快照:首次出现且未命中任何规则的 (进程,目标IP) 身份入队。
    /// 新增判定基于 conn.id(连接四元组哈希,快照间稳定)。
    /// 启动首帧为基线:存量连接视为放行(身份记入已问),只询问之后的新连接
    pub fn update(&mut self, conns: &[Connection], rules: &RuleSet, rdns: &rdns::Rdns) {
        let baseline = self.seen.is_empty() && !conns.is_empty();
        let mut current = HashSet::with_capacity(conns.len());
        let mut fresh: Vec<&Connection> = Vec::new();
        for c in conns {
            if self.seen.insert(c.id) {
                fresh.push(c);
            }
            current.insert(c.id);
        }
        self.seen = current;

        for c in fresh {
            let key = identity_key(c);
            if baseline {
                self.asked.insert(key);
                continue;
            }
            if !self.asked.insert(key) {
                continue;
            }
            // 静默放行:系统进程、未知进程(无法生成有意义的进程条件)与
            // 不可询问的目标(回环/局域网/保留段等)
            if c.pid == SYSTEM_PID
                || c.pid == 0
                || c.process.is_empty()
                || (c.proto == Protocol::Udp && c.remote_ip.is_unspecified())
                || !is_askable(&c.remote_ip)
            {
                continue;
            }
            let req = MatchReq::from_conn(c, rdns.lookup(c.remote_ip));
            if rules.evaluate(&req).is_some() {
                continue;
            }
            if self.queue.len() >= QUEUE_LIMIT {
                continue;
            }
            self.queue.push_back(AskItem {
                proc_path: c.proc_path.clone(),
                process: c.process.clone(),
                pid: c.pid,
                remote_ip: c.remote_ip,
                remote_port: c.remote_port,
                proto: c.proto,
                domain: rdns.lookup(c.remote_ip).map(str::to_owned),
                deadline: Instant::now() + ASK_TIMEOUT,
                scope: Scope::Once,
            });
        }
    }

    /// 弹窗调度:当前无弹窗时从队列取下一个;返回是否正在询问
    pub fn poll(&mut self) -> bool {
        if self.active.is_none()
            && let Some(mut item) = self.queue.pop_front()
        {
            item.deadline = Instant::now() + ASK_TIMEOUT;
            self.active = Some(item);
        }
        self.active.is_some()
    }

    /// 当前弹窗倒计时是否结束(视为默认动作 拒绝·仅本次)
    pub fn expired(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|a| a.deadline <= Instant::now())
    }

    /// 取走当前询问(决策处理)
    pub fn take(&mut self) -> Option<AskItem> {
        self.active.take()
    }

    /// 关闭询问(配置项关闭时):丢弃队列与当前弹窗
    pub fn clear(&mut self) {
        self.queue.clear();
        self.active = None;
    }
}

/// 连接身份:(进程路径/名, 目标IP)。端口/协议不参与去重,
/// 同一目标多端口只询问一次
fn identity_key(c: &Connection) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    c.proc_path.as_ref().unwrap_or(&c.process).hash(&mut h);
    c.remote_ip.hash(&mut h);
    h.finish()
}
