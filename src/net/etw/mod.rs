//! ETW 流量事件采集(Microsoft-Windows-Kernel-Network):表快照无字节
//! 语义且会漏掉采样间隙的短命连接,本模块消费内核网络事件补齐——
//! TCP 收发字节(EventID 10/11)、UDP 收发字节(42/43)、连接建立与
//! 关闭(12/15/13,发起方向与短命连接捕获)。需管理员权限;会话为
//! 全局命名实例,启动时先清理同名残留,退出时停止。
//!
//! 字节序与字段布局经探针实测(对拍 tracerpt):payload 前 20 字节
//! 全部关注事件同构 —— PID@0/size@4(u32 主机序)、daddr@8/saddr@12
//! (IPv4 网络序字节)、dport@16/sport@18(u16 网络序)。事件视角:
//! 发送/发起/接受/关闭事件的 saddr 为本机侧,接收事件的 daddr 为
//! 本机侧,按此归一本地/远端。EventID 18(用户态拷贝)与 11 对同一
//! 批字节双计,IPv6 事件(26+ 系列)暂不消费,均显式忽略。
//! 事件解析与流聚合在 decode,会话生命周期在 session。

mod decode;
mod session;

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::model::Protocol;

/// 关注事件 payload 头部长度(PID/size/daddr/saddr/dport/sport)
const PAYLOAD_HEAD: usize = 20;
/// UDP 流空闲判定:超时无事件视为完结(UDP 无关闭事件)
const UDP_IDLE: Duration = Duration::from_secs(10);
/// TCP 流空闲兜底:close 事件可能因多处理器缓冲乱序晚于末尾字节事件
/// 到达,流被拆成残余;超时无事件也按完结收割(曾入表快照的流由
/// 合并方过滤,不产生错误历史)
const TCP_IDLE: Duration = Duration::from_secs(60);
/// 流表上限保护:内核 close 事件丢失时按最旧强转完结,防无限增长
const MAX_FLOWS: usize = 4096;

/// 一条流(本机进程视角的连接)聚合键
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FlowKey {
    pub pid: u32,
    pub proto: Protocol,
    pub local_ip: Ipv4Addr,
    pub local_port: u16,
    pub remote_ip: Ipv4Addr,
    pub remote_port: u16,
}

/// 流聚合快照(字节自会话启动累计;发起方向为 ETW 真实事件)
#[derive(Clone, Copy, Debug)]
pub struct FlowAgg {
    pub key: FlowKey,
    pub up_bytes: u64,
    pub down_bytes: u64,
    /// Some(true) = 出站发起(connect),Some(false) = 入站接受(accept)
    pub initiated_out: Option<bool>,
    /// 首个/最近事件时刻(单调钟,供完结事件换算时间区间)
    pub first: Instant,
    pub last: Instant,
}

struct FlowStat {
    first: Instant,
    up: u64,
    down: u64,
    last: Instant,
    initiated_out: Option<bool>,
}

#[derive(Default)]
struct Agg {
    flows: HashMap<FlowKey, FlowStat>,
    finished: Vec<(FlowKey, FlowStat)>,
}

/// ETW 采集句柄:消费线程持会话,主线程经 snapshot/take_finished 读取;
/// drop 时停止会话并回收线程
pub struct Etw {
    agg: Arc<Mutex<Agg>>,
    consumer: Option<JoinHandle<()>>,
}

impl Etw {
    /// 启动 ETW 会话与消费线程;须在提权进程内调用,失败返回错误描述
    pub fn start() -> Result<Etw, String> {
        session::stop_session()?;
        let agg = Arc::new(Mutex::new(Agg::default()));
        let handle = {
            let agg = Arc::clone(&agg);
            std::thread::Builder::new()
                .name("etw-consumer".into())
                .spawn(move || unsafe { session::run_consumer(agg) })
                .map_err(|e| format!("启动 ETW 消费线程失败: {e}"))?
        };
        Ok(Etw {
            agg,
            consumer: Some(handle),
        })
    }

    /// 当前活跃流快照;同时把空闲超时的流(UDP 短/长活 TCP 兜底)
    /// 转入完结队列
    pub fn snapshot(&self) -> Vec<FlowAgg> {
        let mut g = lock(&self.agg);
        let now = Instant::now();
        let idle: Vec<FlowKey> = g
            .flows
            .iter()
            .filter(|(k, st)| {
                let idle_for = now.duration_since(st.last);
                match k.proto {
                    Protocol::Udp => idle_for > UDP_IDLE,
                    Protocol::Tcp => idle_for > TCP_IDLE,
                }
            })
            .map(|(k, _)| *k)
            .collect();
        for k in idle {
            if let Some(st) = g.flows.remove(&k) {
                g.finished.push((k, st));
            }
        }
        if g.flows.len() > MAX_FLOWS {
            let mut oldest: Vec<(FlowKey, Instant)> =
                g.flows.iter().map(|(k, st)| (*k, st.last)).collect();
            oldest.sort_by_key(|(_, t)| *t);
            let excess = g.flows.len() - MAX_FLOWS;
            tracing::warn!(
                "[ETW] 流表超过 {MAX_FLOWS} 上限,强制完结最旧 {excess} 条流(close 事件疑似丢失)"
            );
            for (k, _) in oldest.into_iter().take(excess) {
                if let Some(st) = g.flows.remove(&k) {
                    g.finished.push((k, st));
                }
            }
        }
        // 事件回调与快照锁竞争丢弃的计数(仅异常时非零)
        let missed = session::LOCK_MISSED.swap(0, Ordering::Relaxed);
        if missed > 0 {
            tracing::debug!("[ETW] 缓冲锁竞争,本轮丢弃 {missed} 条事件");
        }
        g.flows
            .iter()
            .map(|(k, st)| FlowAgg {
                key: *k,
                up_bytes: st.up,
                down_bytes: st.down,
                initiated_out: st.initiated_out,
                first: st.first,
                last: st.last,
            })
            .collect()
    }

    /// 取走已完结流(TCP close 或空闲超时),供短命连接收割
    pub fn take_finished(&self) -> Vec<FlowAgg> {
        let mut g = lock(&self.agg);
        g.finished
            .drain(..)
            .map(|(k, st)| FlowAgg {
                key: k,
                up_bytes: st.up,
                down_bytes: st.down,
                initiated_out: st.initiated_out,
                first: st.first,
                last: st.last,
            })
            .collect()
    }

    /// 停止会话并等待消费线程退出
    pub fn shutdown(&mut self) {
        if let Err(e) = session::stop_session() {
            tracing::warn!("[ETW] 停止会话失败: {e}");
        } else {
            tracing::info!("[ETW] 流量事件采集会话已停止");
        }
        if let Some(h) = self.consumer.take()
            && let Err(e) = h.join()
        {
            tracing::debug!("[ETW] 消费线程异常退出: {e:?}");
        }
    }
}

impl Drop for Etw {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn lock(g: &Mutex<Agg>) -> std::sync::MutexGuard<'_, Agg> {
    g.lock().unwrap_or_else(|e| e.into_inner())
}
