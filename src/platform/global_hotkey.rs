//! 全局热键唤出主窗:Ctrl+Alt+N。后台线程自建消息窗口(HWND_MESSAGE,
//! 不占任务栏与 ALT+TAB 序)注册热键,收到 WM_HOTKEY 经托盘命令通道
//! 发送 CMD_SHOW——与托盘菜单"显示主窗口"同一处理路径(恢复可见/
//! 解除最小化/聚焦)。注册失败(组合键被其它程序占用)仅告警降级,
//! 不影响其余功能。

use std::sync::mpsc::Sender;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey, VK_N,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetMessageW, HWND_MESSAGE, MSG, RegisterClassW, WM_HOTKEY,
    WNDCLASSW,
};

/// 与托盘命令通道共用的命令字(见 platform::tray::CMD_SHOW)
const CMD_SHOW: &str = "show";
/// 热键标识(任意;本进程只注册一个)
const HOTKEY_ID: i32 = 1;
/// 消息窗口类名(全局命名空间,避免与他人类名冲突)
const CLASS_NAME: &str = "NetOwlHotkeyMsgWnd";

/// 在后台线程注册全局热键并循环处理消息;`tx` 为托盘命令通道发送端
pub fn spawn(tx: Sender<String>) {
    std::thread::spawn(move || {
        if let Err(e) = run(&tx) {
            tracing::warn!("[Hotkey] 全局热键不可用(不影响其它功能): {e}");
        }
    });
}

fn run(tx: &Sender<String>) -> windows::core::Result<()> {
    unsafe {
        let hinstance = GetModuleHandleW(None)?;
        let class_name: Vec<u16> = CLASS_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let class = windows::core::PCWSTR(class_name.as_ptr());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
        // 消息窗口:有消息队列但不显示、不接收广播之外的窗口消息之外的一切
        let hwnd = CreateWindowExW(
            Default::default(),
            class,
            class,
            Default::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinstance.into()),
            None,
        )?;
        // Ctrl+Alt+N;NOREPEAT 防长按连续触发。注册失败(组合键被占用)
        // 向上传递,由 spawn 记录告警后降级
        let modifiers = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
        RegisterHotKey(Some(hwnd), HOTKEY_ID, modifiers, u32::from(VK_N.0))?;
        tracing::info!("[Hotkey] 全局热键 Ctrl+Alt+N 已注册(唤出主窗)");
        let mut msg = MSG::default();
        // 返回 false(WM_QUIT 或错误)时退出循环;进程退出时线程随进程终止
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_HOTKEY && msg.wParam.0 as i32 == HOTKEY_ID {
                let _ = tx.send(CMD_SHOW.to_owned());
            }
        }
        let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID);
        Ok(())
    }
}

/// 消息窗口过程:全部透传默认处理(本窗口只消费 RegisterHotKey 投递的
/// WM_HOTKEY,无需自定义行为)
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
