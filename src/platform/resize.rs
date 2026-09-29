//! 无边框窗口边缘缩放:winit 0.30 对 undecorated 窗口不做边缘 hit-test
//! (事件循环无 WM_NCHITTEST 边缘分支),由本模块自实现——
//! egui 侧提供热区命中判定,拖拽经 SetWindowPos 逐帧跟随(不走系统
//! SC_SIZE 模态循环:模态期间 egui 收不到鼠标释放事件,交互状态会粘滞)。

use eframe::egui;
use egui::{CursorIcon, Pos2, Rect};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowRect, SET_WINDOW_POS_FLAGS, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
};

/// 边缘热区宽度(逻辑点)
pub const EDGE_PX: f32 = 8.0;

/// 拖拽缩放方向(对应 WM_NCHITTEST 边缘语义)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeHit {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl EdgeHit {
    /// 热区对应的光标形状
    pub fn cursor_icon(self) -> CursorIcon {
        match self {
            EdgeHit::Left | EdgeHit::Right => CursorIcon::ResizeHorizontal,
            EdgeHit::Top | EdgeHit::Bottom => CursorIcon::ResizeVertical,
            EdgeHit::TopLeft | EdgeHit::BottomRight => CursorIcon::ResizeNwSe,
            EdgeHit::TopRight | EdgeHit::BottomLeft => CursorIcon::ResizeNeSw,
        }
    }
}

/// 热区命中判定(pos/rect 均为 egui 逻辑坐标,rect = 窗口客户区)
pub fn edge_hit_test(pos: Pos2, rect: Rect) -> Option<EdgeHit> {
    if !rect.contains(pos) {
        return None;
    }
    let left = pos.x < rect.left() + EDGE_PX;
    let right = pos.x > rect.right() - EDGE_PX;
    let top = pos.y < rect.top() + EDGE_PX;
    let bottom = pos.y > rect.bottom() - EDGE_PX;
    match (left, right, top, bottom) {
        (true, _, true, _) => Some(EdgeHit::TopLeft),
        (true, _, _, true) => Some(EdgeHit::BottomLeft),
        (_, true, true, _) => Some(EdgeHit::TopRight),
        (_, true, _, true) => Some(EdgeHit::BottomRight),
        (true, _, _, _) => Some(EdgeHit::Left),
        (_, true, _, _) => Some(EdgeHit::Right),
        (_, _, true, _) => Some(EdgeHit::Top),
        (_, _, _, true) => Some(EdgeHit::Bottom),
        _ => None,
    }
}

/// 拖拽缩放状态机:按下记录起始光标/窗口矩形,按住期间逐帧
/// SetWindowPos 跟随(手动钳制最小尺寸),松开结束
#[derive(Default)]
pub struct DragResize {
    active: Option<EdgeHit>,
    start_cursor: POINT,
    start_rect: RECT,
}

impl DragResize {
    /// 进行中的缩放方向(光标形状用,进行中即使指针离开热区也保持)
    pub fn active(&self) -> Option<EdgeHit> {
        self.active
    }

    /// 每帧驱动:hit = 当前热区命中;pressed/held = 主键按下沿/持续按住;
    /// min_size 为逻辑点最小窗口尺寸;ppp = 当前缩放比
    pub fn update(
        &mut self,
        hwnd: isize,
        hit: Option<EdgeHit>,
        pressed: bool,
        held: bool,
        min_size: (f32, f32),
        ppp: f32,
    ) {
        if hwnd == 0 {
            return;
        }
        let hwnd = HWND(hwnd as *mut core::ffi::c_void);
        if self.active.is_none() {
            if pressed && let Some(hit) = hit {
                let mut cursor = POINT::default();
                // 光标/窗口矩形查询失败时放弃本次缩放,不粘滞状态
                if unsafe { GetCursorPos(&mut cursor) }.is_ok()
                    && unsafe { GetWindowRect(hwnd, &mut self.start_rect) }.is_ok()
                {
                    self.start_cursor = cursor;
                    self.active = Some(hit);
                }
            }
            return;
        } // 进行中:松开即结束
        if !held {
            self.active = None;
            return;
        }
        let Some(hit) = self.active else {
            return;
        };
        let mut cursor = POINT::default();
        if unsafe { GetCursorPos(&mut cursor) }.is_err() {
            return;
        }
        let dx = cursor.x - self.start_cursor.x;
        let dy = cursor.y - self.start_cursor.y;
        let r = self.start_rect;
        let mut left = r.left;
        let mut top = r.top;
        let mut right = r.right;
        let mut bottom = r.bottom;
        let min_w = (min_size.0 * ppp) as i32;
        let min_h = (min_size.1 * ppp) as i32;
        // 固定对侧边,移动命中侧;SetWindowPos 不走 WM_GETMINMAXINFO,
        // 最小尺寸在此手动钳制
        if matches!(hit, EdgeHit::Left | EdgeHit::TopLeft | EdgeHit::BottomLeft) {
            left = (r.left + dx).at_most(r.right - min_w);
        }
        if matches!(
            hit,
            EdgeHit::Right | EdgeHit::TopRight | EdgeHit::BottomRight
        ) {
            right = (r.right + dx).at_least(r.left + min_w);
        }
        if matches!(hit, EdgeHit::Top | EdgeHit::TopLeft | EdgeHit::TopRight) {
            top = (r.top + dy).at_most(r.bottom - min_h);
        }
        if matches!(
            hit,
            EdgeHit::Bottom | EdgeHit::BottomLeft | EdgeHit::BottomRight
        ) {
            bottom = (r.bottom + dy).at_least(r.top + min_h);
        }
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                left,
                top,
                right - left,
                bottom - top,
                SET_WINDOW_POS_FLAGS(SWP_NOZORDER.0 | SWP_NOACTIVATE.0),
            );
        }
    }
}

trait AtLeast {
    fn at_least(self, min: i32) -> i32;
    fn at_most(self, max: i32) -> i32;
}
impl AtLeast for i32 {
    fn at_least(self, min: i32) -> i32 {
        self.max(min)
    }
    fn at_most(self, max: i32) -> i32 {
        self.min(max)
    }
}
