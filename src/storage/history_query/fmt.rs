//! 历史查询的本地时间与文本格式化:时区偏移、FILETIME 换算、
//! 远端前缀解析、时长/本地时间字符串。

use std::net::Ipv4Addr;

use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
use windows::Win32::System::Time::{
    FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime,
    TzSpecificLocalTimeToSystemTime,
};

/// FILETIME(1601 起 100ns)与 unix 秒的基准差(秒)
const EPOCH_DELTA: u64 = 11_644_473_600;

/// 本地时区相对 UTC 的偏移秒数:用量分桶按本地日界/小时界切割。
/// 一次取 UTC 经 SystemTimeToTzSpecificLocalTime(None = 当前时区)换算
/// 出本地,与 GetLocalTime 两次独立采样会跨秒边界产生 ±1s 偏差值
/// (如 28801);换算失败回退 0(按 UTC 分桶,仅桶标签偏移时区)。
/// 偏移可为负(UTC 以西):local < utc 是正常取值,不是换算失败
pub fn local_tz_offset_secs() -> i64 {
    unsafe {
        let utc = GetSystemTime();
        let mut local = SYSTEMTIME::default();
        if SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err() {
            return 0;
        }
        let (a, b) = (filetime_of(&utc), filetime_of(&local));
        if a == 0 || b == 0 {
            return 0;
        }
        (b - a) / 10_000_000
    }
}

/// SYSTEMTIME -> FILETIME tick 数(1601 起 100ns);失败返回 0
fn filetime_of(st: &SYSTEMTIME) -> i64 {
    let mut ft = FILETIME::default();
    if unsafe { SystemTimeToFileTime(st, &mut ft) }.is_ok() {
        (ft.dwHighDateTime as i64) << 32 | ft.dwLowDateTime as i64
    } else {
        0
    }
}

/// 远端前缀解析:"142.250." / "142.250.73.78" -> 网段范围(前缀补零);
/// 空串或非法输入返回 None(视为不过滤)
pub fn parse_ip_prefix(input: &str) -> Option<(u32, u32)> {
    let s = input.trim().trim_end_matches('.');
    if s.is_empty() {
        return None;
    }
    let mut octets = [0u8; 4];
    let mut filled = 0;
    for part in s.split('.') {
        let v: u8 = part.trim().parse().ok()?;
        // 先判段数再写入:5 段输入(如 1.2.3.4.5)直接拒绝,不能先下标
        if filled >= 4 {
            return None;
        }
        octets[filled] = v;
        filled += 1;
    }
    let min = u32::from(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]));
    let max = if filled == 4 {
        min
    } else {
        min | ((1u32 << ((4 - filled) * 8)) - 1)
    };
    Some((min, max))
}

/// unix 秒时长 -> "H:MM:SS" / "M:SS"
pub fn fmt_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// 本地时区本月 1 日 0 时起的 unix 秒。换算失败(SYSTEMTIME 字段非法,
/// 正常路径不会发生)退化为 0 即不设下界并告警:调用方在 egui 主线程,
/// 一次 expect 崩溃会拖垮整个应用(与 fmt_local 同口径)
pub fn month_start() -> u64 {
    unsafe {
        let mut st = GetLocalTime();
        st.wDay = 1;
        st.wHour = 0;
        st.wMinute = 0;
        st.wSecond = 0;
        st.wMilliseconds = 0;
        day_start_from(&st, "本月起点")
    }
}

/// 本地时区当日 0 时起的 unix 秒(今日总量口径);失败口径同 month_start
pub fn day_start() -> u64 {
    unsafe {
        let mut st = GetLocalTime();
        st.wHour = 0;
        st.wMinute = 0;
        st.wSecond = 0;
        st.wMilliseconds = 0;
        day_start_from(&st, "当日起点")
    }
}

/// SYSTEMTIME(已归零到日界的本地挂钟)-> unix 秒。
/// SystemTimeToFileTime 把输入按 UTC 解释(MSDN),本地挂钟必须先经
/// TzSpecificLocalTimeToSystemTime 转成 UTC:直接换算会多出一个时区
/// 偏移(UTC+8 下今日/本月窗口从本地 08:00 起算,漏掉凌晨流量);
/// 用逆转换而非减当前偏移,DST 切换日也不会差一小时
fn day_start_from(st: &SYSTEMTIME, what: &str) -> u64 {
    let mut utc = SYSTEMTIME::default();
    if unsafe { TzSpecificLocalTimeToSystemTime(None, st, &mut utc) }.is_err() {
        tracing::warn!("[History] {what}本地时间转 UTC 失败,退化为不设下界");
        return 0;
    }
    let ticks = filetime_of(&utc);
    if ticks == 0 {
        tracing::warn!("[History] {what}换算失败,退化为不设下界");
        return 0;
    }
    ((ticks as u64).saturating_sub(EPOCH_DELTA * 10_000_000)) / 10_000_000
}

/// unix 秒 -> 本地时间 "MM-DD HH:MM:SS"。
/// 库中时间戳损坏(负数/极大值)时换算失败不回退 panic:UI 线程一次
/// expect 崩溃会拖垮整个应用,降级为占位文本,行数据照常展示
pub fn fmt_local(unix: u64) -> String {
    unsafe {
        let Some(ticks) = unix.saturating_add(EPOCH_DELTA).checked_mul(10_000_000) else {
            return "--".to_owned();
        };
        let ft = FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        let mut utc = SYSTEMTIME::default();
        if FileTimeToSystemTime(&ft, &mut utc).is_err() {
            return "--".to_owned();
        }
        let mut local = SYSTEMTIME::default();
        if SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err() {
            return "--".to_owned();
        }
        format!(
            "{:02}-{:02} {:02}:{:02}:{:02}",
            local.wMonth, local.wDay, local.wHour, local.wMinute, local.wSecond
        )
    }
}
