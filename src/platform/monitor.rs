//! 显示器几何与窗口 DWM 属性:多屏工作区查询、透明小窗去系统边框。

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMNCRENDERINGPOLICY, DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
    DWMWA_NCRENDERING_POLICY, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, FindWindowW, GWLP_WNDPROC, GetCursorPos, GetSystemMetrics, GetWindowLongPtrW,
    SM_CXSCREEN, SM_CYSCREEN, SPI_GETWORKAREA, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetWindowLongPtrW, SetWindowPos,
    SystemParametersInfoW, WM_NCCALCSIZE,
};
use windows::core::{HSTRING, PCWSTR};

/// 主屏工作区(物理像素,已排除任务栏)换算为逻辑点;查询失败退回
/// 整屏尺寸(GetSystemMetrics 恒成功,不丢失定位能力)
pub fn workarea_logical() -> (f32, f32) {
    unsafe {
        let sys_scale = GetDpiForSystem() as f32 / 96.0;
        let mut rect = RECT::default();
        let ok = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut rect as *mut _ as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        if ok.is_ok() && rect.right > 0 && rect.bottom > 0 {
            (
                rect.right as f32 / sys_scale,
                rect.bottom as f32 / sys_scale,
            )
        } else {
            (
                GetSystemMetrics(SM_CXSCREEN) as f32 / sys_scale,
                GetSystemMetrics(SM_CYSCREEN) as f32 / sys_scale,
            )
        }
    }
}

/// 逻辑坐标点所在显示器的工作区(逻辑点 left,top,right,bottom;多屏
/// 下副屏坐标可为负);查询失败退回主屏工作区。坐标换算沿用系统 DPI
/// (与 workarea_logical 一致,per-monitor DPI 差异场景未支持)
pub fn workarea_of(x: f32, y: f32) -> (f32, f32, f32, f32) {
    unsafe {
        let sys_scale = GetDpiForSystem() as f32 / 96.0;
        let pt = POINT {
            x: (x * sys_scale).round() as i32,
            y: (y * sys_scale).round() as i32,
        };
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(hmon, &mut info).as_bool() {
            let rc = info.rcWork;
            (
                rc.left as f32 / sys_scale,
                rc.top as f32 / sys_scale,
                rc.right as f32 / sys_scale,
                rc.bottom as f32 / sys_scale,
            )
        } else {
            let (w, h) = workarea_logical();
            (0.0, 0.0, w, h)
        }
    }
}

/// 按标题查找顶层窗口(无边框小窗标题不显示,仅作 FindWindow 定位锚)
pub fn find_window_by_title(title: &str) -> Option<HWND> {
    let title_h = HSTRING::from(title);
    unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title_h.as_ptr())).ok() }
}

/// 当前光标位置(物理像素;查询失败返回 None)
pub fn cursor_pos_physical() -> Option<(i32, i32)> {
    let mut pt = POINT::default();
    unsafe { GetCursorPos(&mut pt).ok().map(|_| (pt.x, pt.y)) }
}

/// 主鼠标按键是否按下(GetAsyncKeyState 最高位;悬浮条窗口收不到
/// 窗口外的鼠标消息,菜单打开期间靠它检测菜单外点击以关闭菜单)
pub fn primary_button_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) < 0 }
}

/// 副鼠标按键(右键)是否按下
pub fn secondary_button_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_RBUTTON};
    unsafe { GetAsyncKeyState(VK_RBUTTON.0 as i32) < 0 }
}

/// 移除窗口的 DWM 系统边框与非客户区渲染。
/// Windows 11 会为圆角窗口绘制 1px 系统边框,在透明窗口上表现为
/// 圆角外圈的一圈方框,需显式禁用;旧系统 BORDER_COLOR 调用失败可忽略
pub fn remove_dwm_frame(hwnd: HWND) {
    unsafe {
        // 禁用非客户区渲染(系统阴影/边框)
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY,
            &DWMNCRP_DISABLED as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<DWMNCRENDERINGPOLICY>() as u32,
        );

        // Win11 22000+:将边框颜色设为 NONE
        let color_none = DWMWA_COLOR_NONE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &color_none as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

/// 悬浮球窗口扩展样式修正:置 WS_EX_TOOLWINDOW(不占任务栏与 Alt-Tab)、
/// 清 WS_EX_APPWINDOW(egui-winit 的 taskbar(false) 在当前 fork 未生效)
pub fn fix_toolwindow_style(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, SetWindowLongPtrW, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let patched = (style & !(WS_EX_APPWINDOW.0 as isize)) | WS_EX_TOOLWINDOW.0 as isize;
        if patched != style {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, patched);
        }
    }
}

