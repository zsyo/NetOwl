//! 局域网设备发现:GetIpNetTable(IPv4 ARP 缓存)只读轮询,无需管理员
//! 权限。表项是系统级 ARP 缓存(最近与本机通信过的邻居),作为"在线"
//! 口径;条目会老化消失,历史与首见时间由 lan_devices 表持久化(app 层
//! 编排,本模块只做查询原语与视图行模型)。

use std::net::Ipv4Addr;

use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
use windows::Win32::NetworkManagement::IpHelper::{GetIpNetTable, MIB_IPNETTABLE};

/// LAN 页设备行(库记录 + 本轮在线状态合并)
pub struct DeviceRow {
    /// 规范化 MAC("aa:bb:cc:dd:ee:ff" 小写冒号串,表主键)
    pub mac: String,
    pub ip: Ipv4Addr,
    pub first_seen: u64,
    pub last_seen: u64,
    /// 本轮 ARP 表可见(最近有过局域网通信)
    pub online: bool,
    /// 新设备:首见不足 24 小时
    pub is_new: bool,
}

/// ARP 表项:(IP, 规范化 MAC)
pub type ArpEntry = (Ipv4Addr, String);

/// MIB_IPNET_TYPE_INVALID:表项无效
const TYPE_INVALID: u32 = 2;

/// 查询本机 IPv4 ARP 缓存;排除无效表项与组播/广播 MAC(首字节 I/G 位
/// 为 1:ff 广播、01:00:5e IPv4 组播、33:33 IPv6 组播等)。查询失败返回
/// 错误描述
pub fn query_arp() -> Result<Vec<ArpEntry>, String> {
    unsafe {
        // 两段式:首次 NULL 调用取所需缓冲大小
        let mut size = 0u32;
        let probe = GetIpNetTable(None, &mut size, false);
        if probe != ERROR_INSUFFICIENT_BUFFER.0 && probe != ERROR_SUCCESS.0 {
            return Err(format!("GetIpNetTable 探测失败: code {probe:#x}"));
        }
        let mut buf = vec![0u64; size as usize / 8 + 1];
        let table = buf.as_mut_ptr() as *mut MIB_IPNETTABLE;
        let err = GetIpNetTable(Some(table), &mut size, false);
        if err != ERROR_SUCCESS.0 {
            return Err(format!("GetIpNetTable 失败: code {err:#x}"));
        }
        let rows =
            std::slice::from_raw_parts((*table).table.as_ptr(), (*table).dwNumEntries as usize);
        let mut out = Vec::new();
        for row in rows {
            // dwType 与 Type 是 union 两视图;按 MIB_IPNET_TYPE_INVALID(2)过滤
            if row.Anonymous.dwType == TYPE_INVALID
                || row.dwPhysAddrLen == 0
                || row.bPhysAddr[0] & 1 == 1
            {
                continue;
            }
            let len = row.dwPhysAddrLen as usize;
            let mac = row.bPhysAddr[..len]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(":");
            // dwAddr 与连接表同惯例存网络序原始值,from_be 还原主机序
            out.push((Ipv4Addr::from(u32::from_be(row.dwAddr)), mac));
        }
        Ok(out)
    }
}
