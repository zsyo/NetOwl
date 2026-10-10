//! 全局热键唤出主窗:后台线程自建消息窗口(HWND_MESSAGE,不占任务栏与
//! ALT+TAB 序)注册热键,收到 WM_HOTKEY 经托盘命令通道发送 CMD_SHOW——
//! 与托盘菜单"显示主窗口"同一处理路径(恢复可见/解除最小化/聚焦)。
//!
//! 组合可配置(预设下拉,存 config):线程阻塞在 GetMessageW,App 层经
//! PostThreadMessageW 唤醒后比对期望组合,热注销重注册;注册失败
//! (组合被其它程序占用)经回报通道交 App 弹 toast——冲突从静默死亡
//! 变为可见可改。

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetMessageW, HWND_MESSAGE, MSG, PostThreadMessageW,
    RegisterClassW, WM_APP, WM_HOTKEY, WNDCLASSW,
};

/// 与托盘命令通道共用的命令字(见 platform::tray::CMD_SHOW)
const CMD_SHOW: &str = "show";
/// 热键标识(任意;本进程只注册一个)
const HOTKEY_ID: i32 = 1;
/// 唤醒热键线程重检查期望组合的线程消息(PostThreadMessageW)
const MSG_RECHECK: u32 = WM_APP;
/// 消息窗口类名(全局命名空间,避免与他人类名冲突)
const CLASS_NAME: &str = "NetOwlHotkeyMsgWnd";
/// ERROR_HOTKEY_ALREADY_REGISTERED(Win32 稳定值):组合已被占用。
/// App 侧据此给出可读失败原因(见 HotkeyReport::Failed 的 code)
pub const ERROR_HOTKEY_TAKEN: u32 = 1409;

