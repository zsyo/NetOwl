//! 开机自启动:按进程权限选择机制。管理员令牌经计划任务(登录触发,最高
//! 权限,免 UAC)——Run 键启动需要提权的程序会被 Windows 静默跳过;标准
//! 用户经 HKCU Run 键(登录后普通权限运行)。

use std::os::windows::process::CommandExt;

use windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    RegSetValueExW,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{HSTRING, PCWSTR};

/// HKCU 自启动 Run 键(标准用户路径;管理员用户迁移到计划任务后仅作清理)
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
/// 计划任务名(管理员令牌路径,登录触发)
const TASK_NAME: &str = "NetOwl";
/// 静默启动参数:自启动拉起时主窗口不弹出,仅托盘运行(main.rs 解析)
pub const MINIMIZED_ARG: &str = "--minimized";
/// 安装器交接参数:NSIS 安装包"附加选项"页勾选"开机自启动"后以此参数启动,
/// 应用置位 config.general.autostart 并经 sync_autostart 落地
pub const AUTOSTART_ON_ARG: &str = "--autostart-on";
/// schtasks 子进程隐藏控制台窗口
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 设置开机自启动:开启 = 管理员令牌注册登录触发的最高权限计划任务(任务
/// 以最高权限静默启动,不弹 UAC;成功后清理 Run 键历史残留),标准用户写
/// Run 值(普通权限运行);关闭 = 删除任务并清理 Run 键残留,各自"已不
/// 存在"均视作达成。返回是否达成目标态;失败由调用方择机重试
pub fn set_enabled(enable: bool) -> bool {
    let done = if enable {
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!("[Autostart] 读取本进程路径失败,自启动未应用: {e}");
                return false;
            }
        };
        let command = format!("\"{}\" {MINIMIZED_ARG}", exe.display());
        if is_elevated() {
            run_schtasks(&[
                "/Create", "/TN", TASK_NAME, "/TR", &command, "/SC", "ONLOGON", "/RL", "HIGHEST",
                "/F",
            ]) && delete_run_value()
        } else {
            write_run_value(&command)
        }
    } else {
        let task_gone = if task_exists() {
            run_schtasks(&["/Delete", "/TN", TASK_NAME, "/F"])
        } else {
            true
        };
        task_gone && delete_run_value()
    };
    if done {
        tracing::info!(
            "[Autostart] 开机自启动已{}",
            if enable { "开启" } else { "关闭" }
        );
    } else {
        tracing::debug!("[Autostart] 写入自启动失败,自启动未应用");
    }
    done
}

/// 当前进程是否以管理员令牌运行(与 rules::wfp::is_elevated 同源;平台层
/// 独立持有,避免反向依赖规则模块)
fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut ret = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elev as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elev.TokenIsElevated != 0
    }
}

/// 执行 schtasks(隐藏控制台窗口),返回退出码是否为 0
fn run_schtasks(args: &[&str]) -> bool {
    std::process::Command::new("schtasks")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn task_exists() -> bool {
    run_schtasks(&["/Query", "/TN", TASK_NAME])
}

/// 写 Run 键值(标准用户路径):值名 = 应用名,内容 = 带引号 exe 路径 +
/// 静默参数,REG_SZ、UTF-16 null 结尾
fn write_run_value(command: &str) -> bool {
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
        let mut data: Vec<u8> = command
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        data.extend_from_slice(&[0, 0]);
        let done =
            RegSetValueExW(key, PCWSTR(value_name.as_ptr()), None, REG_SZ, Some(&data)).is_ok();
        let _ = RegCloseKey(key);
        done
    }
}

/// 删除 Run 键值(值不存在同样视为达成)
fn delete_run_value() -> bool {
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
            return true;
        }
        let value_name = HSTRING::from(crate::APP_NAME);
        let done = matches!(
            RegDeleteValueW(key, PCWSTR(value_name.as_ptr())),
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND
        );
        let _ = RegCloseKey(key);
        done
    }
}
