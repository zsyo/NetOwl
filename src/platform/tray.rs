//! 托盘与菜单:tray-icon + muda(经 tray_icon::menu re-export)。
//!
//! 托盘在 eframe 主线程创建(见 AGENTS.md 平台规范),事件经
//! set_event_handler 转发到 mpsc 通道,并 request_repaint 唤醒隐藏状态下的主循环。

use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_DWORD, RRF_RT_REG_SZ, RegCloseKey,
    RegDeleteValueW, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, RegQueryInfoKeyW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

/// 托盘命令:显示主窗口
pub const CMD_SHOW: &str = "show";
/// 托盘命令:隐藏到托盘
pub const CMD_HIDE: &str = "hide";
/// 托盘命令:退出应用
pub const CMD_QUIT: &str = "quit";

/// 托盘句柄;保活即图标在位,drop 时自动移除
pub struct Tray {
    _inner: TrayIcon,
}

/// 在当前线程创建托盘与菜单,返回(句柄, 命令接收端)
pub fn create(ctx: egui::Context) -> (Tray, Receiver<String>) {
    let menu = Menu::new();
    let show = MenuItem::with_id(CMD_SHOW, "显示主窗口", true, None);
    let hide = MenuItem::with_id(CMD_HIDE, "隐藏到托盘", true, None);
    let quit = MenuItem::with_id(CMD_QUIT, "退出", true, None);
    menu.append(&show).expect("追加托盘菜单项失败");
    menu.append(&hide).expect("追加托盘菜单项失败");
    menu.append(&PredefinedMenuItem::separator())
        .expect("追加托盘分隔符失败");
    menu.append(&quit).expect("追加托盘菜单项失败");

    let (rgba, width, height) = crate::platform::icon::tray_icon_rgba();
    let icon = tray_icon::Icon::from_rgba(rgba, width, height).expect("托盘图标数据非法");
    let tray = TrayIconBuilder::new()
        .with_id("netowl-tray")
        .with_tooltip("NetOwl 网络监控")
        .with_icon(icon)
        .with_menu(Box::new(menu))
        // 左键不弹菜单,而是直接显示主窗口(TrayIconEvent 处理)
        .with_menu_on_left_click(false)
        .build()
        .expect("创建系统托盘失败");

    let (tx, rx) = channel::<String>();
    MenuEvent::set_event_handler(Some(forward(tx.clone(), ctx.clone())));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            send(&tx, CMD_SHOW);
            ctx.request_repaint();
        }
    }));

    (Tray { _inner: tray }, rx)
}

/// 消费托盘命令,返回是否还有未处理事件由调用方决定;这里一次抽干
pub fn drain(receiver: &Receiver<String>) -> Vec<String> {
    let mut cmds = Vec::new();
    while let Ok(cmd) = receiver.try_recv() {
        cmds.push(cmd);
    }
    cmds
}

fn forward(tx: Sender<String>, ctx: egui::Context) -> impl Fn(MenuEvent) + Send + Sync {
    move |event: MenuEvent| {
        send(&tx, &event.id.0);
        ctx.request_repaint();
    }
}

fn send(tx: &Sender<String>, cmd: &str) {
    let _ = tx.send(cmd.to_owned());
}

/// 托盘设置注册表根键与相关值名
const NOTIFY_KEY: &str = "Control Panel\\NotifyIconSettings";
const IS_PROMOTED: &str = "IsPromoted";
const EXECUTABLE_PATH: &str = "ExecutablePath";

