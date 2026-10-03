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
//! 事件解析与流聚合在 decode。

mod decode;

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::System::Diagnostics::Etw::{
    CloseTrace, ControlTraceW, EVENT_RECORD, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW,
    EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE, EnableTraceEx2, OpenTraceW,
    PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME, ProcessTrace, StartTraceW,
    WNODE_FLAG_TRACED_GUID,
};
use windows::core::{GUID, PCWSTR, PWSTR};

use crate::model::Protocol;

/// Microsoft-Windows-Kernel-Network 提供程序
const PROVIDER: GUID = GUID::from_u128(0x7dd42a49_5329_4832_8dfd_43d979153a88);
/// 会话 GUID(任意固定值,便于查询与残留清理)
const SESSION_GUID: GUID = GUID::from_u128(0x6e744f77_6c4b_4e54_4f57_4c0000000001);
/// 会话名(全局命名实例;启动时按名清理残留)
const SESSION_NAME: PCWSTR = windows::core::w!("NetOwl-KernelNet");
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
        stop_session()?;
        let agg = Arc::new(Mutex::new(Agg::default()));
        let handle = {
            let agg = Arc::clone(&agg);
            std::thread::Builder::new()
                .name("etw-consumer".into())
                .spawn(move || unsafe { run_consumer(agg) })
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
            for (k, _) in oldest.into_iter().take(excess) {
                if let Some(st) = g.flows.remove(&k) {
                    g.finished.push((k, st));
                }
            }
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
        let _ = stop_session();
        if let Some(h) = self.consumer.take() {
            let _ = h.join();
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

/// 启动会话并进入事件循环(消费线程主函数);失败仅记录后返回
unsafe fn run_consumer(agg: Arc<Mutex<Agg>>) {
    unsafe {
        let mut session = Default::default();
        let props = prepare_props();
        let err = StartTraceW(&mut session, SESSION_NAME, props);
        if err.is_err() {
            tracing::warn!("[ETW] 会话启动失败({err:?}),流量字节与短命连接不可用");
            return;
        }
        // ENABLE(1)、Information(4)、全部关键字
        let enable = EnableTraceEx2(session, &PROVIDER, 1, 4, u64::MAX, 0, 0, None);
        if enable.is_err() {
            tracing::warn!("[ETW] 提供程序启用失败({enable:?}),流量字节与短命连接不可用");
            let _ = ControlTraceW(
                Default::default(),
                SESSION_NAME,
                props,
                EVENT_TRACE_CONTROL_STOP,
            );
            return;
        }
        let mut logfile = EVENT_TRACE_LOGFILEW {
            LoggerName: PWSTR(SESSION_NAME.as_ptr() as *mut u16),
            ..Default::default()
        };
        logfile.Anonymous2.EventRecordCallback = Some(on_event);
        logfile.Context = Arc::as_ptr(&agg) as *mut core::ffi::c_void;
        logfile.Anonymous1.ProcessTraceMode =
            PROCESS_TRACE_MODE_EVENT_RECORD | PROCESS_TRACE_MODE_REAL_TIME;
        let handle = OpenTraceW(&mut logfile);
        let _ = ProcessTrace(&[handle], None, None);
        let _ = CloseTrace(handle);
        let _ = ControlTraceW(
            Default::default(),
            SESSION_NAME,
            props,
            EVENT_TRACE_CONTROL_STOP,
        );
    }
}

/// 构造 StartTrace/ControlTrace 共用的会话属性缓冲
/// (结构体后跟会话名 UTF-16 区)
unsafe fn prepare_props() -> *mut EVENT_TRACE_PROPERTIES {
    unsafe {
        let name_wide = SESSION_NAME.as_wide();
        let total = size_of::<EVENT_TRACE_PROPERTIES>() + (name_wide.len() + 1) * 2;
        let buf = vec![0u8; total].leak() as *mut [u8] as *mut u8;
        let props = buf as *mut EVENT_TRACE_PROPERTIES;
        (*props).Wnode.BufferSize = total as u32;
        (*props).Wnode.Guid = SESSION_GUID;
        (*props).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        (*props).Wnode.ClientContext = 1;
        // BufferSize 单位 KB(1MB/缓冲);实时模式满缓冲即投递消费者,
        // Kernel-Network 事件量下足量且内核侧总占用 = 缓冲数 × 1MB。
        // 此前误传字节值(64MiB/缓冲),乘 CPU 数后会话缓冲达 GB 级
        (*props).BufferSize = 1024;
        (*props).MinimumBuffers = 8;
        (*props).MaximumBuffers = 32;
        (*props).FlushTimer = 1;
        (*props).LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        (*props).LoggerNameOffset = size_of::<EVENT_TRACE_PROPERTIES>() as u32;
        (*props).LogFileNameOffset = 0;
        props
    }
}

/// 停止同名会话(含上次异常退出的残留);会话不存在视为已停止
fn stop_session() -> Result<(), String> {
    const ERROR_WMI_INSTANCE_NOT_FOUND: u32 = 4201;
    let props = unsafe { prepare_props() };
    let err = unsafe {
        ControlTraceW(
            Default::default(),
            SESSION_NAME,
            props,
            EVENT_TRACE_CONTROL_STOP,
        )
    };
    // 会话不存在 = 无需清理;其余错误如实上报
    if err.is_err() && err.0 != ERROR_WMI_INSTANCE_NOT_FOUND {
        return Err(format!("停止残留 ETW 会话失败: {err:?}"));
    }
    Ok(())
}

/// 实时事件回调(ProcessTrace 消费线程内同步调用;Context 即 Agg)
unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    unsafe {
        let r = &*record;
        if r.EventHeader.ProviderId != PROVIDER {
            return;
        }
        if usize::from(r.UserDataLength) < PAYLOAD_HEAD || r.UserData.is_null() {
            return;
        }
        let agg = &*(r.UserContext as *const Mutex<Agg>);
        let Ok(mut g) = agg.try_lock() else {
            return;
        };
        decode::handle_event(&mut g, r);
    }
}
