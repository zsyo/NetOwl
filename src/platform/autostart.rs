//! 开机自启动:HKCU Run 注册表键值读写。

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR};

/// HKCU 自启动 Run 键:登录后系统按值逐项启动,标准用户权限即可写
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
/// 静默启动参数:自启动拉起时主窗口不弹出,仅托盘运行(main.rs 解析)
pub const MINIMIZED_ARG: &str = "--minimized";
/// 安装器交接参数:NSIS 安装包 Finish 页勾选"开机自启动"后以此参数启动,
/// 应用置位 config.general.autostart 并经 sync_autostart 落注册表
pub const AUTOSTART_ON_ARG: &str = "--autostart-on";

/// 设置开机自启动:开启 = 写 Run 值(值名 = 应用名,内容 = 带引号 exe 路径 +
/// 静默参数),关闭 = 删除该值。返回是否达成目标态;失败由调用方择机重试,
/// 关闭时值不存在同样视为达成
pub fn set_enabled(enable: bool) -> bool {
    unsafe {
        let key_name = HSTRING::from(RUN_KEY);
        let mut key = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_name.as_ptr()),
            None,
            KEY_SET_VALUE,
            &mut key,
        )
        .is_err()
        {
            tracing::debug!("[Autostart] 打开 Run 键失败,自启动未应用");
            return false;
        }
        let value_name = HSTRING::from(crate::APP_NAME);
        let done = if enable {
            let exe = match std::env::current_exe() {
                Ok(p) => p,
                Err(e) => {
                    tracing::debug!("[Autostart] 读取本进程路径失败,自启动未应用: {e}");
                    let _ = RegCloseKey(key);
                    return false;
                }
            };
            let command = format!("\"{}\" {MINIMIZED_ARG}", exe.display());
            // REG_SZ 数据须为 null 结尾的 UTF-16
            let mut data: Vec<u8> = command
                .encode_utf16()
                .flat_map(|u| u.to_le_bytes())
                .collect();
            data.extend_from_slice(&[0, 0]);
            RegSetValueExW(key, PCWSTR(value_name.as_ptr()), None, REG_SZ, Some(&data)).is_ok()
        } else {
            matches!(
                RegDeleteValueW(key, PCWSTR(value_name.as_ptr())),
                ERROR_SUCCESS | ERROR_FILE_NOT_FOUND
            )
        };
        let _ = RegCloseKey(key);
        if done {
            tracing::info!(
                "[Autostart] 开机自启动已{}",
                if enable { "开启" } else { "关闭" }
            );
        } else {
            tracing::debug!("[Autostart] 写入 Run 键失败,自启动未应用");
        }
        done
    }
}
