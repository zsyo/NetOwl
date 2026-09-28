//! 单实例运行:命名互斥体保证同一登录会话内唯一实例,二次启动时唤出已有窗口。
//!
//! 唤出走系统 API(AllowSetForegroundWindow 授权 + SW_RESTORE + SetForegroundWindow):
//! 主实例可能在托盘隐藏态,直接显示窗口比进程间通知更省事;主实例 app 层
//! 每帧经 IsWindowVisible 校准可见性跟踪,外部 ShowWindow 的变更同样被覆盖。
//! 互斥体用 Local\ 前缀限定会话命名空间,多用户会话互不干扰,进程退出自动释放。

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, FindWindowW, IsWindowVisible, SW_RESTORE,
    SetForegroundWindow, ShowWindow,
};
use windows::core::{HSTRING, PCWSTR};

/// 互斥体名(Local\ = 登录会话命名空间)
const MUTEX_NAME: &str = "Local\\NetOwl.SingleInstance";

/// 单实例凭证;保活至进程退出,drop 时关闭互斥体句柄
pub struct SingleInstanceGuard(HANDLE);

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// 尝试成为本会话唯一实例;Some = 成功(凭证需保活到进程退出),
/// None = 已有实例在运行(调用方应唤出已有窗口后退出)
pub fn acquire() -> Option<SingleInstanceGuard> {
    let name = HSTRING::from(MUTEX_NAME);
    let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())).ok()? };
    // CreateMutexW 在互斥体已存在时仍返回有效句柄,以 last error 区分
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return None;
    }
    Some(SingleInstanceGuard(handle))
}

/// 唤出已有实例的主窗口:标题查找(品牌名恒定,不随语言变化)。
/// 主窗口显示/恢复并抢占前台;询问弹窗同为应用窗口,若恰在询问中可能
/// 命中弹窗——弹窗本身置顶可见,误中无实际影响。
pub fn activate_existing(title: &str) -> bool {
    let title_h = HSTRING::from(title);
    let Ok(hwnd) = (unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title_h.as_ptr())) }) else {
        tracing::warn!("[SingleInstance] 未找到标题为 {title} 的已有窗口");
        return false;
    };
    unsafe {
        // 第二实例把前台权让渡给主实例(Windows 前台激活限制)
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }
    true
}

/// 按标题查找本应用主窗口 HWND(0 = 未找到,app 层跳过可见性校准)。
/// 启动早期进程内仅主窗口使用该标题,询问弹窗在其后才创建。
pub fn main_hwnd(title: &str) -> isize {
    let title_h = HSTRING::from(title);
    match unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title_h.as_ptr())) } {
        Ok(hwnd) => hwnd.0 as isize,
        Err(_) => 0,
    }
}

/// 原生查询窗口是否可见;hwnd 无效时返回 true(不因查询失败暂停刷新)
pub fn is_window_visible(hwnd: isize) -> bool {
    if hwnd == 0 {
        return true;
    }
    unsafe { IsWindowVisible(HWND(hwnd as *mut core::ffi::c_void)) }.as_bool()
}
