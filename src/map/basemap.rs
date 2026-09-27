//! 地图底图:视图/投影、经纬网格、陆地填充、洞环、海岸线、国界、
//! 主要河流与国家/海洋名称标签。
//!
//! 每帧按视口即时构建几何(无缓存):环级包围盒剔除 + 三角形/线段
//! 视口剔除,保证任意缩放级别下每帧只处理可见部分。名称标签按缩放
//! 分级显隐,不随底图缩放变形;节点与飞线在屏幕坐标系绘制(见 map.rs)。

use std::sync::Arc;

use eframe::egui;
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Shape, Vec2};

use crate::map::world::{LabelKind, MapLabel, MapLevel, RingKind, map_data};
use crate::ui::theme;

/// 放大超过该倍数后切换到 50m 精细档
const DETAIL_ZOOM: f32 = 3.0;
/// 海岸线线宽(屏幕像素)
const COAST_WIDTH: f32 = 1.2;
/// 国界线宽(屏幕像素)
const BORDER_WIDTH: f32 = 0.8;
/// 河流线宽(屏幕像素)
const RIVER_WIDTH: f32 = 1.1;
/// 全局适配视图的纬度窗口高度(不显示南极)
const FIT_LAT_SPAN: f32 = 142.0;

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

/// 绘制完整底图:海洋底色、网格、世界层(NE)、中国层(DataV 覆盖)、
/// 十段线与名称标签。中国层陆地填充盖住 NE 中伸入中国境内的邻国
/// 国界线与误划几何,中国边界与省界画在最顶层
pub fn draw(painter: &egui::Painter, rect: Rect, proj: &Projection, labels_zh: bool) {
    painter.rect_filled(
        rect,
        CornerRadius::same(theme::RADIUS_LG),
        theme::c().bg_map,
    );
    draw_grid(painter, rect, proj);

    let data = map_data();
    let idx = if proj.zoom >= DETAIL_ZOOM { 1 } else { 0 };

    // 经度方向 wrap:数据落在 [-180,180],按可见经度窗口求覆盖的世界副本,
    // 每个副本(平移 k*360 度)完整走一遍层序;副本屏幕区间互不重叠,
    // 不可见部分由环级/段级/标签剔除自然过滤
    let (lon_l, _) = proj.unproject(rect.left_top());
    let (lon_r, _) = proj.unproject(rect.right_bottom());
    let k_min = ((lon_l.min(lon_r) + 180.0) / 360.0).floor() as i32;
    let k_max = ((lon_r.max(lon_l) + 180.0) / 360.0).floor() as i32;
    for k in k_min..=k_max {
        let p = proj.shifted(k);
        draw_level(painter, rect, &p, &data.world[idx]);

        // 中国层:陆地填充 + 海岸/国界,整体覆盖世界层之上
        draw_level(painter, rect, &p, &data.china[idx]);

        // 河流独立于两层之后绘制:中国层陆地填充会盖住层内线条
        draw_rivers(painter, rect, &p, &data.rivers[idx]);

        draw_south_sea_line(painter, rect, &p, &data.south_sea_line);
        draw_labels(painter, rect, &p, &data.labels, labels_zh);
    }
}

/// 单层底图:陆地与洞环填充、海岸线与国界(共享边检测在解码期完成)
fn draw_level(painter: &egui::Painter, rect: Rect, proj: &Projection, level: &MapLevel) {
    let mut land = Mesh::default();
    let mut holes = Mesh::default();
    add_fill(level, &mut land, &mut holes, rect, proj);
    painter.add(Shape::Mesh(Arc::new(land)));
    painter.add(Shape::Mesh(Arc::new(holes)));

    let mut coast = Mesh::default();
    let mut border = Mesh::default();
    add_lines(level, &mut coast, &mut border, rect, proj);
    painter.add(Shape::Mesh(Arc::new(border)));
    painter.add(Shape::Mesh(Arc::new(coast)));
}

