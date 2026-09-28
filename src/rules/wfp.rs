//! WFP 拦截:把启用规则翻译为 Windows Filtering Platform 过滤器,
//! 实现真实的按进程/远端放行与阻断。
//!
//! 专用线程持有动态会话(FWPM_SESSION_FLAG_DYNAMIC):进程退出或崩溃时
//! 内核对象自动销毁,不会遗留永久阻断;优雅退出经 FwpmEngineClose0 同样
//! 全量清除。目标过滤器集合由 App 与采集同频(约 1s)重建,线程内 diff
//! 增删,UI 线程不受阻塞。无管理员权限时引擎打开失败,状态回落 NoAdmin:
//! 规则求值标注仍可用(只读模式),以管理员启动后自动生效。
//!
//! 语义:全部过滤器挂在一个子层下,weight 由规则优先级派生(优先级高者
//! weight 大),同一连接命中的最高 weight 过滤器决定动作——与求值引擎的
//! "按优先级首个命中"一致。子层权重置 0(低于系统防火墙),permit 不
//! 越过系统防火墙的阻断策略。域名规则的 WFP 静态条件无法表达(域名到 IP
//! 是动态映射),此类规则不参与过滤器翻译,仅由连接列表求值标注;真实
//! 拦截覆盖 进程/网段/端口/协议/方向 维度。

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FWP_ACTION_BLOCK, FWP_ACTION_PERMIT, FWP_BYTE_BLOB, FWP_BYTE_BLOB_TYPE, FWP_CONDITION_VALUE0,
    FWP_CONDITION_VALUE0_0, FWP_MATCH_EQUAL, FWP_MATCH_RANGE, FWP_RANGE_TYPE, FWP_RANGE0,
    FWP_UINT8, FWP_UINT16, FWP_UINT32, FWP_VALUE0, FWP_VALUE0_0, FWPM_ACTION0,
    FWPM_CONDITION_ALE_APP_ID, FWPM_CONDITION_IP_PROTOCOL, FWPM_CONDITION_IP_REMOTE_ADDRESS,
    FWPM_CONDITION_IP_REMOTE_PORT, FWPM_DISPLAY_DATA0, FWPM_FILTER_CONDITION0, FWPM_FILTER0,
    FWPM_LAYER_ALE_AUTH_CONNECT_V4, FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4, FWPM_SESSION_FLAG_DYNAMIC,
    FWPM_SESSION0, FWPM_SUBLAYER0, FwpmEngineClose0, FwpmEngineOpen0, FwpmFilterAdd0,
    FwpmFilterDeleteByKey0, FwpmFreeMemory0, FwpmGetAppIdFromFileName0, FwpmSubLayerAdd0,
};
use windows::core::{GUID, PCWSTR, PWSTR};

/// NetOwl 规则子层(固定 GUID;动态会话内每次创建,会话结束即销毁)
const SUBLAYER_KEY: GUID = GUID::from_u128(0x8f3a9c2e_5b14_4d7e_9a6c_3e2d1b0f8a55);
/// ERROR_ACCESS_DENIED:非管理员打开引擎
const OPEN_ACCESS_DENIED: u32 = 5;

/// 过滤器所在 ALE 层:出站连入 vs 入站接受
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    Out,
    In,
}

/// 一条 WFP 过滤器的描述;引擎线程按此增删
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Spec {
    pub layer: Layer,
    /// 优先级派生:数值大者优先判定
    pub weight: u8,
    /// true = 阻断,false = 放行
    pub block: bool,
    /// 进程映像完整路径(Win32 形式,引擎线程转 app id blob)
    pub app_path: Option<String>,
    /// 远端网段区间 [lo, hi](主机序)
    pub remote: Option<(u32, u32)>,
    pub proto: Option<u8>,
    pub port: Option<u16>,
}

/// 拦截引擎状态(设置/规则页展示)
#[derive(Clone, Debug)]
pub enum Status {
    /// 初始化中
    Off,
    /// 无管理员权限:引擎未打开,规则仅标注
    NoAdmin,
    /// 引擎操作失败(异常)
    Failed(String),
    /// 已生效;当前过滤器数量
    Active(usize),
}

enum Msg {
    Sync(Arc<Vec<Spec>>),
}

/// WFP 管理线程句柄;Drop 即关闭通道并等待线程收尾(动态会话随之清除)
pub struct Manager {
    tx: Option<Sender<Msg>>,
    handle: Option<JoinHandle<()>>,
    status: Arc<Mutex<Status>>,
}

