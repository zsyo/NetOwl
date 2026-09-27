//! 真实连接采集:GetExtendedTcpTable/GetExtendedUdpTable(owner-PID)定期
//! 快照,OpenProcess 反查进程映像名。均为只读查询,普通用户权限即可,
//! 无需管理员。
//!
//! 表快照无字节计数语义(ESTATS Data 采集在本机系统上返回值不可信,
//! 已实测为部分未初始化数据),下载/上传列显示 0;短命 UDP 交互与字节
//! 统计依赖后续 ETW 事件源补齐。

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP_STATE_LAST_ACK, MIB_TCP_STATE_SYN_SENT,
    MIB_TCPTABLE_OWNER_PID, MIB_UDPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    UDP_TABLE_OWNER_PID,
};
use windows::Win32::Networking::WinSock::AF_INET;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::collector::{Collector, CollectorKind};
use crate::model::{Connection, Protocol};

/// 表快照间隔:连接增减的可见延迟上限(与任务管理器刷新节奏相当)
const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// 表查询重试次数:两次调用之间表增长会要求更大缓冲,属正常时序
const QUERY_RETRIES: usize = 3;
/// System 进程(PID 4)持有内核级 socket,任务管理器同样显示为 System
const SYSTEM_PID: u32 = 4;
/// 活动状态区间 [SYN_SENT, LAST_ACK]:LISTEN 是监听而非连接,
/// CLOSED/TIME_WAIT 已无数据交互,均不入连接列表
const STATE_ACTIVE_MIN: u32 = MIB_TCP_STATE_SYN_SENT.0 as u32;
const STATE_ACTIVE_MAX: u32 = MIB_TCP_STATE_LAST_ACK.0 as u32;

/// 连接身份:四元组 + 归属进程;快照间据此识别同一连接。
/// 地址为网络字节序原始值,端口为主机序;UDP 无远端以 0 填充。
#[derive(Clone, PartialEq, Eq, Hash)]
struct ConnKey {
    proto: Protocol,
    local_addr: u32,
    local_port: u16,
    remote_addr: u32,
    remote_port: u16,
    pid: u32,
}

/// 由连接身份派生稳定 id(同连接跨快照保持一致,驱动地图粒子相位等)
fn key_id(key: &ConnKey) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    h.finish()
}

/// 真实采集器:每 POLL_INTERVAL 查询一次系统表,快照间维护连接与进程名缓存
pub struct TableCollector {
    /// 有序连接快照(累计流量降序),非轮询帧直接返回
    ordered: Vec<Connection>,
    /// 连接身份 -> 最近快照(保留 first_seen)
    live: HashMap<ConnKey, Connection>,
    /// PID -> 进程映像名(仅存活于连接表中的进程)
    proc_names: HashMap<u32, String>,
    last_poll: Instant,
    pending_poll: bool,
}

impl TableCollector {
    pub fn new() -> Self {
        TableCollector {
            ordered: Vec::new(),
            live: HashMap::new(),
            proc_names: HashMap::new(),
            last_poll: Instant::now(),
            pending_poll: true,
        }
    }

    /// 查询 TCP/UDP 表并重建快照;表查询失败保留上一轮快照并输出错误
    fn poll(&mut self) {
        let tcp = match query_tcp() {
            Ok(rows) => rows,
            Err(e) => {
                eprintln!("[Collector] TCP 表查询失败: {e}");
                return;
            }
        };
        let udp = match query_udp() {
            Ok(rows) => rows,
            Err(e) => {
                eprintln!("[Collector] UDP 表查询失败: {e}");
                return;
            }
        };

        let now = Instant::now();
        let old = std::mem::take(&mut self.live);
        let mut new_live: HashMap<ConnKey, Connection> =
            HashMap::with_capacity(tcp.len() + udp.len());

        for key in tcp.into_iter().chain(udp) {
            let id = key_id(&key);
            let pid = key.pid;
            let proto = key.proto;
            let remote_ip = Ipv4Addr::from(u32::from_be(key.remote_addr));
            let remote_port = key.remote_port;
            let first_seen = old.get(&key).map_or(now, |c| c.first_seen);
            new_live.insert(key, Connection {
                id,
                pid,
                process: self.process_name(pid),
                proto,
                remote_ip,
                remote_port,
                city: None,
                bytes_in: 0,
                bytes_out: 0,
                first_seen,
            });
        }

        // 进程名缓存只保留本轮出现在连接表中的进程(PID 复用随行消失而自然失效)
        let live_pids: HashSet<u32> = new_live.keys().map(|k| k.pid).collect();
        self.proc_names.retain(|pid, _| live_pids.contains(pid));

        self.live = new_live;
        self.ordered = self.live.values().cloned().collect();
        // 表快照字节恒为 0:按连接建立时间倒序,最新连接在前
        self.ordered
            .sort_by_key(|c| (std::cmp::Reverse(c.total_bytes()), std::cmp::Reverse(c.first_seen)));
        self.last_poll = now;
    }

