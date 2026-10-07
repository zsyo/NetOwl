//! 检查更新:GitHub Releases 查询与语义化版本比较。
//!
//! HTTPS 走 WinINet(跟随系统/IE 代理设置,免引 TLS 依赖),阻塞调用,
//! 须在工作线程执行;结果经 mpsc 回主线程收割。

use serde::Deserialize;
use windows::Win32::Foundation::GetLastError;
use windows::Win32::Networking::WinInet::{
    HTTP_QUERY_FLAG_NUMBER, HTTP_QUERY_STATUS_CODE, HttpOpenRequestW, HttpQueryInfoW,
    HttpSendRequestW, INTERNET_FLAG_NO_CACHE_WRITE, INTERNET_FLAG_RELOAD, INTERNET_FLAG_SECURE,
    INTERNET_OPEN_TYPE_PRECONFIG, INTERNET_OPTION_CONNECT_TIMEOUT, INTERNET_OPTION_RECEIVE_TIMEOUT,
    INTERNET_OPTION_SEND_TIMEOUT, INTERNET_SERVICE_HTTP, InternetCloseHandle, InternetConnectW,
    InternetOpenW, InternetReadFile, InternetSetOptionW,
};
use windows::core::{HSTRING, PCWSTR};

/// 仓库发布页 API(检查更新数据源)
const API_HOST: &str = "api.github.com";
/// HTTPS 默认端口(windows crate 未导出 WinInet 的该常量)
const HTTPS_PORT: u16 = 443;
/// 正式渠道:仅最新正式版(不含 pre-release)
const LATEST_PATH: &str = "/repos/zsyo/NetOwl/releases/latest";
/// 预览渠道:发布列表首项(含 pre-release;最新为正式版时同样覆盖)
const LIST_PATH: &str = "/repos/zsyo/NetOwl/releases?per_page=1";

/// 当前进程版本号(Cargo.toml version)
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 一条发布信息(检查更新结果)
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    /// 版本 tag(如 "v0.1.0" / "v0.2.0-beta.1")
    pub tag_name: String,
    /// 是否预发布(预览渠道返回正式版时为 false,供 UI 展示)
    pub prerelease: bool,
    /// release 详情页(前往下载)
    pub html_url: String,
}

/// 查询最新发布:正式渠道取 releases/latest,预览渠道取列表首项;
/// Ok(None) = 仓库尚无匹配发布(404 / 空列表)
pub fn fetch_latest(preview_channel: bool) -> Result<Option<ReleaseInfo>, String> {
    let body = match https_get(
        API_HOST,
        if preview_channel {
            LIST_PATH
        } else {
            LATEST_PATH
        },
    )? {
        Some(b) => b,
        None => return Ok(None),
    };
    parse_release(&body)
}

/// 解析 releases API 响应:预览渠道返回数组取首项,正式渠道返回单个对象
fn parse_release(body: &str) -> Result<Option<ReleaseInfo>, String> {
    #[derive(Deserialize)]
    struct Rel {
        #[serde(rename = "tag_name")]
        tag: String,
        prerelease: bool,
        #[serde(rename = "html_url")]
        url: String,
    }
    let from_rel = |r: Rel| ReleaseInfo {
        tag_name: r.tag,
        prerelease: r.prerelease,
        html_url: r.url,
    };
    if let Ok(list) = serde_json::from_str::<Vec<Rel>>(body) {
        return Ok(list.into_iter().next().map(from_rel));
    }
    serde_json::from_str::<Rel>(body)
        .map(|r| Some(from_rel(r)))
        .map_err(|e| format!("JSON 解析失败: {e}"))
}

