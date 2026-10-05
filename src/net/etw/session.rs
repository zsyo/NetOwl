//! ETW 会话管理:StartTrace/EnableTrace/OpenTrace 消费线程、会话属性
//! 构造、残留会话清理与实时事件回调;流聚合状态与快照 API 在 mod。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use windows::Win32::System::Diagnostics::Etw::{
    CloseTrace, ControlTraceW, EVENT_RECORD, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW,
    EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE, EnableTraceEx2, OpenTraceW,
    PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME, ProcessTrace, StartTraceW,
    WNODE_FLAG_TRACED_GUID,
};
use windows::core::{GUID, PCWSTR, PWSTR};

use super::{Agg, PAYLOAD_HEAD};

/// Microsoft-Windows-Kernel-Network 提供程序
const PROVIDER: GUID = GUID::from_u128(0x7dd42a49_5329_4832_8dfd_43d979153a88);
/// 会话 GUID(任意固定值,便于查询与残留清理)
const SESSION_GUID: GUID = GUID::from_u128(0x6e744f77_6c4b_4e54_4f57_4c0000000001);
/// 会话名(全局命名实例;启动时按名清理残留)
const SESSION_NAME: PCWSTR = windows::core::w!("NetOwl-KernelNet");

/// 事件回调 try_lock 失败计数(主线程持锁期间到达的事件被丢弃;
/// 回调在内核消费线程内,不能逐条记日志,累计后随快照输出)
pub(super) static LOCK_MISSED: AtomicU64 = AtomicU64::new(0);

/// 启动会话并进入事件循环(消费线程主函数);失败仅记录后返回
pub(super) unsafe fn run_consumer(agg: Arc<Mutex<Agg>>) {
    unsafe {
        let mut session = Default::default();
        let props = prepare_props();
        let err = StartTraceW(&mut session, SESSION_NAME, props);
        if err.is_err() {
            tracing::warn!("[ETW] 会话启动失败({err:?}),流量字节与短命连接不可用");
            return;
        }
        // ENABLE(1)、Information(4)、全部关键字
        let enable = EnableTraceEx2(session, &PROVIDER, 1, 4, u64::MAX, 0, 0, None);
        if enable.is_err() {
            tracing::warn!("[ETW] 提供程序启用失败({enable:?}),流量字节与短命连接不可用");
            let _ = ControlTraceW(
                Default::default(),
                SESSION_NAME,
                props,
                EVENT_TRACE_CONTROL_STOP,
            );
            return;
        }
        let mut logfile = EVENT_TRACE_LOGFILEW {
            LoggerName: PWSTR(SESSION_NAME.as_ptr() as *mut u16),
            ..Default::default()
        };
        logfile.Anonymous2.EventRecordCallback = Some(on_event);
        logfile.Context = Arc::as_ptr(&agg) as *mut core::ffi::c_void;
        logfile.Anonymous1.ProcessTraceMode =
            PROCESS_TRACE_MODE_EVENT_RECORD | PROCESS_TRACE_MODE_REAL_TIME;
        let handle = OpenTraceW(&mut logfile);
        // INVALID_PROCESSTRACE_HANDLE = u64::MAX:消费者注册失败时会话在跑
        // 但收不到任何事件,字节列静默归零,必须留痕
        if handle.Value == u64::MAX {
            tracing::warn!("[ETW] 消费者注册失败(OpenTraceW),流量字节与短命连接不可用");
        }
        let _ = ProcessTrace(&[handle], None, None);
        let _ = CloseTrace(handle);
        let _ = ControlTraceW(
            Default::default(),
            SESSION_NAME,
            props,
            EVENT_TRACE_CONTROL_STOP,
        );
    }
}

/// 构造 StartTrace/ControlTrace 共用的会话属性缓冲
/// (结构体后跟会话名 UTF-16 区)
unsafe fn prepare_props() -> *mut EVENT_TRACE_PROPERTIES {
    unsafe {
        let name_wide = SESSION_NAME.as_wide();
        let total = size_of::<EVENT_TRACE_PROPERTIES>() + (name_wide.len() + 1) * 2;
        let buf = vec![0u8; total].leak() as *mut [u8] as *mut u8;
        let props = buf as *mut EVENT_TRACE_PROPERTIES;
        (*props).Wnode.BufferSize = total as u32;
        (*props).Wnode.Guid = SESSION_GUID;
        (*props).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        (*props).Wnode.ClientContext = 1;
        // BufferSize 单位 KB(1MB/缓冲);实时模式满缓冲即投递消费者,
        // Kernel-Network 事件量下足量且内核侧总占用 = 缓冲数 × 1MB。
        // 此前误传字节值(64MiB/缓冲),乘 CPU 数后会话缓冲达 GB 级
        (*props).BufferSize = 1024;
        (*props).MinimumBuffers = 8;
        (*props).MaximumBuffers = 32;
        (*props).FlushTimer = 1;
        (*props).LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        (*props).LoggerNameOffset = size_of::<EVENT_TRACE_PROPERTIES>() as u32;
        (*props).LogFileNameOffset = 0;
        props
    }
}

/// 停止同名会话(含上次异常退出的残留);会话不存在视为已停止
pub(super) fn stop_session() -> Result<(), String> {
    const ERROR_WMI_INSTANCE_NOT_FOUND: u32 = 4201;
    let props = unsafe { prepare_props() };
    let err = unsafe {
        ControlTraceW(
            Default::default(),
            SESSION_NAME,
            props,
            EVENT_TRACE_CONTROL_STOP,
        )
    };
    // 会话不存在 = 无需清理;其余错误如实上报
    if err.is_err() && err.0 != ERROR_WMI_INSTANCE_NOT_FOUND {
        return Err(format!("停止残留 ETW 会话失败: {err:?}"));
    }
    Ok(())
}

/// 实时事件回调(ProcessTrace 消费线程内同步调用;Context 即 Agg)
unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    unsafe {
        let r = &*record;
        if r.EventHeader.ProviderId != PROVIDER {
            return;
        }
        if usize::from(r.UserDataLength) < PAYLOAD_HEAD || r.UserData.is_null() {
            return;
        }
        let agg = &*(r.UserContext as *const Mutex<Agg>);
        let Ok(mut g) = agg.try_lock() else {
            LOCK_MISSED.fetch_add(1, Ordering::Relaxed);
            return;
        };
        super::decode::handle_event(&mut g, r);
    }
}