/// 经纬网格:按视口范围绘制,放大后加密;
/// 经线随 wrap 自由跨周期,纬线端点延伸到可见经度窗口
fn draw_grid(painter: &egui::Painter, rect: Rect, proj: &Projection) {
    let (lon0, lat0) = proj.unproject(rect.left_top());
    let (lon1, lat1) = proj.unproject(rect.right_bottom());
    let (lon0, lon1) = (lon0.min(lon1), lon0.max(lon1));
    let (lat0, lat1) = (lat0.min(lat1), lat0.max(lat1));

    let step: i32 = if proj.zoom >= DETAIL_ZOOM { 10 } else { 30 };
    let first = (lon0.floor() as i32).div_euclid(step) * step;
    for lon in (first..=lon1.ceil() as i32).step_by(step as usize) {
        let lon = lon as f32;
        painter.line_segment(
            [proj.project(lon, 90.0), proj.project(lon, -90.0)],
            egui::Stroke::new(1.0, theme::c().map_grid),
        );
    }
    let first = (lat0.floor() as i32).div_euclid(step) * step;
    for lat in (first..=lat1.ceil() as i32).step_by(step as usize) {
        let lat = (lat as f32).clamp(-90.0, 90.0);
        painter.line_segment(
            [proj.project(lon0, lat), proj.project(lon1, lat)],
            egui::Stroke::new(1.0, theme::c().map_grid),
        );
    }
}

/// 陆地与洞环填充:环级包围盒剔除 + 三角形视口剔除
fn add_fill(level: &MapLevel, land: &mut Mesh, holes: &mut Mesh, rect: Rect, proj: &Projection) {
    let mut scratch: Vec<Pos2> = Vec::with_capacity(256);
    for ring in &level.rings {
        let tl = proj.project(ring.min_lon, ring.max_lat);
        let br = proj.project(ring.max_lon, ring.min_lat);
        if !Rect::from_two_pos(tl, br).intersects(rect) {
            continue;
        }
        let idxs = &level.ring_indices[ring.start as usize..(ring.start + ring.len) as usize];
        scratch.clear();
        scratch.extend(idxs.iter().map(|&i| {
            let (lon, lat) = level.verts[i as usize];
            proj.project(lon, lat)
        }));

        let tris = if ring.kind == RingKind::Land {
            &level.tris[ring.tri_start as usize..(ring.tri_start + ring.tri_len) as usize]
        } else {
            &level.hole_tris[ring.tri_start as usize..(ring.tri_start + ring.tri_len) as usize]
        };
        let color = if ring.kind == RingKind::Land {
            theme::c().map_land
        } else {
            theme::c().bg_map
        };
        let target = if ring.kind == RingKind::Land {
            &mut *land
        } else {
            &mut *holes
        };
        for &[a, b, c] in tris {
            let (p0, p1, p2) = (
                scratch[a as usize],
                scratch[b as usize],
                scratch[c as usize],
            );
            if !tri_visible(p0, p1, p2, rect) {
                continue;
            }
            push_tri(target, p0, p1, p2, color);
        }
    }
}

/// 海岸线与国界:固定屏幕线宽的 quad 段
fn add_lines(level: &MapLevel, coast: &mut Mesh, border: &mut Mesh, rect: Rect, proj: &Projection) {
    draw_line_set(
        level,
        &level.coast,
        COAST_WIDTH,
        theme::c().map_coast,
        coast,
        rect,
        proj,
    );
    draw_line_set(
        level,
        &level.border,
        BORDER_WIDTH,
        theme::c().map_border,
        border,
        rect,
        proj,
    );
}

fn draw_line_set(
    level: &MapLevel,
    segs: &[[u32; 2]],
    width: f32,
    color: Color32,
    mesh: &mut Mesh,
    rect: Rect,
    proj: &Projection,
) {
    let half = width * 0.5;
    for &[a, b] in segs {
        let (la, ta) = level.verts[a as usize];
        let (lb, tb) = level.verts[b as usize];
        push_culled_segment(mesh, rect, proj, (la, ta), (lb, tb), half, color);
    }
}

