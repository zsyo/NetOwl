//! Win32 底层查询原语:TCP/UDP owner-PID 表快照与进程映像路径。
//! 全部只读 API,普通用户权限可调用;供 windows_table 采集器使用。

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::net::Ipv4Addr;

use windows::Wdk::System::SystemInformation::{NtQuerySystemInformation, SystemProcessInformation};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, STATUS_INFO_LENGTH_MISMATCH,
};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP_STATE_DELETE_TCB, MIB_TCP_STATE_LAST_ACK,
    MIB_TCP_STATE_SYN_SENT, MIB_TCPROW_LH, MIB_TCPROW_LH_0, MIB_TCPTABLE_OWNER_PID,
    MIB_UDPTABLE_OWNER_PID, SetTcpEntry, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
};
use windows::Win32::Networking::WinSock::AF_INET;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::System::WindowsProgramming::SYSTEM_PROCESS_INFORMATION;
use windows::core::PWSTR;

use crate::model::{Connection, Protocol};

/// 活动状态区间 [SYN_SENT, LAST_ACK]:LISTEN 是监听而非连接,
/// CLOSED/TIME_WAIT 已无数据交互,均不入连接列表
const STATE_ACTIVE_MIN: u32 = MIB_TCP_STATE_SYN_SENT.0 as u32;
const STATE_ACTIVE_MAX: u32 = MIB_TCP_STATE_LAST_ACK.0 as u32;
/// 表查询重试次数:两次调用之间表增长会要求更大缓冲,属正常时序
const QUERY_RETRIES: usize = 3;

/// 连接身份:四元组 + 归属进程;快照间据此识别同一连接。
/// 地址为网络字节序原始值,端口已转主机序;UDP 无远端以 0 填充。
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ConnKey {
    pub proto: Protocol,
    pub local_addr: u32,
    pub local_port: u16,
    pub remote_addr: u32,
    pub remote_port: u16,
    pub pid: u32,
}

impl ConnKey {
    /// 由连接身份派生稳定 id(同连接跨快照保持一致,驱动地图粒子相位等)
    pub fn id(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut h);
        h.finish()
    }

    /// 远端 IPv4 地址(网络字节序原始值转主机序)
    pub fn remote_ip(&self) -> Ipv4Addr {
        Ipv4Addr::from(u32::from_be(self.remote_addr))
    }

    /// 本地 IPv4 地址(网络字节序原始值转主机序)
    pub fn local_addr_ipv4(&self) -> Ipv4Addr {
        Ipv4Addr::from(u32::from_be(self.local_addr))
    }
}

/// 请求内核删除一条 TCP 连接(状态置 DELETE_TCB,等价强制关闭);
/// 需要管理员权限,非提权返回拒绝访问。地址/端口按 MIB_TCPROW 的
/// 网络序约定重建:地址为内存字节序原始值,端口取 htons 值置于低 16 位
pub fn close_tcp_connection(conn: &Connection) -> Result<(), String> {
    let row = MIB_TCPROW_LH {
        Anonymous: MIB_TCPROW_LH_0 {
            State: MIB_TCP_STATE_DELETE_TCB,
        },
        dwLocalAddr: u32::from_ne_bytes(conn.local_addr.octets()),
        dwLocalPort: u32::from(conn.local_port.to_be()),
        dwRemoteAddr: u32::from_ne_bytes(conn.remote_ip.octets()),
        dwRemotePort: u32::from(conn.remote_port.to_be()),
    };
    let err = unsafe { SetTcpEntry(&row) };
    if err == 0 {
        Ok(())
    } else {
        Err(format!("SetTcpEntry 返回 0x{err:08X}"))
    }
}

/// TCP 表查询:仅活动状态行
pub fn query_tcp() -> Result<Vec<ConnKey>, String> {
    let buf = query_table(|p, size| unsafe {
        GetExtendedTcpTable(p, size, false, AF_INET.0 as u32, TCP_TABLE_OWNER_PID_ALL, 0)
    })?;
    let table = unsafe { &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID) };
    // ANY_SIZE 惯用法:行数组紧跟 dwNumEntries,实际行数由 dwNumEntries 给出
    let rows =
        unsafe { std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize) };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if row.dwState < STATE_ACTIVE_MIN || row.dwState > STATE_ACTIVE_MAX {
            continue;
        }
        out.push(ConnKey {
            proto: Protocol::Tcp,
            local_addr: row.dwLocalAddr,
            // 表结构里的端口是 htons 后的网络序值(低 16 位),须转主机序;
            // 直接 as u16 会得到字节序反转的端口(如 443 -> 47873),
            // 导致与 ETW 流的合并键永远对不上
            local_port: u16::from_be(row.dwLocalPort as u16),
            remote_addr: row.dwRemoteAddr,
            remote_port: u16::from_be(row.dwRemotePort as u16),
            pid: row.dwOwningPid,
        });
    }
    Ok(out)
}

