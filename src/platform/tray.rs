//! 托盘与菜单:tray-icon + muda(经 tray_icon::menu re-export)。
//!
//! 托盘在 eframe 主线程创建(见 AGENTS.md 平台规范),事件经
//! set_event_handler 转发到 mpsc 通道,并 request_repaint 唤醒隐藏状态下的主循环。

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use tray_icon::menu::{
    CheckMenuItem, ContextMenu, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HWND, LPARAM, WPARAM};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_DWORD, RRF_RT_REG_SZ, RegCloseKey,
    RegDeleteValueW, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, RegQueryInfoKeyW, RegSetValueExW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    HMENU, PostMessageW, SetForegroundWindow, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TrackPopupMenu,
    WM_NULL,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

/// 托盘命令:显示主窗口
pub const CMD_SHOW: &str = "show";
/// 托盘命令:退出应用
pub const CMD_QUIT: &str = "quit";
/// 托盘命令:静默模式三态切换
pub const CMD_SILENT_OFF: &str = "silent-off";
pub const CMD_SILENT_ALLOW: &str = "silent-allow";
pub const CMD_SILENT_DENY: &str = "silent-deny";
/// 托盘命令:新连接询问开关切换
pub const CMD_ASK_TOGGLE: &str = "ask-toggle";
/// 托盘命令:打开实时日志窗口
pub const CMD_LOG_OPEN: &str = "log-open";
/// 托盘命令:主窗口落到设置页
pub const CMD_SETTINGS: &str = "settings";

/// 最后一次托盘右键的光标位置(物理坐标):tray-icon 内部右键弹出菜单
/// 用光标定位,而 show_menu() 用 Shell_NotifyIconGetRect 图标矩形定位,
/// 两者结果不同——开关项重开菜单须复用右键锚点,否则菜单位置跳动
static MENU_ANCHOR: Mutex<Option<(i32, i32)>> = Mutex::new(None);

/// 托盘句柄;保活即图标在位,drop 时自动移除
pub struct Tray {
    _inner: TrayIcon,
    /// 静默模式三态勾选项(句柄保活;app 侧 sync_silent 校准勾选态)
    silent_items: [CheckMenuItem; 3],
    /// 新连接询问勾选项(句柄保活;app 侧 sync_ask 校准勾选态)
    ask_item: CheckMenuItem,
    /// 托盘消息窗口与菜单句柄(重开菜单用;菜单随 TrayIcon 保活,
    /// 句柄在 Tray 存续期内有效)
    hwnd: isize,
    hmenu: isize,
}

impl Tray {
    /// 更新悬停提示(速率跟随刷新用);失败静默(Windows tooltip
    /// 偶发重建失败不值得打扰)
    pub fn set_tooltip(&self, text: &str) {
        let _ = self._inner.set_tooltip(Some(text));
    }

    /// 重开托盘菜单:锚点复用最后一次右键弹出的光标位置,菜单在原位
    /// 重现(开关项点击后的保持打开效果);无记录时回退托盘图标定位
    pub fn reopen_menu(&self) {
        let anchor = MENU_ANCHOR.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((x, y)) = *anchor {
            self.show_menu_at(x, y);
        } else {
            self._inner.show_menu();
        }
    }

    /// 在指定物理坐标弹出托盘菜单:复刻 tray-icon 的 show_tray_menu
    /// (SetForegroundWindow 保证点击外部自动关闭,BOTTOMALIGN+LEFTALIGN
    /// 与右键弹出同款对齐,弹出后按 shell 建议投递 WM_NULL 收尾)
    fn show_menu_at(&self, x: i32, y: i32) {
        let hwnd = HWND(self.hwnd as _);
        unsafe {
            // 返回值仅表示前台切换/弹出是否成功,菜单不弹出时无需处理
            let _ = SetForegroundWindow(hwnd);
            let _ = TrackPopupMenu(
                HMENU(self.hmenu as _),
                TPM_LEFTALIGN | TPM_BOTTOMALIGN,
                x,
                y,
                None,
                hwnd,
                None,
            );
            let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        }
    }

    /// 校准静默模式勾选态(设置页与托盘双入口,app 侧状态唯一来源);
    /// 空串等非法值视作 off
    pub fn sync_silent(&self, mode: &str) {
        let checks = [
            mode != "allow" && mode != "deny",
            mode == "allow",
            mode == "deny",
        ];
        for (item, checked) in self.silent_items.iter().zip(checks) {
            item.set_checked(checked);
        }
    }