impl Manager {
    pub fn spawn() -> Manager {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status::Off));
        let s = Arc::clone(&status);
        let handle = std::thread::Builder::new()
            .name("wfp".into())
            .spawn(move || run(rx, s))
            .expect("启动 WFP 管理线程");
        Manager {
            tx: Some(tx),
            handle: Some(handle),
            status,
        }
    }

    /// 全量同步目标过滤器集合;线程内与当前集合 diff 增删
    pub fn sync(&self, specs: Arc<Vec<Spec>>) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::Sync(specs));
        }
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or(Status::Off)
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        self.tx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// 临时诊断:提权实例 stderr 不可见,验证期间写文件日志(验证后移除)
fn run(rx: Receiver<Msg>, status: Arc<Mutex<Status>>) {
    let set = |v: Status| {
        if let Ok(mut s) = status.lock() {
            *s = v;
        }
    };
    // 提权预检:动态会话非管理员也能打开,添加对象时才会被拒,
    // 直接在入口判定,失败态不持有引擎句柄
    if !is_elevated() {
        set(Status::NoAdmin);
        // 只读模式:吞掉同步消息直到退出,规则求值标注不受影响
        while rx.recv().is_ok() {}
        return;
    }
    let engine = match open_engine() {
        Ok(e) => e,
        Err(OPEN_ACCESS_DENIED) => {
            set(Status::NoAdmin);
            // 只读模式:吞掉同步消息直到退出,规则求值标注不受影响
            while rx.recv().is_ok() {}
            return;
        }
        Err(rc) => {
            set(Status::Failed(format!("FwpmEngineOpen0 code {rc}")));
            while rx.recv().is_ok() {}
            return;
        }
    };
    match ensure_sublayer(engine) {
        Ok(()) => {}
        Err(OPEN_ACCESS_DENIED) => {
            set(Status::NoAdmin);
            unsafe {
                let _ = FwpmEngineClose0(engine);
            }
            while rx.recv().is_ok() {}
            return;
        }
        Err(rc) => {
            set(Status::Failed(format!(
                "sublayer: FwpmSubLayerAdd0 code {rc}"
            )));
            unsafe {
                let _ = FwpmEngineClose0(engine);
            }
            while rx.recv().is_ok() {}
            return;
        }
    }

    let mut live: HashMap<GUID, Spec> = HashMap::new();
    set(Status::Active(0));
    // 收敛到最新目标集合再 reconcile(同步消息可以任意密集)
    while let Ok(Msg::Sync(first)) = rx.recv() {
        let mut latest = first;
        while let Ok(Msg::Sync(s)) = rx.try_recv() {
            latest = s;
        }
        reconcile(engine, &mut live, &latest);
        set(Status::Active(live.len()));
    }
    // 关闭动态会话:全部过滤器与子层随之清除
    unsafe {
        let _ = FwpmEngineClose0(engine);
    }
    set(Status::Off);
}

/// 打开动态会话引擎;非管理员返回 ERROR_ACCESS_DENIED(5)
fn open_engine() -> Result<HANDLE, u32> {
    let session = FWPM_SESSION0 {
        flags: FWPM_SESSION_FLAG_DYNAMIC,
        ..Default::default()
    };
    let mut handle = HANDLE::default();
    // RPC_C_AUTHN_WINNT = 10
    let rc = unsafe { FwpmEngineOpen0(PCWSTR::null(), 10, None, Some(&session), &mut handle) };
    (rc == 0).then_some(handle).ok_or(rc)
}

/// 创建 NetOwl 子层(weight 0:低于系统防火墙,不抢占系统策略判定);
/// 失败返回 WIN32 错误码
fn ensure_sublayer(engine: HANDLE) -> Result<(), u32> {
    // "NetOwl" UTF-16;子层名静态保活
    static NAME: [u16; 7] = [78, 101, 116, 79, 119, 108, 0];
    let sl = FWPM_SUBLAYER0 {
        subLayerKey: SUBLAYER_KEY,
        displayData: FWPM_DISPLAY_DATA0 {
            name: PWSTR(NAME.as_ptr() as *mut _),
            description: PWSTR::null(),
        },
        ..Default::default()
    };
    let rc = unsafe { FwpmSubLayerAdd0(engine, &sl, None) };
    (rc == 0).then_some(()).ok_or(rc)
}

/// diff 当前集合与目标集合,事务外逐条增删(单条失败仅记录,不影响其余)
fn reconcile(engine: HANDLE, live: &mut HashMap<GUID, Spec>, target: &[Spec]) {
    let target_set: HashSet<&Spec> = target.iter().collect();
    let stale: Vec<GUID> = live
        .iter()
        .filter(|(_, s)| !target_set.contains(*s))
        .map(|(k, _)| *k)
        .collect();
    for key in &stale {
        let rc = unsafe { FwpmFilterDeleteByKey0(engine, key) };
        if rc == 0 {
            live.remove(key);
        } else {
            tracing::warn!("[Wfp] 删除过滤器失败 code {rc}");
        }
    }
    let live_specs: HashSet<Spec> = live.values().cloned().collect();
    for spec in target {
        if live_specs.contains(spec) {
            continue;
        }
        match add_filter(engine, spec) {
            Ok(key) => {
                live.insert(key, spec.clone());
            }
            Err(e) => tracing::warn!("[Wfp] 添加过滤器失败: {e}"),
        }
    }
}

/// 添加一条过滤器并返回其运行时 GUID;app id blob 经
/// FwpmGetAppIdFromFileName0 规范化为 NT 设备路径(FwpmFilterAdd0
/// 深拷贝条件数据,调用后即可释放)
fn add_filter(engine: HANDLE, spec: &Spec) -> Result<GUID, String> {
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

/// 当前进程是否以管理员令牌运行
pub fn is_elevated() -> bool {
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
        let _ = windows::Win32::Foundation::CloseHandle(token);
        ok && elev.TokenIsElevated != 0
    }
}
