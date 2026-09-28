//! 流量地图画布:egui painter 自绘(底图渲染见 basemap,悬停信息卡见
//! card,此处负责贝塞尔连线、流动粒子与节点聚合)。

pub mod basemap;
mod card;
pub mod triangulate;
pub mod world;

use std::collections::BTreeMap;

use std::collections::HashMap;

use std::hash::{Hash, Hasher};

use eframe::egui;
use egui::epaint::QuadraticBezierShape;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, TextureHandle, Vec2};

use self::basemap::{Projection, View};
use crate::i18n::I18n;
use crate::model::{Connection, Place};
use crate::net::geoip;
use crate::net::rdns;
use crate::ui::theme;

/// 视图动画趋近系数(30fps 下约 0.12s 收敛)
const ANIM_K: f32 = 0.22;
/// 缩放上限:12 倍时 1px 约对应 0.03 度,50m 档数据仍平滑
const ZOOM_MAX: f32 = 12.0;

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

/// 鼠标到点的最短距离:x 差对世界周期取模,任意 wrap 副本均可命中
fn wrap_dist(h: Pos2, pos: Pos2, cycle_px: f32) -> f32 {
    let dx = (h.x - pos.x).rem_euclid(cycle_px);
    let dx = dx.min(cycle_px - dx);
    let dy = h.y - pos.y;
    (dx * dx + dy * dy).sqrt()
}

/// 由 id/字节数派生稳定的 [0,1) 相位偏移,让粒子/脉冲错落
fn hash_phase(seed: u64) -> f32 {
    ((seed as f32 * 0.618_034) % 1.0).abs()
}

/// 归属节点聚合:连接数与累计流量
type Agg = BTreeMap<Place, (usize, u64)>;

pub fn draw(
    ui: &mut egui::Ui,
    conns: &[Connection],
    i18n: &I18n,
    view: &mut View,
    rdns: &rdns::Rdns,
    icon_tex: &HashMap<String, Option<TextureHandle>>,
    local_pos: (f32, f32),
) {
    ui.horizontal(|ui| {
        ui.heading(theme::accent_text(&i18n.t("map-title"), 20.0));
        ui.label(theme::dim_text(&i18n.t("map-subtitle"), 13.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            legend(ui, theme::c().outbound, &i18n.t("map-legend-out"));
            ui.add_space(10.0);
            legend(ui, theme::c().inbound, &i18n.t("map-legend-in"));
        });
    });
    ui.add_space(6.0);

    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let t = ui.input(|i| i.time) as f32;
    let canvas = rect.shrink(10.0);

    handle_input(ui, &resp, canvas, view);
    view.animate(ANIM_K);
    clamp_view(canvas, view);
    let proj = Projection::new(canvas, *view);
    let hover_pos = resp.hover_pos();

    basemap::draw(&painter, rect, &proj, i18n.current_lang.starts_with("zh"));

    let local = proj.project(local_pos.0, local_pos.1);
    let agg = aggregate(conns);
    let cycle_px = proj.cycle_px();

    // 连线按 远端归属+主导方向 聚合绘制(LOD):连接数不再直接放大绘制量,
    // 数百连接下仍是一城一线;线宽随聚合数增长表达流量规模
    let mut lines: BTreeMap<(Place, bool), (usize, u64)> = BTreeMap::new();
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
        let hovered = hover_pos.is_some_and(|h| wrap_dist(h, end, cycle_px) < 20.0);
        let width = 1.4 + 1.1 * (*count as f32 - 1.0).sqrt().min(2.0);
        let stroke = if hovered {
            Stroke::new(width + 0.6, color.gamma_multiply(0.9))
        } else {
            Stroke::new(width, color.gamma_multiply(0.45))
        };
        // 曲线横向 bbox 为端点包围盒(控制点 x 居中),据此求可见副本区间
        let (min_x, max_x) = (start.x.min(end.x), start.x.max(end.x));
        let k0 = ((rect.left() - max_x) / cycle_px).floor() as i32;
        let k1 = ((rect.right() - min_x) / cycle_px).floor() as i32;
        // 粒子与尾迹(主副本算一次,副本平移);相位按聚合键派生保持稳定
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        place.hash(&mut hasher);
        inbound.hash(&mut hasher);
        let phase = (t * 0.22 + hash_phase(hasher.finish())) % 1.0;
        let trail: Vec<Pos2> = (0..3u32)
            .map(|k| bezier(start, ctrl, end, (phase - 0.02 * k as f32).rem_euclid(1.0)))
            .collect();
        for k in k0..=k1 {
            let off = Vec2::new(k as f32 * cycle_px, 0.0);
            painter.add(Shape::QuadraticBezier(QuadraticBezierShape {
                points: [start + off, ctrl + off, end + off],
                closed: false,
                fill: Color32::TRANSPARENT,
                stroke: stroke.into(),
            }));
            for (i, pt) in trail.iter().enumerate() {
                painter.circle_filled(
                    *pt + off,
                    2.6 - 0.7 * i as f32,
                    color.gamma_multiply(0.9 - 0.3 * i as f32),
                );
            }
        }
    }

    // 归属节点:半径随连接数增长,外圈脉冲;命中悬停的节点记下来。
    // 节点与标签对每个可见 wrap 副本各画一份
    let mut hovered_place = None;
    for (place, (count, bytes)) in &agg {
        let (lon, lat) = geoip::place_pos(*place);
        let pos = proj.project(lon, lat);
        let r = 4.0 + 2.2 * (*count as f32).sqrt();
        let hovered = hover_pos.is_some_and(|h| wrap_dist(h, pos, cycle_px) < r + 8.0);
        if hovered {
            hovered_place = Some(*place);
        }
        let pulse_alpha =
            0.35 + 0.3 * (0.5 + 0.5 * (t * 2.0 + hash_phase(*bytes) * std::f32::consts::TAU).sin());
        let (k0, k1) = proj.visible_cycles(pos.x, rect);
        for k in k0..=k1 {
            let pos = pos + Vec2::new(k as f32 * cycle_px, 0.0);
            painter.circle_stroke(
                pos,
                r + 5.0,
                Stroke::new(1.5, theme::c().map_node.gamma_multiply(pulse_alpha)),
            );
            painter.circle_filled(
                pos,
                r,
                if hovered {
                    theme::c().accent
                } else {
                    theme::c().map_node
                },
            );
            painter.circle_stroke(pos, r, Stroke::new(1.0, theme::c().text));
            painter.text(
                pos + Vec2::new(0.0, r + 13.0),
                Align2::CENTER_CENTER,
                geoip::place_label(*place, i18n),
                FontId::proportional(11.0),
                if hovered {
                    theme::c().text
                } else {
                    theme::c().text_dim
                },
            );
        }
    }

    // 本机节点(标签放上方,避开东亚密集城市的下方标签)
    let (k0, k1) = proj.visible_cycles(local.x, rect);
    for k in k0..=k1 {
        let pos = local + Vec2::new(k as f32 * cycle_px, 0.0);
        painter.circle_stroke(
            pos,
            10.0,
            Stroke::new(1.5, theme::c().accent.gamma_multiply(0.5)),
        );
        painter.circle_filled(pos, 6.0, theme::c().accent);
        painter.text(
            pos + Vec2::new(0.0, -16.0),
            Align2::CENTER_BOTTOM,
            i18n.t("map-local"),
            FontId::proportional(11.0),
            theme::c().text,
        );
    }

    if let Some(place) = hovered_place {
        card::info_card(&painter, rect, place, conns, i18n, rdns, icon_tex);
    }
}

