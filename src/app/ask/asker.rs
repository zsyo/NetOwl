//! 询问编排状态机:新增连接检测与身份去重、入队静默放行判定、
//! 弹窗调度与倒计时、决策完成后的去重标记解除。

use std::collections::{HashSet, VecDeque};
use std::time::Instant;

use super::item::{AskItem, SYSTEM_PID, identity_key, remote_text};
use super::{ASK_TIMEOUT, Scope};
use crate::model::Connection;
use crate::net::rdns;
use crate::rules::{MatchReq, RuleSet};

/// 待询问队列上限:超出后新身份静默放行,防瞬时连接风暴刷屏
const QUEUE_LIMIT: usize = 10;

/// 询问编排:新增连接检测、去重、队列与当前弹窗
pub struct Asker {
    pub queue: VecDeque<AskItem>,
    pub active: Option<AskItem>,
    /// 已询问过的连接身份(会话内不重复问)
    asked: HashSet<u64>,
    /// 上轮出现过的连接 id(conn.id 稳定)
    seen: HashSet<u64>,
    /// 启动基线是否已完成:只在第一个非空快照做一次,之后快照短暂为空
    /// (休眠唤醒/网络抖动)不得触发重新基线化,否则存量身份被整体静默
    /// 放行,询问功能失效
    baselined: bool,
    /// 队列超限告警已发:超限期间每轮都有新身份命中,只报首次
    limit_warned: bool,
}

impl Asker {
    pub fn new() -> Asker {
        Asker {
            queue: VecDeque::new(),
            active: None,
            asked: HashSet::new(),
            seen: HashSet::new(),
            baselined: false,
            limit_warned: false,
        }
    }

    /// 每轮喂入连接快照:首次出现且未命中任何规则的 (进程,目标IP) 身份入队。
    /// 新增判定基于 conn.id(连接四元组哈希,快照间稳定)。
    /// 启动首帧(第一个非空快照)为基线:存量连接视为放行(身份记入已问),
    /// 只询问之后的新连接
    pub fn update(&mut self, conns: &[Connection], rules: &RuleSet, rdns: &rdns::Rdns) {
        let baseline = if !self.baselined && !conns.is_empty() {
            self.baselined = true;
            true
        } else {
            false
        };
        let mut current = HashSet::with_capacity(conns.len());
        let mut fresh: Vec<&Connection> = Vec::new();
        for c in conns {
            // "身份未定"的行不进已见集合,待身份补全后再评估是否询问:
            // ① UDP 行首见时无远端(系统 UDP 表不含对端),远端由 ETW 延后
            //    一回合填,而 conn.id 不变——首轮即记入 seen 会使该连接
            //    终身不再是 fresh,询问永远不触发;
            // ② 进程名未解析出的行(路径反查与名字兜底都失败时),名字补齐
            //    后身份键随之变化,同样需要重新评估
            if (c.proto == crate::model::Protocol::Udp && c.remote_ip.is_unspecified())
                || c.process.is_empty()
            {
                continue;
            }
            if self.seen.insert(c.id) {
                fresh.push(c);
            }
            current.insert(c.id);
        }
        self.seen = current;
        if baseline {
            tracing::debug!("[Ask] 启动基线完成,存量 {} 个连接身份放行", fresh.len());
        }

        for c in fresh {
            let key = identity_key(c.proc_path.as_deref(), &c.process, c.remote_ip);
            if baseline {
                self.asked.insert(key);
                continue;
            }
            if !self.asked.insert(key) {
                continue;
            }
            // 静默放行:自身(NetOwl 不询问/拦截自己)、系统进程、未知进程
            // (无法生成有意义的进程条件)与不可询问的目标(回环/局域网/保留段等)
            if c.pid == std::process::id()
                || c.pid == SYSTEM_PID
                || c.pid == 0
                || c.process.is_empty()
                || (c.proto == crate::model::Protocol::Udp && c.remote_ip.is_unspecified())
                || !crate::net::rdns::is_queryable(&c.remote_ip)
            {
                continue;
            }
            let req = MatchReq::from_conn(c, rdns.lookup(c.remote_ip));
            if rules.evaluate(&req).is_some() {
                continue;
            }
            if self.queue.len() >= QUEUE_LIMIT {
                if !self.limit_warned {
                    self.limit_warned = true;
                    tracing::warn!(
                        "[Ask] 询问队列已达上限 {QUEUE_LIMIT},新连接静默放行(身份已去重,不再询问)"
                    );
                }
                continue;
            }
            self.limit_warned = false;
            tracing::info!(
                "[Ask] 新连接询问入队:{}({}) -> {}",
                c.process,
                c.pid,
                remote_text(&c.remote_ip, c.remote_port, rdns.lookup(c.remote_ip))
            );
            self.queue.push_back(AskItem {
                proc_path: c.proc_path.clone(),
                process: c.process.clone(),
                pid: c.pid,
                local_port: c.local_port,
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

    /// 决策完成后解除该身份的已询问标记:仅本次决策只作用于当前连接,
    /// 其结束后同目标新连接重新询问(永久决策靠生成的规则求值命中,
    /// 无需解除)
    pub fn unask(&mut self, item: &AskItem) {
        self.asked.remove(&identity_key(
            item.proc_path.as_deref(),
            &item.process,
            item.remote_ip,
        ));
    }
}
