//! 总上传/下载速率:GetIfTable2 接口字节计数采样差值,导航栏展示实时总速率。
//!
//! 对全部接口的 In/OutOctets 求和(排除回环与隧道:回环无外部语义,隧道在
//! 代理类虚拟网卡场景会与物理网卡重复计数)。采样为本地内核查询,开销极小,
//! 直接在 UI 线程按节流间隔执行;可见/隐藏(托盘)均按秒采样,托盘悬停
//! 提示跟随刷新(AGENTS.md 功耗要求不适用于秒级内核查询)。

use std::time::{Duration, Instant};

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};

/// IANA ifType:不计入总速率的接口类型
/// (软件回环 24 无外部流量;隧道 131 场景下物理网卡已有同份流量)
const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;
const IF_TYPE_TUNNEL: u32 = 131;
/// InterfaceAndOperStatusFlags.FilterInterface(bit1):WFP 轻量过滤/QoS
/// 等过滤接口会与底层物理接口重复报告同一份字节计数,必须排除,
/// 否则总速率按过滤链数量成倍虚高
const IF_FLAG_FILTER_INTERFACE: u8 = 0x02;
/// 基线间隔上限:超过该间隔(读表连续失败后恢复)只重建基线不计算速率
const MAX_BASELINE_GAP_SECS: f32 = 10.0;

/// 总速率采样器:poll() 按传入间隔节流,返回最近一次采样差值(字节/秒)
pub struct Sampler {
    /// 上次采样:(下行字节和, 上行字节和, 时刻);None 表示仅建立基线
    last: Option<(u64, u64, Instant)>,
    /// 最近速率(字节/秒):(下行, 上行)
    rates: (u64, u64),
    /// 读表失败已上报:失败期间每次采样都失败,只报首次,恢复时复位
    read_failed: bool,
}

impl Sampler {
    pub fn new() -> Self {
        Sampler {
            last: None,
            rates: (0, 0),
            read_failed: false,
        }
    }

    /// 距上次采样达到 interval 才重新读表;返回 None 表示本帧未产生新采样
    /// (节流沿用旧值/查询失败/首次仅建基线),调用方不应把它计入历史序列;
    /// 表查询失败时保留旧速率(导航栏速率短暂停更,不中断界面),但清掉
    /// 基线——否则恢复后第一帧的 dt 是"上次成功采样到现在"的超长间隔,
    /// 速率被严重低估,且这个错误值会进 rate_hist 持续一整轮
    pub fn poll(&mut self, interval: Duration) -> Option<(u64, u64)> {
        if self.last.is_some_and(|(_, _, at)| at.elapsed() < interval) {
            return None;
        }
        let now = Instant::now();
        let Some((in_octets, out_octets)) = interface_octets() else {
            if !self.read_failed {
                self.read_failed = true;
                tracing::debug!("[Traffic] GetIfTable2 读表失败,总速率沿用旧值(基线重建)");
            }
            self.last = None;
            return None;
        };
        self.read_failed = false;
        if let Some((last_in, last_out, at)) = self.last.replace((in_octets, out_octets, now)) {
            let dt = now.duration_since(at).as_secs_f32();
            // 间隔异常长(读表连续失败后恢复)同样只重建基线:差值除以
            // 巨大 dt 得到的低速率没有意义
            if dt > 0.0 && dt <= MAX_BASELINE_GAP_SECS {
                let down = in_octets.saturating_sub(last_in);
                let up = out_octets.saturating_sub(last_out);
                self.rates = ((down as f32 / dt) as u64, (up as f32 / dt) as u64);
                return Some(self.rates);
            }
        }
        None
    }
}

/// 全部接口的收发字节和(排除回环与隧道);查询失败返回 None
fn interface_octets() -> Option<(u64, u64)> {
    unsafe {
        let mut table = std::ptr::null_mut::<MIB_IF_TABLE2>();
        if GetIfTable2(&mut table) != ERROR_SUCCESS {
            return None;
        }
        let (mut total_in, mut total_out) = (0u64, 0u64);
        // ANY_SIZE 惯用法:行数组容量由 NumEntries 给出
        let rows =
            std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        for row in rows {
            if row.Type == IF_TYPE_SOFTWARE_LOOPBACK
                || row.Type == IF_TYPE_TUNNEL
                || row.InterfaceAndOperStatusFlags._bitfield & IF_FLAG_FILTER_INTERFACE != 0
            {
                continue;
            }
            total_in += row.InOctets;
            total_out += row.OutOctets;
        }
        FreeMibTable(table.cast());
        Some((total_in, total_out))
    }
}
