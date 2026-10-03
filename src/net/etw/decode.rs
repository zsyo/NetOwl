//! ETW 事件解析与流聚合:按 EventID 分派,payload 前 20 字节同构解析
//! (见 super 模块注释),本机侧归一后聚合进流表。

use std::net::Ipv4Addr;
use std::time::Instant;

use windows::Win32::System::Diagnostics::Etw::EVENT_RECORD;

use super::{Agg, FlowKey, FlowStat, PAYLOAD_HEAD};
use crate::model::Protocol;

/// 解析关注事件并聚合;payload 前 20 字节同构(见模块注释)
pub(super) fn handle_event(agg: &mut Agg, r: &EVENT_RECORD) {
    let id = r.EventHeader.EventDescriptor.Id as u32;
    let d = unsafe { std::slice::from_raw_parts(r.UserData as *const u8, PAYLOAD_HEAD) };
    let pid = u32::from_le_bytes(d[0..4].try_into().expect("长度恒定"));
    let size = u32::from_le_bytes(d[4..8].try_into().expect("长度恒定")) as u64;
    let daddr = Ipv4Addr::new(d[8], d[9], d[10], d[11]);
    let saddr = Ipv4Addr::new(d[12], d[13], d[14], d[15]);
    let dport = u16::from_be_bytes(d[16..18].try_into().expect("长度恒定"));
    let sport = u16::from_be_bytes(d[18..20].try_into().expect("长度恒定"));
    match id {
        // TCP connect(出站发起)/ accept(入站接受)/ close
        12 => mark_initiated(agg, tcp_flow(pid, saddr, sport, daddr, dport), true),
        15 => mark_initiated(agg, tcp_flow(pid, saddr, sport, daddr, dport), false),
        13 => finish_flow(agg, tcp_flow(pid, saddr, sport, daddr, dport)),
        // TCP send / recv:发送视角 saddr 为本机侧,接收视角 daddr 为本机侧
        10 => add_bytes(agg, tcp_flow(pid, saddr, sport, daddr, dport), true, size),
        11 => add_bytes(agg, tcp_flow(pid, daddr, dport, saddr, sport), false, size),
        // UDP send / recv
        42 => add_bytes(agg, udp_flow(pid, saddr, sport, daddr, dport), true, size),
        43 => add_bytes(agg, udp_flow(pid, daddr, dport, saddr, sport), false, size),
        // 其余事件(retransmit/用户态拷贝/IPv6 系列)不消费
        _ => {}
    }
}

/// 发送/发起/接受视角的四元组(saddr 为本机侧)
fn tcp_flow(
    pid: u32,
    local_ip: Ipv4Addr,
    local_port: u16,
    remote_ip: Ipv4Addr,
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
    local_ip: Ipv4Addr,
    local_port: u16,
    remote_ip: Ipv4Addr,
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
