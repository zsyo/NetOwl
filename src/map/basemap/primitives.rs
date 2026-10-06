//! 底图几何图元:视口剔除线段/三角形与固定屏幕线宽 quad 压入;
//! 海岸/国界/河流/十段线共用,布局层在 mod。

use std::sync::Arc;

use eframe::egui;
use egui::epaint::{Vertex, WHITE_UV};
use egui::{Color32, Mesh, Pos2, Rect, Shape, Vec2};

use super::Projection;
use crate::ui::theme;

/// 河流线宽(屏幕像素)
pub(super) const RIVER_WIDTH: f32 = 1.1;

/// 段级粗剔除(端点均在同一外侧才跳过)后压入固定屏幕线宽 quad
pub(super) fn push_culled_segment(
    mesh: &mut Mesh,
    rect: Rect,
    proj: &Projection,
    a: (f32, f32),
    b: (f32, f32),
    half: f32,
    color: Color32,
) {
    let (la, ta) = a;
    let (lb, tb) = b;
    let (min_lon, max_lon) = (la.min(lb), la.max(lb));
    let (min_lat, max_lat) = (ta.min(tb), ta.max(tb));
    let tl = proj.project(min_lon, max_lat);
    let br = proj.project(max_lon, min_lat);
    if br.x < rect.left() || tl.x > rect.right() || br.y < rect.top() || tl.y > rect.bottom() {
        return;
    }
    push_segment(
        mesh,
        proj.project(la, ta),
        proj.project(lb, tb),
        half,
        color,
    );
}

/// 主要河流:固定屏幕线宽折线(数据已按档抽稀),逐段视口剔除;
/// 绘制于世界/中国层之上,不受中国层陆地填充覆盖
pub(super) fn draw_rivers(painter: &egui::Painter, rect: Rect, proj: &Projection, lines: &[Vec<(f32, f32)>]) {
    let mut mesh = Mesh::default();
    let half = RIVER_WIDTH * 0.5;
    for line in lines {
        for pair in line.windows(2) {
            push_culled_segment(
                &mut mesh,
                rect,
                proj,
                pair[0],
                pair[1],
                half,
                theme::c().map_river,
            );
        }
    }
    painter.add(Shape::Mesh(Arc::new(mesh)));
}

/// 南海断续国界(十段线):独立线宽与颜色,不与海岸/国界混同
pub(super) fn draw_south_sea_line(painter: &egui::Painter, rect: Rect, proj: &Projection, segs: &[[f32; 4]]) {
    let mut mesh = Mesh::default();
    for &[lon0, lat0, lon1, lat1] in segs {
        let (a, b) = (proj.project(lon0, lat0), proj.project(lon1, lat1));
        let (min_x, max_x) = (a.x.min(b.x), a.x.max(b.x));
        let (min_y, max_y) = (a.y.min(b.y), a.y.max(b.y));
        if max_x < rect.left()
            || min_x > rect.right()
            || max_y < rect.top()
            || min_y > rect.bottom()
        {
            continue;
        }
        push_segment(&mut mesh, a, b, 1.0, theme::c().map_south_sea_line);
    }
    painter.add(Shape::Mesh(Arc::new(mesh)));
}

pub(super) fn tri_visible(a: Pos2, b: Pos2, c: Pos2, rect: Rect) -> bool {
    let (min_x, max_x) = (a.x.min(b.x).min(c.x), a.x.max(b.x).max(c.x));
    let (min_y, max_y) = (a.y.min(b.y).min(c.y), a.y.max(b.y).max(c.y));
    max_x >= rect.left() && min_x <= rect.right() && max_y >= rect.top() && min_y <= rect.bottom()
}

pub(super) fn push_tri(mesh: &mut Mesh, a: Pos2, b: Pos2, c: Pos2, color: Color32) {
    let base = mesh.vertices.len() as u32;
    for p in [a, b, c] {
        mesh.vertices.push(Vertex {
            pos: p,
            uv: WHITE_UV,
            color,
        });
    }
    mesh.indices.extend_from_slice(&[base, base + 1, base + 2]);
}

fn push_segment(mesh: &mut Mesh, a: Pos2, b: Pos2, half: f32, color: Color32) {
    let d = b - a;
    let len = d.length();
    if len < 1e-3 {
        return;
    }
    let n = Vec2::new(-d.y / len, d.x / len) * half;
    let base = mesh.vertices.len() as u32;
    for p in [a - n, a + n, b + n, b - n] {
        mesh.vertices.push(Vertex {
            pos: p,
            uv: WHITE_UV,
            color,
        });
    }
    mesh.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}
