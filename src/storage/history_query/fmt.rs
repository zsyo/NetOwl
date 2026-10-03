//! 历史查询的本地时间与文本格式化:时区偏移、FILETIME 换算、
//! 远端前缀解析、时长/本地时间字符串。

use std::net::Ipv4Addr;

use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
use windows::Win32::System::Time::{
    FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime,
};

/// FILETIME(1601 起 100ns)与 unix 秒的基准差(秒)
const EPOCH_DELTA: u64 = 11_644_473_600;

/// 本地时区相对 UTC 的偏移秒数:用量分桶按本地日界/小时界切割,
/// 同一时刻分别取 GetLocalTime/GetSystemTime 转 FILETIME 差值即偏移;
/// 换算失败回退 0(按 UTC 分桶,仅桶标签偏移时区)
pub fn local_tz_offset_secs() -> i64 {
    unsafe {
        let utc = GetSystemTime();
        let local = GetLocalTime();
        let (a, b) = (filetime_of(&utc), filetime_of(&local));
        if a == 0 || b < a {
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

/// 本地时区本月 1 日 0 时起的 unix 秒
pub fn month_start() -> u64 {
    unsafe {
        let mut st = GetLocalTime();
        st.wDay = 1;
        st.wHour = 0;
        st.wMinute = 0;
        st.wSecond = 0;
        st.wMilliseconds = 0;
        let mut ft = FILETIME::default();
        SystemTimeToFileTime(&st, &mut ft).expect("[History] 本月起点换算失败");
        let ticks = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
        (ticks.saturating_sub(EPOCH_DELTA * 10_000_000)) / 10_000_000
    }
}

/// unix 秒 -> 本地时间 "MM-DD HH:MM:SS"
pub fn fmt_local(unix: u64) -> String {
    unsafe {
        let ticks = unix.saturating_add(EPOCH_DELTA) * 10_000_000;
        let ft = FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        let mut utc = SYSTEMTIME::default();
        FileTimeToSystemTime(&ft, &mut utc).expect("[History] 时间换算失败");
        let mut local = SYSTEMTIME::default();
        SystemTimeToTzSpecificLocalTime(None, &utc, &mut local)
            .expect("[History] 本地时间换算失败");
        format!(
            "{:02}-{:02} {:02}:{:02}:{:02}",
            local.wMonth, local.wDay, local.wHour, local.wMinute, local.wSecond
        )
    }
}
