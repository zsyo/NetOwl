//! 关机/注销落库兜底:winit 不处理 WM_QUERYENDSESSION/WM_ENDSESSION
//! (透传默认窗口过程),系统结束会话时进程被直接终止,eframe 的退出
//! 回调不会执行——写线程队列中未提交的事件与 Tracker 内存中的活跃连接
//! 全部丢失。此处子类化主窗口拦截 WM_ENDSESSION(wParam 非 0 = 会话
//! 确定结束),执行与托盘"退出"一致的收尾:flush 活跃连接并等待写线程
//! 提交,毫秒级完成,远小于系统强杀前的等待窗口。
//!
//! 仅拦截 ENDSESSION,不拦 QUERYENDSESSION:不阻止也不延迟关机。
//! 托盘"退出"与关机重复触发天然幂等(flush drain 后二次为空,shutdown
//! 后 send 为空操作);窗口销毁后钩子不再收到消息,静态指针随之失效
//! 但无解引用路径。

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, GWLP_WNDPROC, SetWindowLongPtrW, WM_ENDSESSION,
};

use crate::storage::history::{Tracker, Writer, unix_now};

/// 窗口过程裸函数指针(旧过程地址与子类过程共用该形态)
type RawWndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

/// 已安装标志(防重复子类化)
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// 原窗口过程地址;0 = 未安装
static OLD_WNDPROC: AtomicIsize = AtomicIsize::new(0);
/// 收尾对象指针(裸指针转 isize 存放,满足 static 的 Send 约束)
static CTX: Mutex<Option<Ctx>> = Mutex::new(None);

struct Ctx {
    tracker: isize,
    writer: isize,
}

/// 子类化主窗口并注册关机收尾对象;hwnd 无效或子类化失败仅告警一次,
/// 关机时行为回退为未装钩子(数据丢失,与无钩子一致)
pub fn install(hwnd: isize, tracker: &mut Tracker, writer: &mut Writer) {
    if hwnd == 0 {
        tracing::warn!("[Shutdown] 未找到主窗口,关机落库钩子未安装");
        return;
    }
    if INSTALLED.swap(true, Ordering::AcqRel) {
        return;
    }
    *CTX.lock().unwrap_or_else(|e| e.into_inner()) = Some(Ctx {
        tracker: tracker as *mut Tracker as isize,
        writer: writer as *mut Writer as isize,
    });
    // SetWindowLongPtrW 的过程地址参数是 isize,fn 指针数值化是子类化固有操作
    #[allow(clippy::fn_to_numeric_cast)]
    let old = unsafe {
        SetWindowLongPtrW(
            HWND(hwnd as *mut core::ffi::c_void),
            GWLP_WNDPROC,
            shutdown_wndproc as RawWndProc as isize,
        )
    };
    if old == 0 {
        tracing::warn!("[Shutdown] 主窗口子类化失败,关机落库钩子未安装");
        return;
    }
    OLD_WNDPROC.store(old, Ordering::Release);
}

/// 关机收尾:活跃连接按已完结落盘并等待写线程提交;锁中毒时恢复继续,
/// 不放弃仅剩的落库机会
fn flush_and_wait() {
    let ctx = CTX.lock().unwrap_or_else(|e| e.into_inner());
    let Some(ctx) = ctx.as_ref() else {
        return;
    };
    // SAFETY:指针由 install 在主线程注册,指向 App 字段;窗口过程只在
    // 主线程消息分发中触发,此时 App 存活且无其他借用存在
    let tracker = unsafe { &mut *(ctx.tracker as *mut Tracker) };
    let writer = unsafe { &mut *(ctx.writer as *mut Writer) };
    let events = tracker.flush(unix_now());
    writer.send(events);
    writer.shutdown();
}

unsafe extern "system" fn shutdown_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_ENDSESSION && wparam.0 != 0 {
        flush_and_wait();
    }
    let old = OLD_WNDPROC.load(Ordering::Acquire);
    // old == 0 不可达:成为窗口过程前必然已完成安装
    let prev: RawWndProc = unsafe { std::mem::transmute(old) };
    unsafe { CallWindowProcW(Some(prev), hwnd, msg, wparam, lparam) }
}