/// 托盘图标常驻任务栏(免折叠进隐藏区):写 NotifyIconSettings 项的
/// IsPromoted(DWORD 1),Explorer 新会话直接展示;关闭 = 删除该值还原系统默认。
/// 项名是 Explorer 内部 hash 不可构造,按 ExecutablePath 匹配本进程 exe 定位;
/// 项由 Explorer 在图标注册时创建,刚启动的数秒内可能尚不存在。
/// 返回是否达成目标态:关闭时项/值不存在同样视为达成;开启而项未注册或
/// 写入失败返回 false,由调用方静默降级(保持系统默认行为)并择机重试
pub fn set_pinned(enable: bool) -> bool {
    let exe = match std::env::current_exe() {
        Ok(p) => norm_path(&p.to_string_lossy()),
        Err(e) => {
            tracing::debug!("[Tray] 读取本进程路径失败,托盘常驻未应用: {e}");
            return false;
        }
    };
    unsafe {
        let root_name = HSTRING::from(NOTIFY_KEY);
        let mut root = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(root_name.as_ptr()),
            None,
            KEY_READ,
            &mut root,
        )
        .is_err()
        {
            tracing::debug!("[Tray] 打开 NotifyIconSettings 失败,托盘常驻未应用");
            return false;
        }
        let done = match find_key_by_path(root, &exe) {
            Some(subkey) => apply_promoted(root, &subkey, enable),
            // 关闭态下项不存在即已还原系统默认
            None => !enable,
        };
        let _ = RegCloseKey(root);
        if done {
            tracing::info!("[Tray] 托盘常驻已{}", if enable { "开启" } else { "关闭" });
        }
        done
    }
}

/// 在项上写/删 IsPromoted 值;返回是否达成目标态
fn apply_promoted(root: HKEY, subkey: &str, enable: bool) -> bool {
    unsafe {
        let name = HSTRING::from(subkey);
        let mut key = HKEY::default();
        if RegOpenKeyExW(
            root,
            PCWSTR(name.as_ptr()),
            None,
            KEY_READ | KEY_SET_VALUE,
            &mut key,
        )
        .is_err()
        {
            tracing::debug!("[Tray] 打开托盘设置项 {subkey} 失败");
            return false;
        }
        let value_name = HSTRING::from(IS_PROMOTED);
        let done = if enable {
            RegSetValueExW(
                key,
                PCWSTR(value_name.as_ptr()),
                None,
                REG_DWORD,
                Some(&1u32.to_ne_bytes()),
            )
            .is_ok()
        } else {
            matches!(
                RegDeleteValueW(key, PCWSTR(value_name.as_ptr())),
                ERROR_SUCCESS | ERROR_FILE_NOT_FOUND
            )
        };
        let _ = RegCloseKey(key);
        if !done {
            tracing::debug!("[Tray] 写入 IsPromoted 失败,保持系统默认行为");
        }
        done
    }
}

/// 枚举 NotifyIconSettings 子键,按 ExecutablePath 匹配本进程 exe,返回项名
fn find_key_by_path(root: HKEY, exe: &str) -> Option<String> {
    unsafe {
        let mut count = 0u32;
        let mut max_len = 0u32;
        if RegQueryInfoKeyW(
            root,
            None,
            None,
            None,
            Some(&mut count),
            Some(&mut max_len),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .is_err()
        {
            return None;
        }
        let mut buf = vec![0u16; max_len as usize + 1];
        for i in 0..count {
            let mut len = buf.len() as u32;
            if RegEnumKeyExW(
                root,
                i,
                Some(PWSTR(buf.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
            .is_err()
            {
                continue;
            }
            let name = String::from_utf16_lossy(&buf[..len as usize]);
            if read_string_value(root, &name, EXECUTABLE_PATH).is_some_and(|p| norm_path(&p) == exe)
            {
                return Some(name);
            }
        }
        None
    }
}

/// 读取 REG_SZ/REG_EXPAND_SZ 值(RegGetValueW 对 EXPAND_SZ 自动展开环境变量)
fn read_string_value(root: HKEY, subkey: &str, value: &str) -> Option<String> {
    unsafe {
        let sk = HSTRING::from(subkey);
        let vn = HSTRING::from(value);
        let mut len = 0u32;
        if RegGetValueW(
            root,
            PCWSTR(sk.as_ptr()),
            PCWSTR(vn.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut len),
        )
        .is_err()
        {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        if RegGetValueW(
            root,
            PCWSTR(sk.as_ptr()),
            PCWSTR(vn.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
        .is_err()
        {
            return None;
        }
        let words: Vec<u16> = buf
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .take_while(|&w| w != 0)
            .collect();
        Some(String::from_utf16_lossy(&words))
    }
}

/// 路径规范化:统一小写与反斜杠,供注册表值与 current_exe 比较
fn norm_path(path: &str) -> String {
    path.to_lowercase().replace('/', "\\")
}