    /// 校准新连接询问勾选态(设置页与托盘双入口,app 侧状态唯一来源)
    pub fn sync_ask(&self, enabled: bool) {
        self.ask_item.set_checked(enabled);
    }
}

/// 在当前线程创建托盘与菜单,返回(句柄, 命令接收端);菜单与提示文案
/// 经 i18n 按启动语言取词(托盘创建一次,运行期语言切换不重建),
/// `silent` 为初始静默模式,`ask` 为新连接询问初始勾选态
pub fn create(
    ctx: egui::Context,
    i18n: &crate::i18n::I18n,
    silent: &str,
    ask: bool,
) -> (Tray, Receiver<String>) {
    let menu = Menu::new();
    let show = MenuItem::with_id(CMD_SHOW, i18n.t("tray-show"), true, None);
    menu.append(&show).expect("追加托盘菜单项失败");
    menu.append(&PredefinedMenuItem::separator())
        .expect("追加托盘分隔符失败");
    // "未命中规则的连接如何处理"一组:新连接询问开关 + 静默模式子菜单;
    // 开关项点击时 muda 自动翻转勾选并发出事件,app 侧按事件翻转 config
    // 后经 sync_ask 校准
    let ask_item = CheckMenuItem::with_id(CMD_ASK_TOGGLE, i18n.t("settings-ask"), true, ask, None);
    menu.append(&ask_item).expect("追加询问菜单项失败");
    let silent_menu = Submenu::with_id("silent-submenu", i18n.t("tray-silent"), true);
    let silent_off = CheckMenuItem::with_id(
        CMD_SILENT_OFF,
        i18n.t("settings-silent-off"),
        true,
        silent != "allow" && silent != "deny",
        None,
    );
    let silent_allow = CheckMenuItem::with_id(
        CMD_SILENT_ALLOW,
        i18n.t("settings-silent-allow"),
        true,
        silent == "allow",
        None,
    );
    let silent_deny = CheckMenuItem::with_id(
        CMD_SILENT_DENY,
        i18n.t("settings-silent-deny"),
        true,
        silent == "deny",
        None,
    );
    silent_menu.append(&silent_off).expect("追加静默菜单项失败");
    silent_menu
        .append(&silent_allow)
        .expect("追加静默菜单项失败");
    silent_menu
        .append(&silent_deny)
        .expect("追加静默菜单项失败");
    menu.append(&silent_menu).expect("追加静默子菜单失败");
    menu.append(&PredefinedMenuItem::separator())
        .expect("追加托盘分隔符失败");
    let log = MenuItem::with_id(CMD_LOG_OPEN, i18n.t("tray-log"), true, None);
    menu.append(&log).expect("追加日志菜单项失败");
    let settings = MenuItem::with_id(CMD_SETTINGS, i18n.t("settings-title"), true, None);
    menu.append(&settings).expect("追加设置菜单项失败");
    menu.append(&PredefinedMenuItem::separator())
        .expect("追加托盘分隔符失败");
    let quit = MenuItem::with_id(CMD_QUIT, i18n.t("tray-quit"), true, None);
    menu.append(&quit).expect("追加托盘菜单项失败");

    let (rgba, width, height) = crate::platform::icon::tray_icon_rgba();
    let icon = tray_icon::Icon::from_rgba(rgba, width, height).expect("托盘图标数据非法");
    // 句柄在 build 前后分别取得:菜单句柄随 move 失效前先存,窗口句柄
    // build 后才存在(重开菜单用,见 Tray::reopen_menu)
    let hmenu = menu.hpopupmenu();
    let tray = TrayIconBuilder::new()
        .with_id("netowl-tray")
        .with_tooltip(i18n.t("tray-tooltip"))
        .with_icon(icon)
        .with_menu(Box::new(menu))
        // 左键不弹菜单,而是直接显示主窗口(TrayIconEvent 处理)
        .with_menu_on_left_click(false)
        .build()
        .expect("创建系统托盘失败");
    let hwnd = tray.window_handle() as isize;

    let (tx, rx) = channel::<String>();
    MenuEvent::set_event_handler(Some(forward(tx.clone(), ctx.clone())));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            position,
            button,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            if button == MouseButton::Right {
                // tray-icon 右键弹出菜单前同步发出本事件,光标即弹出锚点
                *MENU_ANCHOR.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((position.x as i32, position.y as i32));
            } else {
                send(&tx, CMD_SHOW);
                ctx.request_repaint();
            }
        }
    }));

    (
        Tray {
            _inner: tray,
            silent_items: [silent_off, silent_allow, silent_deny],
            ask_item,
            hwnd,
            hmenu,
        },
        rx,
    )
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
