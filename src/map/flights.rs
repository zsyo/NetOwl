//! 流量飞线:贝塞尔连线(远端归属+主导方向聚合,一城一线)与流动
//! 粒子尾迹,按可见世界副本平移铺开,跨太平洋走最短方向短弧。

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use eframe::egui;
use egui::{Color32, Mesh, Pos2, Rect, Shape, Vec2};

use super::Projection;
use crate::model::Connection;
use crate::net::geoip;
use crate::ui::theme;

/// 二次贝塞尔取点
fn bezier(p0: Pos2, ctrl: Pos2, p1: Pos2, t: f32) -> Pos2 {
    let u = 1.0 - t;
    Pos2::new(
        u * u * p0.x + 2.0 * u * t * ctrl.x + t * t * p1.x,
        u * u * p0.y + 2.0 * u * t * ctrl.y + t * t * p1.y,
    )
}

/// 弧线控制点:中点沿法线抬升,距离越远弧越高
fn arc_ctrl(p0: Pos2, p1: Pos2) -> Pos2 {
    let mid = Pos2::new((p0.x + p1.x) * 0.5, (p0.y + p1.y) * 0.5);
    let d = p1 - p0;
    let len = d.length().max(1.0);
    let lift = (len * 0.18).clamp(8.0, 90.0);
    mid + Vec2::new(-d.y, d.x) / len * lift
}

/// 经度差归一到 (-180, 180]:连线取最短方向,跨太平洋不再横穿大陆
fn wrap_delta(d: f32) -> f32 {
    d - 360.0 * (d / 360.0).round()
}

/// 绘制聚合连线与粒子(LOD:连接数不再直接放大绘制量,数百连接下
/// 仍是一城一线;线宽随聚合数增长表达流量规模)
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_flights(
    painter: &egui::Painter,
    rect: Rect,
    proj: &Projection,
    conns: &[Connection],
    local: Pos2,
    local_pos: (f32, f32),
    cycle_px: f32,
    hover_pos: Option<Pos2>,
    selected: Option<crate::model::Place>,
    t: f32,
) {
    // 连线按 远端归属+主导方向 聚合绘制(LOD):连接数不再直接放大绘制量,
    // 数百连接下仍是一城一线;线宽随聚合数增长表达流量规模
    let mut lines: BTreeMap<(crate::model::Place, bool), (usize, u64)> = BTreeMap::new();
    for c in conns {
        let Some(place) = c.city else { continue };
        let entry = lines.entry((place, c.inbound_dominant())).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += c.total_bytes();
    }
    for ((place, inbound), (count, _)) in &lines {
        // 节点端取最短方向等效经度(跨太平洋走短弧),主几何按可见世界副本
        // 平移铺开,屏幕边缘两侧由相邻副本自然接续
        let (place_lon, place_lat) = geoip::place_pos(*place);
        let end_lon = local_pos.0 + wrap_delta(place_lon - local_pos.0);
        let end = proj.project(end_lon, place_lat);
        let (start, end) = if *inbound { (end, local) } else { (local, end) };
        let ctrl = arc_ctrl(start, end);
        let color = if *inbound {
            theme::c().inbound
        } else {
            theme::c().outbound
        };
        let hovered = hover_pos.is_some_and(|h| super::wrap_dist(h, end, cycle_px) < 20.0);
        let dim = selected.is_some_and(|sel| *place != sel);
        let width = 1.4
            + 1.1 * (*count as f32 - 1.0).sqrt().min(2.0)
            + if hovered && !dim { 0.6 } else { 0.0 };
        // 选中端点联动:未选中端点的连线(含粒子)整体淡化
        let line_alpha = if dim {
            0.12
        } else if hovered {
            0.9
        } else {
            0.45
        };
        // 曲线横向 bbox 为端点包围盒(控制点 x 居中),据此求可见副本区间
        let (min_x, max_x) = (start.x.min(end.x), start.x.max(end.x));
        let k0 = ((rect.left() - max_x) / cycle_px).floor() as i32;
        let k1 = ((rect.right() - min_x) / cycle_px).floor() as i32;
        // 粒子与尾迹(主副本算一次,副本平移);相位按聚合键派生保持稳定
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        place.hash(&mut hasher);
        inbound.hash(&mut hasher);
        let phase = (t * 0.22 + super::hash_phase(hasher.finish())) % 1.0;
        let trail: Vec<Pos2> = (0..5u32)
            .map(|k| bezier(start, ctrl, end, (phase - 0.025 * k as f32).rem_euclid(1.0)))
            .collect();
        let arc = gradient_arc_mesh(start, ctrl, end, color, width, line_alpha, 16);
        for k in k0..=k1 {
            let off = Vec2::new(k as f32 * cycle_px, 0.0);
            let mut m = arc.clone();
            m.translate(off);
            painter.add(Shape::Mesh(Arc::new(m)));
            for (i, pt) in trail.iter().enumerate() {
                let a = line_alpha * (1.0 - i as f32 * 0.2);
                // 粒子辉光层 + 实心(头部最大最亮,拖尾渐隐)
                painter.circle_filled(
                    *pt + off,
                    4.6 - 0.6 * i as f32,
                    color.gamma_multiply(a * 0.25),
                );
                painter.circle_filled(*pt + off, 2.8 - 0.5 * i as f32, color.gamma_multiply(a));
            }
        }
    }
}

/// 渐变弧线 mesh:贝塞尔采样 n 段 quad,顶点色沿线从尾淡(信号出发端
/// 6% 亮度)到头亮(到达端),替代单色描边
fn gradient_arc_mesh(
    start: Pos2,
    ctrl: Pos2,
    end: Pos2,
    color: Color32,
    width: f32,
    alpha: f32,
    n: usize,
) -> Mesh {
    let mut mesh = Mesh::default();
    let mut prev = bezier(start, ctrl, end, 0.0);
    for i in 1..=n {
        let t0 = (i - 1) as f32 / n as f32;
        let t1 = i as f32 / n as f32;
        let pt = bezier(start, ctrl, end, t1);
        let a0 = alpha * (0.06 + 0.94 * t0);
        let a1 = alpha * (0.06 + 0.94 * t1);
        let dir = (pt - prev).normalized() * (width * 0.5);
        let nrm = Vec2::new(-dir.y, dir.x);
        let idx = mesh.vertices.len() as u32;
        mesh.colored_vertex(prev - nrm, color.gamma_multiply(a0));
        mesh.colored_vertex(prev + nrm, color.gamma_multiply(a0));
        mesh.colored_vertex(pt + nrm, color.gamma_multiply(a1));
        mesh.colored_vertex(pt - nrm, color.gamma_multiply(a1));
        mesh.add_triangle(idx, idx + 1, idx + 2);
        mesh.add_triangle(idx, idx + 2, idx + 3);
        prev = pt;
    }
    mesh
}
