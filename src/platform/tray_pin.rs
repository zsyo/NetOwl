//! 托盘常驻(任务栏不折叠)注册表写入:NotifyIconSettings 项的
//! IsPromoted 值读写。与托盘 UI 本体无关,独立成文件。

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_DWORD, RRF_RT_REG_SZ, RegCloseKey,
    RegDeleteValueW, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, RegQueryInfoKeyW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

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
            tracing::warn!("[Tray] 读取本进程路径失败,托盘常驻未应用: {e}");
            return false;
        }
    };
    unsafe {
        let root_name = HSTRING::from(NOTIFY_KEY);
        let mut root = HKEY::default();
        let opened = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(root_name.as_ptr()),
            None,
            KEY_READ,
            &mut root,
        );
        if opened.is_err() {
            tracing::warn!("[Tray] 打开 NotifyIconSettings 失败,托盘常驻未应用: {opened:?}");
            return false;
        }
        let done = match find_key_by_path(root, &exe) {
            Some(subkey) => apply_promoted(root, &subkey, enable),
            // 开启态下项尚未注册(Explorer 刚启动/图标刚注册的数秒内
            // 可能发生)属预期瞬态,交由调用方定时重试;关闭态下项不
            // 存在即已还原系统默认
            None => {
                if enable {
                    tracing::debug!("[Tray] 托盘设置项尚未注册,等待重试");
                }
                !enable
            }
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
        let opened = RegOpenKeyExW(
            root,
            PCWSTR(name.as_ptr()),
            None,
            KEY_READ | KEY_SET_VALUE,
            &mut key,
        );
        if opened.is_err() {
            tracing::warn!("[Tray] 打开托盘设置项 {subkey} 失败: {opened:?}");
            return false;
        }
        let value_name = HSTRING::from(IS_PROMOTED);
        let done = if enable {
            let written = RegSetValueExW(
                key,
                PCWSTR(value_name.as_ptr()),
                None,
                REG_DWORD,
                Some(&1u32.to_ne_bytes()),
            );
            if written.is_err() {
                tracing::warn!("[Tray] 写入 IsPromoted 失败,保持系统默认行为: {written:?}");
                false
            } else {
                true
            }
        } else {
            let result = RegDeleteValueW(key, PCWSTR(value_name.as_ptr()));
            let ok = matches!(result, ERROR_SUCCESS | ERROR_FILE_NOT_FOUND);
            if !ok {
                tracing::warn!("[Tray] 删除 IsPromoted 失败: {result:?}");
            }
            ok
        };
        let _ = RegCloseKey(key);
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
