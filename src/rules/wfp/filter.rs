//! WFP 过滤器构造:Spec -> FWPM_FILTER0 的 FFI 翻译(进程 app id blob、
//! 远端网段 RANGE、协议/端口等值条件),含条件值 union 的按类型写入。

use windows::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FWP_ACTION_BLOCK, FWP_ACTION_PERMIT, FWP_BYTE_BLOB, FWP_BYTE_BLOB_TYPE, FWP_CONDITION_VALUE0,
    FWP_CONDITION_VALUE0_0, FWP_MATCH_EQUAL, FWP_MATCH_RANGE, FWP_RANGE_TYPE, FWP_RANGE0,
    FWP_UINT8, FWP_UINT16, FWP_UINT32, FWP_VALUE0, FWP_VALUE0_0, FWPM_ACTION0,
    FWPM_CONDITION_ALE_APP_ID, FWPM_CONDITION_IP_PROTOCOL, FWPM_CONDITION_IP_REMOTE_ADDRESS,
    FWPM_CONDITION_IP_REMOTE_PORT, FWPM_DISPLAY_DATA0, FWPM_FILTER_CONDITION0, FWPM_FILTER0,
    FWPM_LAYER_ALE_AUTH_CONNECT_V4, FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4, FwpmFilterAdd0,
    FwpmFreeMemory0, FwpmGetAppIdFromFileName0,
};
use windows::core::{GUID, PCWSTR, PWSTR};

use super::{Layer, SUBLAYER_KEY, Spec};
use windows::Win32::Foundation::HANDLE;

/// 添加一条过滤器并返回其运行时 GUID;app id blob 经
/// FwpmGetAppIdFromFileName0 规范化为 NT 设备路径(FwpmFilterAdd0
/// 深拷贝条件数据,调用后即可释放)
pub(super) fn add_filter(engine: HANDLE, spec: &Spec) -> Result<GUID, String> {
    let mut blob: *mut FWP_BYTE_BLOB = std::ptr::null_mut();
    let mut range = FWP_RANGE0::default();
    let mut conditions: Vec<FWPM_FILTER_CONDITION0> = Vec::new();

    if let Some(path) = &spec.app_path {
        let wide = to_wide(path);
        let rc = unsafe { FwpmGetAppIdFromFileName0(PCWSTR(wide.as_ptr()), &mut blob) };
        if rc != 0 {
            return Err(format!("AppId({path}) code {rc}"));
        }
        conditions.push(FWPM_FILTER_CONDITION0 {
            fieldKey: FWPM_CONDITION_ALE_APP_ID,
            matchType: FWP_MATCH_EQUAL,
            conditionValue: FWP_CONDITION_VALUE0 {
                r#type: FWP_BYTE_BLOB_TYPE,
                Anonymous: FWP_CONDITION_VALUE0_0 { byteBlob: blob },
            },
        });
    }
    if let Some((lo, hi)) = spec.remote {
        range.valueLow = uint32(lo);
        range.valueHigh = uint32(hi);
        conditions.push(FWPM_FILTER_CONDITION0 {
            fieldKey: FWPM_CONDITION_IP_REMOTE_ADDRESS,
            matchType: FWP_MATCH_RANGE,
            conditionValue: FWP_CONDITION_VALUE0 {
                r#type: FWP_RANGE_TYPE,
                Anonymous: FWP_CONDITION_VALUE0_0 {
                    rangeValue: &mut range,
                },
            },
        });
    }
    if let Some(p) = spec.proto {
        conditions.push(cond_u8(FWPM_CONDITION_IP_PROTOCOL, p));
    }
    if let Some(p) = spec.port {
        conditions.push(cond_u16(FWPM_CONDITION_IP_REMOTE_PORT, p));
    }

    static FILTER_NAME: [u16; 7] = [78, 101, 116, 79, 119, 108, 0];
    let key = GUID::new().map_err(|e| format!("GUID: {e}"))?;
    let action = if spec.block {
        FWP_ACTION_BLOCK
    } else {
        FWP_ACTION_PERMIT
    };
    let filter = FWPM_FILTER0 {
        filterKey: key,
        layerKey: match spec.layer {
            Layer::Out => FWPM_LAYER_ALE_AUTH_CONNECT_V4,
            Layer::In => FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4,
        },
        subLayerKey: SUBLAYER_KEY,
        displayData: FWPM_DISPLAY_DATA0 {
            name: PWSTR(FILTER_NAME.as_ptr() as *mut _),
            description: PWSTR::null(),
        },
        weight: FWP_VALUE0 {
            r#type: FWP_UINT8,
            Anonymous: FWP_VALUE0_0 { uint8: spec.weight },
        },
        numFilterConditions: conditions.len() as u32,
        filterCondition: conditions.as_mut_ptr(),
        action: FWPM_ACTION0 {
            r#type: action,
            ..Default::default()
        },
        ..Default::default()
    };
    let rc = unsafe { FwpmFilterAdd0(engine, &filter, None, None) };
    if !blob.is_null() {
        unsafe { FwpmFreeMemory0(&mut blob as *mut _ as *mut *mut core::ffi::c_void) };
    }
    (rc == 0)
        .then_some(filter.filterKey)
        .ok_or_else(|| format!("FwpmFilterAdd0 code {rc}"))
}

/// 等值条件:8/16 位无符号值按类型分别写入 union(union 布局重叠,
/// 统一走 uint8 成员会截断 16 位值如端口 443)
fn cond_u8(field: GUID, v: u8) -> FWPM_FILTER_CONDITION0 {
    FWPM_FILTER_CONDITION0 {
        fieldKey: field,
        matchType: FWP_MATCH_EQUAL,
        conditionValue: FWP_CONDITION_VALUE0 {
            r#type: FWP_UINT8,
            Anonymous: FWP_CONDITION_VALUE0_0 { uint8: v },
        },
    }
}

fn cond_u16(field: GUID, v: u16) -> FWPM_FILTER_CONDITION0 {
    FWPM_FILTER_CONDITION0 {
        fieldKey: field,
        matchType: FWP_MATCH_EQUAL,
        conditionValue: FWP_CONDITION_VALUE0 {
            r#type: FWP_UINT16,
            Anonymous: FWP_CONDITION_VALUE0_0 { uint16: v },
        },
    }
}

fn uint32(v: u32) -> FWP_VALUE0 {
    FWP_VALUE0 {
        r#type: FWP_UINT32,
        Anonymous: FWP_VALUE0_0 { uint32: v },
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