    /// 进程名(带缓存);查询失败缓存空串,列表显示占位文本
    fn process_name(&mut self, pid: u32) -> String {
        if let Some(name) = self.proc_names.get(&pid) {
            return name.clone();
        }
        let name = query_process_name(pid);
        self.proc_names.insert(pid, name.clone());
        name
    }
}

impl Collector for TableCollector {
    fn snapshot(&mut self) -> Vec<Connection> {
        if self.pending_poll || self.last_poll.elapsed() >= POLL_INTERVAL {
            self.pending_poll = false;
            self.poll();
        }
        self.ordered.clone()
    }

    fn kind(&self) -> CollectorKind {
        CollectorKind::Real
    }
}

/// TCP 表查询:仅活动状态行(LISTEN 是监听而非连接,CLOSED/TIME_WAIT
/// 已无数据交互)
fn query_tcp() -> Result<Vec<ConnKey>, String> {
    let buf = query_table(|p, size| unsafe {
        GetExtendedTcpTable(p, size, false, AF_INET.0 as u32, TCP_TABLE_OWNER_PID_ALL, 0)
    })?;
    let table = unsafe { &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID) };
    // ANY_SIZE 惯用法:行数组紧跟 dwNumEntries,实际行数由 dwNumEntries 给出
    let rows = unsafe {
        std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize)
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if row.dwState < STATE_ACTIVE_MIN || row.dwState > STATE_ACTIVE_MAX {
            continue;
        }
        out.push(ConnKey {
            proto: Protocol::Tcp,
            local_addr: row.dwLocalAddr,
            local_port: row.dwLocalPort as u16,
            remote_addr: row.dwRemoteAddr,
            remote_port: row.dwRemotePort as u16,
            pid: row.dwOwningPid,
        });
    }
    Ok(out)
}

/// UDP 表查询:本机 socket 行(无远端语义)
fn query_udp() -> Result<Vec<ConnKey>, String> {
    let buf = query_table(|p, size| unsafe {
        GetExtendedUdpTable(p, size, false, AF_INET.0 as u32, UDP_TABLE_OWNER_PID, 0)
    })?;
    let table = unsafe { &*(buf.as_ptr() as *const MIB_UDPTABLE_OWNER_PID) };
    let rows = unsafe {
        std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize)
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(ConnKey {
            proto: Protocol::Udp,
            local_addr: row.dwLocalAddr,
            local_port: row.dwLocalPort as u16,
            remote_addr: 0,
            remote_port: 0,
            pid: row.dwOwningPid,
        });
    }
    Ok(out)
}

/// 两段式表查询(首次调用取所需缓冲大小,不足时按返回值重试);
/// `fill` 返回 WIN32_ERROR 码
fn query_table(fill: impl Fn(Option<*mut std::ffi::c_void>, *mut u32) -> u32) -> Result<Vec<u8>, String> {
    const SUCCESS: u32 = ERROR_SUCCESS.0;
    const INSUFFICIENT: u32 = ERROR_INSUFFICIENT_BUFFER.0;
    let mut size = 0u32;
    for _ in 0..QUERY_RETRIES {
        let mut buf = vec![0u8; size.max(64) as usize];
        match fill(Some(buf.as_mut_ptr() as *mut std::ffi::c_void), &mut size) {
            SUCCESS => return Ok(buf),
            INSUFFICIENT => continue,
            other => return Err(format!("code {other}")),
        }
    }
    Err("缓冲大小重试耗尽".to_owned())
}

/// 进程映像名(文件名部分);打开失败(刚退出/受保护)返回空串
fn query_process_name(pid: u32) -> String {
    if pid == SYSTEM_PID {
        return "System".to_owned();
    }
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 512];
        let mut size = buf.len() as u32;
        let name = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
        .ok()
        .map(|_| {
            let full = String::from_utf16_lossy(&buf[..size as usize]);
            full.rsplit(['\\', '/']).next().unwrap_or_default().to_owned()
        })
        .unwrap_or_default();
        let _ = CloseHandle(handle);
        name
    }
}
