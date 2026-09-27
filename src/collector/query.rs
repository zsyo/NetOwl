//! Win32 底层查询原语:TCP/UDP owner-PID 表快照与进程映像路径。
//! 全部只读 API,普通用户权限可调用;供 windows_table 采集器使用。

use std::hash::{Hash, Hasher};
use std::net::Ipv4Addr;

use windows::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP_STATE_LAST_ACK, MIB_TCP_STATE_SYN_SENT,
    MIB_TCPTABLE_OWNER_PID, MIB_UDPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
};
use windows::Win32::Networking::WinSock::AF_INET;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::PWSTR;

use crate::model::Protocol;

/// 活动状态区间 [SYN_SENT, LAST_ACK]:LISTEN 是监听而非连接,
/// CLOSED/TIME_WAIT 已无数据交互,均不入连接列表
const STATE_ACTIVE_MIN: u32 = MIB_TCP_STATE_SYN_SENT.0 as u32;
const STATE_ACTIVE_MAX: u32 = MIB_TCP_STATE_LAST_ACK.0 as u32;
/// 表查询重试次数:两次调用之间表增长会要求更大缓冲,属正常时序
const QUERY_RETRIES: usize = 3;

/// 连接身份:四元组 + 归属进程;快照间据此识别同一连接。
/// 地址为网络字节序原始值,端口为主机序;UDP 无远端以 0 填充。
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
            local_port: row.dwLocalPort as u16,
            remote_addr: row.dwRemoteAddr,
            remote_port: row.dwRemotePort as u16,
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
            local_port: row.dwLocalPort as u16,
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

/// 两段式表查询(首次调用取所需缓冲大小,不足时按返回值重试);
/// `fill` 返回 WIN32_ERROR 码
fn query_table(
    fill: impl Fn(Option<*mut std::ffi::c_void>, *mut u32) -> u32,
) -> Result<Vec<u8>, String> {
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