/// UDP 表查询:本机 socket 行(无远端语义)
pub fn query_udp() -> Result<Vec<ConnKey>, String> {
    let buf = query_table(|p, size| unsafe {
        GetExtendedUdpTable(p, size, false, AF_INET.0 as u32, UDP_TABLE_OWNER_PID, 0)
    })?;
    let table = unsafe { &*(buf.as_ptr() as *const MIB_UDPTABLE_OWNER_PID) };
    let rows =
        unsafe { std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize) };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(ConnKey {
            proto: Protocol::Udp,
            local_addr: row.dwLocalAddr,
            local_port: u16::from_be(row.dwLocalPort as u16),
            remote_addr: 0,
            remote_port: 0,
            pid: row.dwOwningPid,
        });
    }
    Ok(out)
}

/// 进程映像完整路径;打开失败(刚退出/受保护进程)返回 None
pub fn query_process_path(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let path = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
        .ok()
        .map(|_| String::from_utf16_lossy(&buf[..size as usize]));
        let _ = CloseHandle(handle);
        path
    }
}

/// 全量进程映像名快照(PID -> 映像名):NtQuerySystemInformation 枚举
/// 不打开进程句柄,被 DACL 拒绝 OpenProcess 的服务进程也能拿到名字
/// (任务管理器同源);仅作为路径反查失败后的名字兜底,拿不到完整路径
pub fn query_process_names() -> HashMap<u32, String> {
    // 缓冲自适应:所需大小随进程数增长,STATUS_INFO_LENGTH_MISMATCH 时
    // 倍增重试,上限 16 MiB(异常则放弃,返回空表由上层按无名处理);
    // 缓冲按 u64 对齐分配,SYSTEM_PROCESS_INFORMATION 含指针对齐 8
    let mut size = 512 * 1024usize;
    loop {
        let words = size.div_ceil(size_of::<u64>());
        let mut buf = vec![0u64; words];
        let byte_len = buf.len() * size_of::<u64>();
        let mut ret = 0u32;
        let status = unsafe {
            NtQuerySystemInformation(
                SystemProcessInformation,
                buf.as_mut_ptr().cast(),
                byte_len as u32,
                &mut ret,
            )
        };
        if status.is_ok() {
            return parse_process_names(&buf);
        }
        if status == STATUS_INFO_LENGTH_MISMATCH && size < 16 * 1024 * 1024 {
            size *= 2;
            continue;
        }
        return HashMap::new();
    }
}

/// 解析 SystemProcessInformation 单链表(仅取名字与 PID,布局由
/// windows crate 的 WDK 结构保证):节点 ImageName.Buffer 指向本缓冲区
/// 内部,仍做区间校验防越界读;无名节点(内核线程等)跳过
fn parse_process_names(buf: &[u64]) -> HashMap<u32, String> {
    let step = std::mem::size_of::<SYSTEM_PROCESS_INFORMATION>();
    let mut out = HashMap::new();
    let base = buf.as_ptr() as usize;
    let byte_len = std::mem::size_of_val(buf);
    let mut off = 0usize;
    while off + step <= byte_len {
        let info = unsafe { &*((base + off) as *const SYSTEM_PROCESS_INFORMATION) };
        let name = &info.ImageName;
        if name.Length > 0 && !name.Buffer.is_null() {
            let start = name.Buffer.0 as usize;
            let span = name.Length as usize;
            if start >= base && start + span <= base + byte_len {
                let chars = unsafe { std::slice::from_raw_parts(name.Buffer.0, span / 2) };
                out.insert(
                    info.UniqueProcessId.0 as usize as u32,
                    String::from_utf16_lossy(chars),
                );
            }
        }
        if info.NextEntryOffset == 0 {
            break;
        }
        // 病态偏移(小于节点尺寸)直接终止,防死循环
        if (info.NextEntryOffset as usize) < step {
            break;
        }
        off += info.NextEntryOffset as usize;
    }
    out
}

/// 两段式表查询(首次调用取所需缓冲大小,不足时按返回值重试);
/// `fill` 返回 WIN32_ERROR 码。缓冲按 u64 对齐分配:MIB 表结构体
/// 含 u32 字段(对齐 4),vec![0u8] 对齐 1 上做结构体引用形式上 UB
fn query_table(
    fill: impl Fn(Option<*mut std::ffi::c_void>, *mut u32) -> u32,
) -> Result<Vec<u64>, String> {
    const SUCCESS: u32 = ERROR_SUCCESS.0;
    const INSUFFICIENT: u32 = ERROR_INSUFFICIENT_BUFFER.0;
    let mut size = 0u32;
    for _ in 0..QUERY_RETRIES {
        let mut buf = vec![0u64; size.max(64) as usize / size_of::<u64>() + 1];
        match fill(Some(buf.as_mut_ptr().cast()), &mut size) {
            SUCCESS => return Ok(buf),
            INSUFFICIENT => continue,
            other => return Err(format!("code {other}")),
        }
    }
    Err("缓冲大小重试耗尽".to_owned())
}