/// 段级粗剔除(端点均在同一外侧才跳过)后压入固定屏幕线宽 quad
fn push_culled_segment(
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
fn draw_rivers(painter: &egui::Painter, rect: Rect, proj: &Projection, lines: &[Vec<(f32, f32)>]) {
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

/// 名称标签:按缩放分级显隐(<1.6x 只显示 rank0,>3.5x 全部),
/// 国家名英文大写加字距,海洋名蓝色,省名(中国)中性色小一号
fn draw_labels(
    painter: &egui::Painter,
    rect: Rect,
    proj: &Projection,
    labels: &[MapLabel],
    labels_zh: bool,
) {
    let max_rank = if proj.zoom < 1.6 {
        0
    } else if proj.zoom < 3.5 {
        1
    } else {
        2
    };
    for label in labels {
        if label.rank > max_rank {
            continue;
        }
        let pos = proj.project(label.lon, label.lat);
        if !rect.contains(pos) {
            continue;
        }
        let (text, color, size) = match label.kind {
            LabelKind::Country => {
                let name = if labels_zh {
                    &label.name_zh
                } else {
                    &label.name_en
                };
                (name.clone(), theme::c().map_label_country, 11.0)
            }
            LabelKind::Sea => {
                let name = if labels_zh {
                    &label.name_zh
                } else {
                    &label.name_en
                };
                (name.clone(), theme::c().map_label_sea, 10.0)
            }
            // 省名与国家名同字号逻辑但用弱化色,不与国家名争视觉层级
            LabelKind::Province => {
                let name = if labels_zh {
                    &label.name_zh
                } else {
                    &label.name_en
                };
                (name.clone(), theme::c().map_label_province, 10.0)
            }
        };
        if label.kind == LabelKind::Country && !labels_zh {
            draw_spaced_upper(painter, pos, &text, size, color);
        } else {
            painter.text(
                pos,
                Align2::CENTER_CENTER,
                &text,
                FontId::proportional(size),
                color,
            );
        }
    }
}

/// 南海断续国界(十段线):独立线宽与颜色,不与海岸/国界混同
fn draw_south_sea_line(painter: &egui::Painter, rect: Rect, proj: &Projection, segs: &[[f32; 4]]) {
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

/// 英文国家名:大写 + 字距(逐字符绘制,egui 无字距 API)
fn draw_spaced_upper(painter: &egui::Painter, center: Pos2, text: &str, size: f32, color: Color32) {
    const SPACING: f32 = 1.5;
    let upper = text.to_uppercase();
    let font = FontId::proportional(size);
    let width = |s: &str| {
        painter
            .layout_no_wrap(s.to_owned(), font.clone(), color)
            .rect
            .width()
    };
    let chars: Vec<char> = upper.chars().collect();
    let total: f32 = chars.iter().map(|c| width(&c.to_string())).sum::<f32>()
        + SPACING * (chars.len() - 1) as f32;
    let mut x = center.x - total * 0.5;
    for c in chars {
        let s = c.to_string();
        let cw = width(&s);
        painter.text(
            Pos2::new(x, center.y),
            Align2::LEFT_CENTER,
            &s,
            font.clone(),
            color,
        );
        x += cw + SPACING;
    }
}

fn tri_visible(a: Pos2, b: Pos2, c: Pos2, rect: Rect) -> bool {
    let (min_x, max_x) = (a.x.min(b.x).min(c.x), a.x.max(b.x).max(c.x));
    let (min_y, max_y) = (a.y.min(b.y).min(c.y), a.y.max(b.y).max(c.y));
    max_x >= rect.left() && min_x <= rect.right() && max_y >= rect.top() && min_y <= rect.bottom()
}

fn push_tri(mesh: &mut Mesh, a: Pos2, b: Pos2, c: Pos2, color: Color32) {
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
