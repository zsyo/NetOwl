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
//! 拦截覆盖 进程/网段/端口/协议/方向 维度。FWPM_FILTER0 构造见 filter。

mod filter;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FWPM_DISPLAY_DATA0, FWPM_SESSION_FLAG_DYNAMIC, FWPM_SESSION0, FWPM_SUBLAYER0, FwpmEngineClose0,
    FwpmEngineOpen0, FwpmFilterDeleteByKey0, FwpmSubLayerAdd0,
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
    /// 同步失败已上报:管理线程死亡后 sync 每 1s 都会失败,只报首次
    send_dead: AtomicBool,
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
            send_dead: AtomicBool::new(false),
        }
    }

    /// 全量同步目标过滤器集合;线程内与当前集合 diff 增删
    pub fn sync(&self, specs: Arc<Vec<Spec>>) {
        if let Some(tx) = &self.tx
            && tx.send(Msg::Sync(specs)).is_err()
            && !self.send_dead.swap(true, Ordering::Relaxed)
        {
            tracing::error!("[Wfp] 管理线程已退出,过滤器同步停止,拦截不再更新");
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
        tracing::info!("[Wfp] 未提权运行,拦截引擎未启动(规则仅求值标注)");
        // 只读模式:吞掉同步消息直到退出,规则求值标注不受影响
        while rx.recv().is_ok() {}
        return;
    }
    let engine = match open_engine() {
        Ok(e) => e,
        Err(OPEN_ACCESS_DENIED) => {
            set(Status::NoAdmin);
            tracing::warn!("[Wfp] 引擎打开被拒绝(非管理员),拦截引擎未启动");
            // 只读模式:吞掉同步消息直到退出,规则求值标注不受影响
            while rx.recv().is_ok() {}
            return;
        }
        Err(rc) => {
            set(Status::Failed(format!("FwpmEngineOpen0 code {rc}")));
            tracing::error!("[Wfp] 引擎打开失败 code {rc},拦截引擎未启动");
            while rx.recv().is_ok() {}
            return;
        }
    };
    match ensure_sublayer(engine) {
        Ok(()) => {}
        Err(OPEN_ACCESS_DENIED) => {
            set(Status::NoAdmin);
            tracing::warn!("[Wfp] 子层创建被拒绝(非管理员),拦截引擎未启动");
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
            tracing::error!("[Wfp] 子层创建失败 code {rc},拦截引擎未启动");
            unsafe {
                let _ = FwpmEngineClose0(engine);
            }
            while rx.recv().is_ok() {}
            return;
        }
    }

    let mut live: HashMap<GUID, Spec> = HashMap::new();
    set(Status::Active(0));
    tracing::info!("[Wfp] 拦截引擎已启动(动态会话,退出自毁)");
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
    tracing::info!("[Wfp] 管理线程收尾,动态会话已关闭");
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
    let mut added = 0usize;
    for spec in target {
        if live_specs.contains(spec) {
            continue;
        }
        match filter::add_filter(engine, spec) {
            Ok(key) => {
                live.insert(key, spec.clone());
                added += 1;
            }
            Err(e) => tracing::warn!("[Wfp] 添加过滤器失败: {e}"),
        }
    }
    if added > 0 || !stale.is_empty() {
        tracing::debug!(
            "[Wfp] 过滤器同步:增 {added} 删 {},共 {} 条",
            stale.len(),
            live.len()
        );
    }
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
