//! ETW 事件解析与流聚合:v4/v6 两族事件按 EventID 归一为同一动作集
//! (v6 = v4 + 16 偏移,provider 消息模板确认同构,见 super 模块注释),
//! 本机侧归一后聚合进流表。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use windows::Win32::System::Diagnostics::Etw::EVENT_RECORD;

use super::{Agg, FlowKey, FlowStat, PAYLOAD_HEAD_V4, PAYLOAD_HEAD_V6};
use crate::model::Protocol;

/// 关注事件归一动作:v4/v6 两族共用
enum Ev {
    TcpSend,
    TcpRecv,
    Connect,
    Accept,
    Close,
    UdpSend,
    UdpRecv,
}

/// EventID -> (动作, 地址族 v6)。v6 系列 = v4 + 16:10/11/12/13/15 ->
/// 26/27/28/29/31,42/43 -> 58/59;14/30 retransmit 与 18/34 用户态
/// 拷贝与收发双计,不在消费集
fn classify(id: u32) -> Option<(Ev, bool)> {
    Some(match id {
        10 => (Ev::TcpSend, false),
        11 => (Ev::TcpRecv, false),
        12 => (Ev::Connect, false),
        13 => (Ev::Close, false),
        15 => (Ev::Accept, false),
        26 => (Ev::TcpSend, true),
        27 => (Ev::TcpRecv, true),
        28 => (Ev::Connect, true),
        29 => (Ev::Close, true),
        31 => (Ev::Accept, true),
        42 => (Ev::UdpSend, false),
        43 => (Ev::UdpRecv, false),
        58 => (Ev::UdpSend, true),
        59 => (Ev::UdpRecv, true),
        _ => return None,
    })
}

/// 首个 v6 事件的 payload 对拍日志(一次):提权运行的现场实证,
/// 与既定布局(见 super 模块注释)比对
static V6_LOGGED: AtomicBool = AtomicBool::new(false);

/// 解析关注事件并聚合;v4 头 20 字节、v6 头 44 字节(见模块注释)
pub(super) fn handle_event(agg: &mut Agg, r: &EVENT_RECORD) {
    let Some((ev, v6)) = classify(r.EventHeader.EventDescriptor.Id as u32) else {
        return;
    };
    let head_len = if v6 { PAYLOAD_HEAD_V6 } else { PAYLOAD_HEAD_V4 };
    // 长度防御:头长不足即丢弃(布局由 provider 模板保证,理论不可达)
    if usize::from(r.UserDataLength) < head_len || r.UserData.is_null() {
        return;
    }
    let d = unsafe { std::slice::from_raw_parts(r.UserData as *const u8, head_len) };
    let pid = u32::from_le_bytes(d[0..4].try_into().expect("长度恒定"));
    let size = u32::from_le_bytes(d[4..8].try_into().expect("长度恒定")) as u64;
    let (daddr, saddr, dport, sport) = if v6 {
        // 首次到达留一份 payload 证据(debug 档),便于现场对拍布局
        if !V6_LOGGED.swap(true, Ordering::Relaxed) {
            let hex: String = d.iter().map(|b| format!("{b:02x}")).collect();
            tracing::debug!(
                "[ETW] 首个 IPv6 事件(id {})payload {head_len} 字节: {hex}",
                r.EventHeader.EventDescriptor.Id
            );
        }
        let da: [u8; 16] = d[8..24].try_into().expect("长度恒定");
        let sa: [u8; 16] = d[24..40].try_into().expect("长度恒定");
        (
            IpAddr::V6(Ipv6Addr::from(da)),
            IpAddr::V6(Ipv6Addr::from(sa)),
            u16::from_be_bytes(d[40..42].try_into().expect("长度恒定")),
            u16::from_be_bytes(d[42..44].try_into().expect("长度恒定")),
        )
    } else {
        (
            IpAddr::V4(Ipv4Addr::new(d[8], d[9], d[10], d[11])),
            IpAddr::V4(Ipv4Addr::new(d[12], d[13], d[14], d[15])),
            u16::from_be_bytes(d[16..18].try_into().expect("长度恒定")),
            u16::from_be_bytes(d[18..20].try_into().expect("长度恒定")),
        )
    };
    match ev {
        // TCP connect(出站发起)/ accept(入站接受)/ close
        Ev::Connect => mark_initiated(agg, tcp_flow(pid, saddr, sport, daddr, dport), true),
        Ev::Accept => mark_initiated(agg, tcp_flow(pid, saddr, sport, daddr, dport), false),
        Ev::Close => finish_flow(agg, tcp_flow(pid, saddr, sport, daddr, dport)),
        // TCP send / recv:发送视角 saddr 为本机侧,接收视角 daddr 为本机侧
        Ev::TcpSend => add_bytes(agg, tcp_flow(pid, saddr, sport, daddr, dport), true, size),
        Ev::TcpRecv => add_bytes(agg, tcp_flow(pid, daddr, dport, saddr, sport), false, size),
        // UDP send / recv
        Ev::UdpSend => add_bytes(agg, udp_flow(pid, saddr, sport, daddr, dport), true, size),
        Ev::UdpRecv => add_bytes(agg, udp_flow(pid, daddr, dport, saddr, sport), false, size),
    }
}

/// 发送/发起/接受视角的四元组(saddr 为本机侧)
fn tcp_flow(
    pid: u32,
    local_ip: IpAddr,
    local_port: u16,
    remote_ip: IpAddr,
    remote_port: u16,
) -> FlowKey {
    FlowKey {
        pid,
        proto: Protocol::Tcp,
        local_ip,
        local_port,
        remote_ip,
        remote_port,
    }
}

/// 接收视角的四元组(daddr 为本机侧)
fn udp_flow(
    pid: u32,
    local_ip: IpAddr,
    local_port: u16,
    remote_ip: IpAddr,
    remote_port: u16,
) -> FlowKey {
    FlowKey {
        pid,
        proto: Protocol::Udp,
        local_ip,
        local_port,
        remote_ip,
        remote_port,
    }
}

fn mark_initiated(agg: &mut Agg, key: FlowKey, out: bool) {
    flow_slot(agg, key).initiated_out = Some(out);
}

fn add_bytes(agg: &mut Agg, key: FlowKey, up: bool, size: u64) {
    let st = flow_slot(agg, key);
    if up {
        st.up += size;
    } else {
        st.down += size;
    }
    st.last = Instant::now();
}

fn flow_slot(agg: &mut Agg, key: FlowKey) -> &mut FlowStat {
    agg.flows.entry(key).or_insert_with(|| FlowStat {
        first: Instant::now(),
        up: 0,
        down: 0,
        last: Instant::now(),
        initiated_out: None,
    })
}

fn finish_flow(agg: &mut Agg, key: FlowKey) {
    if let Some(st) = agg.flows.remove(&key) {
        agg.finished.push((key, st));
    }
}