/// 视图交互:拖拽平移(1:1 跟手)、滚轮/捏合锚点缩放、双击复位
fn handle_input(ui: &egui::Ui, resp: &egui::Response, canvas: Rect, view: &mut View) {
    if resp.dragged() {
        let d = resp.drag_delta();
        let ppd = Projection::fit_ppd(canvas) * view.zoom;
        view.center_lon -= d.x / ppd;
        view.center_lat += d.y / ppd;
        view.target_lon = view.center_lon;
        view.target_lat = view.center_lat;
    }
    if resp.hovered() {
        // 普通滚轮与 Ctrl+滚轮/触控板捏合都驱动缩放
        let (wheel, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = pinch * (wheel * 0.002f32).exp();
        if (factor - 1.0).abs() > 1e-4 {
            let new_zoom = (view.target_zoom * factor).clamp(1.0, ZOOM_MAX);
            if let Some(pos) = resp.hover_pos() {
                // 锚定指针下的地理点:反解新缩放下的中心,使该点仍位于指针处
                let (glon, glat) = Projection::new(canvas, *view).unproject(pos);
                let ppd = Projection::fit_ppd(canvas) * new_zoom;
                let center = canvas.center();
                view.target_zoom = new_zoom;
                view.target_lon = glon - (pos.x - center.x) / ppd;
                view.target_lat = glat + (pos.y - center.y) / ppd;
            }
        }
    }
    if resp.double_clicked() {
        *view = View::global();
    }
}

/// 视口钳制:经度方向无缝循环,仅把中心与动画目标同步归一化到
/// [-180, 180) 防止数值无限增长(同步平移不改变动画相对关系);
/// 纬度仍限制中心使视口不脱出世界
fn clamp_view(canvas: Rect, view: &mut View) {
    let ppd = Projection::fit_ppd(canvas) * view.zoom;
    let half_lat = (canvas.height() * 0.5) / ppd;
    let shift = (view.center_lon / 360.0).round() * 360.0;
    if shift != 0.0 {
        view.center_lon -= shift;
        view.target_lon -= shift;
    }
    view.center_lat = clamp_center(view.center_lat, half_lat, 90.0);
    view.target_lat = clamp_center(view.target_lat, half_lat, 90.0);
}

/// 视口比世界还大时居中,否则限制中心使视口不脱出世界
fn clamp_center(v: f32, half: f32, limit: f32) -> f32 {
    if half >= limit {
        0.0
    } else {
        v.clamp(-limit + half, limit - half)
    }
}

fn aggregate(conns: &[Connection]) -> Agg {
    let mut agg: Agg = BTreeMap::new();
    for c in conns {
        let Some(place) = c.city else { continue };
        let entry = agg.entry(place).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += c.total_bytes();
    }
    agg
}

fn legend(ui: &mut egui::Ui, color: Color32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(12.0, 12.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
    ui.label(theme::dim_text(label, 12.0));
}
