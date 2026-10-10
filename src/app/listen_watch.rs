//! 新监听端口提醒:监听快照(TCP LISTEN + UDP 绑定)按 (进程, 协议,
//! 绑定地址, 端口) 做会话内基线 diff——启动首轮基线不计,之后新出现
//! 的监听端口返回供通知层弹 toast。恶意程序开端口即时可见(安全监控),
//! 与新设备接入提醒(lan)同一模式。

use std::collections::HashSet;
use std::net::Ipv4Addr;

use crate::model::{ListenEntry, Protocol};

/// 监听身份键:(PID, 协议, 绑定地址, 端口)。含 PID:同程序多实例/
/// 端口复用不相互掩盖
type ListenKey = (u32, Protocol, Ipv4Addr, u16);

/// 监听观察状态(App 持有)
pub struct ListenWatch {
    /// 会话内出现过的监听键。只增不替换:端口关闭再开重通知没有意义,
    /// 且键空间 = 进程数 × 端口数,会话内有界
    known: HashSet<ListenKey>,
    /// 启动基线是否已完成:只在第一个非空监听快照做一次(首轮 TCP 表
    /// 查询失败时快照为空,顺延到下一个非空快照)
    baselined: bool,
}

impl ListenWatch {
    pub fn new() -> Self {
        ListenWatch {
            known: HashSet::new(),
            baselined: false,
        }
    }

    /// 喂入本轮监听快照,返回基线之后新出现的监听条目
    pub fn observe<'a>(&mut self, listens: &'a [ListenEntry]) -> Vec<&'a ListenEntry> {
        if !self.baselined {
            if listens.is_empty() {
                return Vec::new();
            }
            self.baselined = true;
            self.known.extend(
                listens
                    .iter()
                    .map(|l| (l.pid, l.proto, l.local_addr, l.local_port)),
            );
            return Vec::new();
        }
        let mut fresh = Vec::new();
        for l in listens {
            if self
                .known
                .insert((l.pid, l.proto, l.local_addr, l.local_port))
            {
                fresh.push(l);
            }
        }
        fresh
    }
}
