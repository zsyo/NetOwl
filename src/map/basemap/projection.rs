//! 地图视图与投影:中心经纬度/缩放的动画视图状态,等距圆柱投影
//! (含经度周期副本的平移与可见副本计算)。

use eframe::egui;
use egui::{Pos2, Rect};

/// 底图数据的纬度窗口:窗口外无陆地/标签数据,视口钳制不越出该范围
pub const FIT_MIN_LAT: f32 = -58.0;
pub const FIT_MAX_LAT: f32 = 84.0;
/// 全局适配视图的纬度窗口高度(不显示南极)
const FIT_LAT_SPAN: f32 = FIT_MAX_LAT - FIT_MIN_LAT;

/// 视图:中心经纬度与缩放倍数(1.0 = 全局适配)。
/// target_* 为滚轮/双击设置的动画目标,每帧向其平滑趋近;拖拽即时生效。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub center_lon: f32,
    pub center_lat: f32,
    pub zoom: f32,
    pub target_lon: f32,
    pub target_lat: f32,
    pub target_zoom: f32,
}

impl View {
    /// 全局适配视图:经度全宽,纬度窗口中心约 +13 度
    pub fn global() -> Self {
        View {
            center_lon: 0.0,
            center_lat: 13.0,
            zoom: 1.0,
            target_lon: 0.0,
            target_lat: 13.0,
            target_zoom: 1.0,
        }
    }

    /// 指定中心与缩放的视图(target 同值,瞬跳无动画)。
    /// 启动定位与双击复位等"视图整体切换"用瞬跳:若走 target 动画,
    /// 动画期间 clamp_view 会以放大途中(小 zoom)的大纬度半窗钳制
    /// target_lat,高纬目标会在途中被夹低,终点失准
    pub fn at(lon: f32, lat: f32, zoom: f32) -> Self {
        View {
            center_lon: lon,
            center_lat: lat,
            zoom,
            target_lon: lon,
            target_lat: lat,
            target_zoom: zoom,
        }
    }

    /// 向动画目标趋近(k 为每帧插值系数)
    pub fn animate(&mut self, k: f32) {
        self.center_lon += (self.target_lon - self.center_lon) * k;
        self.center_lat += (self.target_lat - self.center_lat) * k;
        self.zoom += (self.target_zoom - self.zoom) * k;
    }
}

impl Default for View {
    fn default() -> Self {
        View::global()
    }
}

/// 等距圆柱投影:像素/度线性映射,画布中心对应视图中心。
/// 经度方向以 360 度为周期无缝平铺(wrap-around),世界可按副本循环绘制
#[derive(Clone, Copy)]
pub struct Projection {
    center: Pos2,
    pub center_lon: f32,
    pub center_lat: f32,
    pub ppd: f32,
    pub zoom: f32,
}

impl Projection {
    /// 画布的全局适配基准:像素/度(未乘缩放倍数)
    pub fn fit_ppd(rect: Rect) -> f32 {
        (rect.width() / 360.0).min(rect.height() / FIT_LAT_SPAN)
    }

    pub fn new(rect: Rect, view: View) -> Self {
        Projection {
            center: rect.center(),
            center_lon: view.center_lon,
            center_lat: view.center_lat,
            ppd: Self::fit_ppd(rect) * view.zoom,
            zoom: view.zoom,
        }
    }

    pub fn project(&self, lon: f32, lat: f32) -> Pos2 {
        Pos2::new(
            self.center.x + (lon - self.center_lon) * self.ppd,
            self.center.y - (lat - self.center_lat) * self.ppd,
        )
    }

    /// 屏幕坐标 -> 经纬度(缩放锚点与视口范围计算用)
    pub fn unproject(&self, p: Pos2) -> (f32, f32) {
        (
            self.center_lon + (p.x - self.center.x) / self.ppd,
            self.center_lat - (p.y - self.center.y) / self.ppd,
        )
    }

    /// 世界横向周期宽度(像素)
    pub fn cycle_px(&self) -> f32 {
        360.0 * self.ppd
    }

    /// 水平平移 n 个世界周期的投影副本:等距圆柱下副本即纯平移,
    /// project/unproject 在副本坐标系内保持互逆
    pub fn shifted(&self, n: i32) -> Projection {
        let mut p = *self;
        p.center_lon -= n as f32 * 360.0;
        p
    }

    /// 屏幕坐标 x 的点可见的世界副本序号区间(边界多画一个,不可见副本由裁剪兜底)
    pub fn visible_cycles(&self, x: f32, rect: Rect) -> (i32, i32) {
        let w = self.cycle_px();
        (
            ((rect.left() - x) / w).floor() as i32,
            ((rect.right() - x) / w).floor() as i32,
        )
    }
}