/// 悬浮窗窗口样式对齐(火绒同款):winit undecorated 窗口残留
/// WS_CAPTION/WS_SYSMENU/WS_THICKFRAME/WS_BORDER(被 NC 计算隐藏但窗口
/// 身份仍是"有边框窗口",与全屏游戏独立翻转的遮挡让位机制互相作用,
/// 引发点击游戏时黑屏切换);清除后成为纯 WS_POPUP,并置 WS_EX_LAYERED
/// 分层合成(火绒同款——悬浮窗由 DWM 独立合成,不参与游戏独占平面的
/// 让位,可在全屏游戏上显示)。SetLayeredWindowAttributes 保持整窗
/// 不透明,像素级透明仍由渲染表面 alpha 提供
pub fn align_floating_style(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GWL_STYLE, GetWindowLongPtrW, LWA_ALPHA, SetLayeredWindowAttributes,
        SetWindowLongPtrW, WS_BORDER, WS_CAPTION, WS_EX_LAYERED, WS_SYSMENU, WS_THICKFRAME,
    };
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let strip = (WS_CAPTION.0 | WS_SYSMENU.0 | WS_THICKFRAME.0 | WS_BORDER.0) as isize;
        let cleaned = style & !strip;
        if cleaned != style {
            SetWindowLongPtrW(hwnd, GWL_STYLE, cleaned);
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if ex & WS_EX_LAYERED.0 as isize == 0 {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as isize);
            // LAYERED 需一次性设置属性,否则窗口不渲染
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
        }
    }
}

/// 窗口过程裸函数指针(旧过程地址与子类过程共用该形态)
type RawWndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

/// 无边框窗口系统拖动:释放鼠标捕获后发送 SC_MOUSEMOVE 进入系统
/// 模态拖动循环,松手后 SendMessage 返回。不依赖窗口焦点——
/// egui-winit 的 StartDrag 有 has_focus 门控,悬浮条未被点击激活时
/// 永远无焦点,拖动命令会被静默忽略
pub fn begin_system_drag(hwnd: HWND) {
    use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
    use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_SYSCOMMAND};
    // SC_MOUSEMOVE = 0xF012,进入系统模态拖动循环(windows crate 未提供该常量)
    const SC_MOUSEMOVE: usize = 0xF012;
    unsafe {
        let _ = ReleaseCapture();
        SendMessageW(
            hwnd,
            WM_SYSCOMMAND,
            Some(WPARAM(SC_MOUSEMOVE)),
            Some(LPARAM(0)),
        );
    }
}

/// 立即隐藏窗口(用于拖动开始时同步隐藏浮窗:系统模态拖动循环期间
/// 不产帧,等 viewport 按帧对账显隐会晚一拍);幂等
pub fn hide_window(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

/// 悬浮球窗口原过程地址;0 = 未安装
static BALL_OLD_WNDPROC: AtomicIsize = AtomicIsize::new(0);

/// 子类化悬浮球窗口,接管 WM_NCCALCSIZE 使客户区覆盖整个窗口矩形。
/// egui-winit 对 decorations(false) 的窗口写死 undecorated shadow,winit
/// 在 WM_NCCALCSIZE 中把客户区顶部下移 1px 以保留 DWM 阴影,DWM 便在窗口
/// 顶边画出一条 1px 边框线,透明窗口上恒可见,DWM 边框颜色属性无法去除;
/// 该窗口不最大化也不参与系统缩放,客户区=全窗口即可消除顶部非客户区。
/// 幂等:过程已替换时不重复安装;1s 节流重查兼容窗口重建。
pub fn subclass_full_client(hwnd: HWND) {
    // SetWindowLongPtrW 的过程地址参数是 isize,fn 指针数值化是子类化固有操作
    #[allow(clippy::fn_to_numeric_cast)]
    let new_proc = full_client_wndproc as RawWndProc as isize;
    let cur = unsafe { GetWindowLongPtrW(hwnd, GWLP_WNDPROC) };
    if cur == new_proc {
        return;
    }
    let old = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, new_proc) };
    if old == 0 {
        return;
    }
    BALL_OLD_WNDPROC.store(old, Ordering::Release);
    // 通知系统按新 NC 计算重排客户区,否则 1px 顶边残留到下次尺寸变化
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

/// 悬浮球窗口过程:wparam != 0 的 WM_NCCALCSIZE 不修改 rgrc[0] 并返回 0
/// (客户区=整个窗口),其余消息全部透传原过程
unsafe extern "system" fn full_client_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCALCSIZE && wparam.0 != 0 {
        return LRESULT(0);
    }
    let old = BALL_OLD_WNDPROC.load(Ordering::Acquire);
    // old == 0 不可达:成为窗口过程前必然已完成安装
    let prev: RawWndProc = unsafe { std::mem::transmute(old) };
    unsafe { CallWindowProcW(Some(prev), hwnd, msg, wparam, lparam) }
}