/// 组合串转可读形式("ctrl+alt+n" -> "Ctrl+Alt+N")
pub fn display(combo: &str) -> String {
    combo
        .split('+')
        .map(|part| {
            let mut chars = part.trim().chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// 解析组合串(大小写不敏感):修饰键 ctrl/alt/shift/win + 主键(单个
/// 字母/数字或 F1..F12);恒带 MOD_NOREPEAT(防长按连续触发)。
/// 无法识别、无主键或多个主键返回 None
fn parse_combo(combo: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let mut modifiers = MOD_NOREPEAT;
    let mut vk = None;
    for part in combo.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        let lower = part.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" => modifiers |= MOD_CONTROL,
            "alt" => modifiers |= MOD_ALT,
            "shift" => modifiers |= MOD_SHIFT,
            "win" => modifiers |= MOD_WIN,
            _ => {
                let v = key_vk(&lower)?;
                if vk.replace(v).is_some() {
                    return None; // 只允许一个主键
                }
            }
        }
    }
    vk.map(|v| (modifiers, v))
}

/// 主键名 -> 虚拟键码:单个字母/数字(ASCII 大写值即 VK)或 F1..F12
/// (VK_F1 = 0x70 起)。其余键不支持(全局热键的实用面外)
fn key_vk(name: &str) -> Option<u32> {
    if name.len() == 1 {
        let c = name.as_bytes()[0];
        return c
            .is_ascii_alphanumeric()
            .then(|| u32::from(c.to_ascii_uppercase()));
    }
    let f: u32 = name.strip_prefix('f')?.parse().ok()?;
    (1..=12).contains(&f).then_some(0x70 + f - 1)
}

/// 注册结果回报(热键线程 -> App 层)
pub enum HotkeyReport {
    /// 注册成功(当前生效组合)
    Ok(String),
    /// 注册失败(组合, Win32 错误码;1409 = 组合已被占用)
    Failed { combo: String, code: u32 },
    /// 已关闭(期望组合为空)
    Off,
}

/// 全局热键句柄(App 持有):写期望组合并唤醒线程,线程内热注销重注册
pub struct HotkeyHandle {
    /// 期望组合(空 = 关闭);App 写、热键线程读
    desired: Arc<Mutex<String>>,
    /// 热键线程 id(唤醒用;0 = 线程尚未就绪)
    thread_id: Arc<AtomicU32>,
}

impl HotkeyHandle {
    /// 设置期望组合(立即热切换,不必等待重启)
    pub fn set(&self, combo: &str) {
        *self.desired.lock().unwrap_or_else(|e| e.into_inner()) = combo.to_owned();
        let tid = self.thread_id.load(Ordering::Acquire);
        if tid != 0 {
            // 唤醒阻塞在 GetMessageW 的线程;线程尚未存储 id 时无需唤醒
            // (首轮循环即读取 desired)
            unsafe {
                let _ = PostThreadMessageW(tid, MSG_RECHECK, WPARAM(0), LPARAM(0));
            }
        }
    }
}

/// 启动全局热键后台线程;`combo` 为初始组合(config),`tray_tx` 为托盘
/// 命令通道发送端,`report_tx` 为注册结果回报发送端(接收端由调用方持有)
pub fn spawn(
    tray_tx: Sender<String>,
    report_tx: Sender<HotkeyReport>,
    combo: &str,
) -> HotkeyHandle {
    let desired = Arc::new(Mutex::new(combo.to_owned()));
    let thread_id = Arc::new(AtomicU32::new(0));
    let handle = HotkeyHandle {
        desired: Arc::clone(&desired),
        thread_id: Arc::clone(&thread_id),
    };
    std::thread::spawn(move || {
        if let Err(e) = run(&tray_tx, &report_tx, &desired, &thread_id) {
            tracing::warn!("[Hotkey] 全局热键线程退出: {e}");
        }
    });
    handle
}

fn run(
    tray_tx: &Sender<String>,
    report_tx: &Sender<HotkeyReport>,
    desired: &Mutex<String>,
    thread_id: &AtomicU32,
) -> windows::core::Result<()> {
    unsafe {
        thread_id.store(GetCurrentThreadId(), Ordering::Release);
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
        // 消息窗口:有消息队列但不显示、不收广播之外的窗口消息
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
        // 当前已注册组合(空 = 未注册);期望变化时热切换
        let mut current = String::new();
        loop {
            let want = desired.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if want != current {
                if !current.is_empty() {
                    let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID);
                }
                current.clear();
                if want.is_empty() {
                    tracing::info!("[Hotkey] 全局热键已关闭");
                    let _ = report_tx.send(HotkeyReport::Off);
                } else if let Some((modifiers, vk)) = parse_combo(&want) {
                    match RegisterHotKey(Some(hwnd), HOTKEY_ID, modifiers, vk) {
                        Ok(()) => {
                            current = want.clone();
                            tracing::info!("[Hotkey] 全局热键 {} 已注册(唤出主窗)", display(&want));
                            let _ = report_tx.send(HotkeyReport::Ok(current.clone()));
                        }
                        Err(e) => {
                            tracing::warn!("[Hotkey] 全局热键 {} 注册失败: {e}", display(&want));
                            let _ = report_tx.send(HotkeyReport::Failed {
                                combo: want,
                                code: e.code().0 as u32,
                            });
                        }
                    }
                } else {
                    tracing::warn!("[Hotkey] 无法识别的组合串:{want}");
                    let _ = report_tx.send(HotkeyReport::Failed {
                        combo: want,
                        code: 0,
                    });
                }
            }
            let mut msg = MSG::default();
            let r = GetMessageW(&mut msg, None, 0, 0);
            // 0 = WM_QUIT,-1 = 错误:两者都退出(BOOL 的 as_bool 对 -1 为真,
            // 不能只判 as_bool,否则错误时原地空转)
            if r.0 == 0 || r.0 == -1 {
                break;
            }
            if msg.message == WM_HOTKEY && msg.wParam.0 as i32 == HOTKEY_ID {
                let _ = tray_tx.send(CMD_SHOW.to_owned());
            }
            // MSG_RECHECK 无处理体:回到循环顶部即重新对齐期望组合
        }
        if !current.is_empty() {
            let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID);
        }
        Ok(())
    }
}

/// 消息窗口过程:全部透传默认处理(本窗口只消费 RegisterHotKey 投递的
/// WM_HOTKEY,无需自定义行为)
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