/// HTTPS GET(WinINet,代理跟随系统设置);404 返回 Ok(None)
fn https_get(host: &str, path: &str) -> Result<Option<String>, String> {
    unsafe {
        let agent = HSTRING::from(format!("NetOwl/{CURRENT_VERSION}"));
        let open = HandleGuard::new(
            InternetOpenW(
                &agent,
                INTERNET_OPEN_TYPE_PRECONFIG.0,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ),
            "InternetOpen",
        )?;
        let connect = HandleGuard::new(
            InternetConnectW(
                open.0,
                &HSTRING::from(host),
                HTTPS_PORT,
                PCWSTR::null(),
                PCWSTR::null(),
                INTERNET_SERVICE_HTTP,
                0,
                Some(0),
            ),
            "InternetConnect",
        )?;
        let request = HandleGuard::new(
            HttpOpenRequestW(
                connect.0,
                &HSTRING::from("GET"),
                &HSTRING::from(path),
                PCWSTR::null(),
                PCWSTR::null(),
                None,
                INTERNET_FLAG_SECURE | INTERNET_FLAG_RELOAD | INTERNET_FLAG_NO_CACHE_WRITE,
                Some(0),
            ),
            "HttpOpenRequest",
        )?;
        // 默认超时可达 20s+,手动触发的按钮收窄到 10s 减少卡顿感
        for (opt, ms) in [
            (INTERNET_OPTION_CONNECT_TIMEOUT, 10_000u32),
            (INTERNET_OPTION_SEND_TIMEOUT, 10_000),
            (INTERNET_OPTION_RECEIVE_TIMEOUT, 10_000),
        ] {
            let bytes = ms.to_ne_bytes();
            let _ = InternetSetOptionW(
                Some(request.0),
                opt,
                Some(bytes.as_ptr() as *const _),
                bytes.len() as u32,
            );
        }
        HttpSendRequestW(request.0, None, None, 0).map_err(|e| format!("HttpSendRequest: {e}"))?;
        let mut status: u32 = 0;
        let mut len = std::mem::size_of_val(&status) as u32;
        HttpQueryInfoW(
            request.0,
            HTTP_QUERY_STATUS_CODE | HTTP_QUERY_FLAG_NUMBER,
            Some(&mut status as *mut u32 as *mut _),
            &mut len,
            None,
        )
        .map_err(|e| format!("HttpQueryInfo: {e}"))?;
        match status {
            200 => {}
            404 => return Ok(None),
            other => return Err(format!("HTTP {other}")),
        }
        let mut body = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let mut read = 0u32;
            InternetReadFile(
                request.0,
                buf.as_mut_ptr() as *mut _,
                buf.len() as u32,
                &mut read,
            )
            .map_err(|e| format!("InternetReadFile: {e}"))?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&buf[..read as usize]);
        }
        String::from_utf8(body)
            .map_err(|e| format!("响应非 UTF-8: {e}"))
            .map(Some)
    }
}

/// WinINet 句柄 RAII:null 返回即报错,已建句柄随 Drop 释放
struct HandleGuard(*mut core::ffi::c_void);
impl HandleGuard {
    /// 判空接管句柄:WinINet 裸指针 API 以 null 表示失败
    fn new(ptr: *mut core::ffi::c_void, what: &str) -> Result<Self, String> {
        if ptr.is_null() {
            return Err(format!("{what}: WinINet 错误 {}", unsafe {
                GetLastError().0
            }));
        }
        Ok(Self(ptr))
    }
}
impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = InternetCloseHandle(self.0);
        }
    }
}

/// 语义化版本比较:`remote` 是否新于 `current`。tag 允许 "v" 前缀与
/// build metadata;pre-release 低于正式版(v0.2.0-beta.1 < v0.2.0)。
/// 任一侧解析失败返回 false(不凭猜测提示更新)
pub fn is_newer(remote: &str, current: &str) -> bool {
    use std::cmp::Ordering;
    let (Some(r), Some(c)) = (parse_ver(remote), parse_ver(current)) else {
        return false;
    };
    match r.0.cmp(&c.0).then(r.1.cmp(&c.1)).then(r.2.cmp(&c.2)) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => match (&r.3, &c.3) {
            (None, None) => false,    // 同一正式版
            (None, Some(_)) => true,  // 正式版 > 预发布
            (Some(_), None) => false, // 预发布 < 正式版
            (Some(a), Some(b)) => pre_cmp(a, b) == Ordering::Greater,
        },
    }
}

/// 解析版本 tag 为 (major, minor, patch, pre-release)
fn parse_ver(s: &str) -> Option<(u64, u64, u64, Option<String>)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let s = s.split('+').next()?; // build metadata 不参与比较
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, Some(p.to_owned())),
        None => (s, None),
    };
    let mut it = core.split('.');
    Some((
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        pre,
    ))
}

/// pre-release 标识符比较(点分段):数字段按数值且低于字母段,前缀
/// 相等时段少者小(与 semver 规则一致)
fn pre_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut ai = a.split('.');
    let mut bi = b.split('.');
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                if x == y {
                    continue;
                }
                return match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(vx), Ok(vy)) => vx.cmp(&vy),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => x.cmp(y),
                };
            }
        }
    }
}

/// 用系统默认浏览器打开 URL(ShellExecuteW 立即返回);
/// release 详情页、仓库首页与新建 Issue 页共用
pub fn open_url(url: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    unsafe {
        ShellExecuteW(
            None,
            PCWSTR::null(),
            &HSTRING::from(url),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}
