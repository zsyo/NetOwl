//! 进程映像的 Authenticode 签名校验(WinVerifyTrust,GENERIC_VERIFY_V2)。
//!
//! 仅判定"有效签名 / 未签名 / 校验未通过"三态,UI_NONE + REVOKE_NONE:
//! 不弹窗、不做吊销联网检查。大文件哈希可能耗时秒级,调用方必须在
//! 工作线程执行(采集器按 PID 异步派发,结果缓存)。

use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0,
    WINTRUST_DATA_PROVIDER_FLAGS, WINTRUST_FILE_INFO, WTD_CHOICE_FILE, WTD_REVOKE_NONE,
    WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTD_UICONTEXT_EXECUTE,
    WinVerifyTrust,
};
use windows::core::{GUID, PCWSTR, PWSTR};

use crate::model::Signing;

/// 校验映像文件的 Authenticode 签名;文件不可访问等错误归入 Invalid
pub fn verify(path: &str) -> Signing {
    let mut path_w: Vec<u16> = path.encode_utf16().collect();
    path_w.push(0);
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(path_w.as_ptr()),
        hFile: HANDLE::default(),
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let mut wd = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        pPolicyCallbackData: std::ptr::null_mut(),
        pSIPClientData: std::ptr::null_mut(),
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &mut file_info,
        },
        dwStateAction: WTD_STATEACTION_VERIFY,
        hWVTStateData: HANDLE::default(),
        pwszURLReference: PWSTR::null(),
        dwProvFlags: WINTRUST_DATA_PROVIDER_FLAGS(0),
        dwUIContext: WTD_UICONTEXT_EXECUTE,
        pSignatureSettings: std::ptr::null_mut(),
    };
    let rc = unsafe { run(&mut action, &mut wd) };
    // VERIFY 会保持验证会话状态,取完结果必须 CLOSE 释放
    wd.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe { run(&mut action, &mut wd) };

    const S_OK: i32 = 0;
    // 0x800B0100:主题无签名
    const TRUST_E_NOSIGNATURE: i32 = 0x800B_0100u32 as i32;
    match rc {
        S_OK => Signing::Signed,
        TRUST_E_NOSIGNATURE => Signing::Unsigned,
        _ => Signing::Invalid,
    }
}

unsafe fn run(action: &mut GUID, wd: &mut WINTRUST_DATA) -> i32 {
    unsafe {
        WinVerifyTrust(
            HWND::default(),
            action,
            wd as *mut WINTRUST_DATA as *mut core::ffi::c_void,
        )
    }
}
