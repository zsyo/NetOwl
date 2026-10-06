//! 流量地图画布:egui painter 自绘(底图渲染见 basemap,悬停信息卡见
//! card,贝塞尔连线与粒子见 flights,此处负责节点聚合与画布交互)。

pub mod basemap;
mod card;
mod flights;
pub mod triangulate;
pub mod world;

use std::collections::BTreeMap;

use std::collections::HashMap;

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2};

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

/// 地图画布的点击结果(UI 层据此更新面板选中状态)
pub enum MapClick {
    /// 命中归属节点:选中该端点
    Place(Place),
    /// 命中空白:清除选中
    Background,
}

/// 画布绘制入口,数据面(连接/域名/图标)与视图状态全部参数化传入,
/// 不引入 App 层上下文类型以保持 map 与 ui 的边界
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &mut egui::Ui,
    conns: &[Connection],
    i18n: &I18n,
    view: &mut View,
    rdns: &rdns::Rdns,
    icon_tex: &HashMap<String, Option<TextureHandle>>,
    default_icon_tex: Option<&TextureHandle>,
    local_pos: (f32, f32),
    // 选中端点:其余连线/节点淡化,选中节点保持高亮(端点选中联动)
    selected: Option<Place>,
) -> Option<MapClick> {
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

    flights::draw_flights(
        &painter, rect, &proj, conns, local, local_pos, cycle_px, hover_pos, selected, t,
    );

    // 归属节点:半径随连接数增长,外圈脉冲;命中悬停的节点记下来,
    // 单击命中即选中端点(双击复位视图不算选中)。节点与标签对每个
    // 可见 wrap 副本各画一份
    let mut hovered_place = None;
    let mut clicked_place = None;
    for (place, (count, bytes)) in &agg {
        let (lon, lat) = geoip::place_pos(*place);
        let pos = proj.project(lon, lat);
        let r = 4.0 + 2.2 * (*count as f32).sqrt();
        let hovered = hover_pos.is_some_and(|h| wrap_dist(h, pos, cycle_px) < r + 8.0);
        if hovered {
            hovered_place = Some(*place);
            if resp.clicked() && !resp.double_clicked() {
                clicked_place = Some(*place);
            }
        }
        let is_sel = selected == Some(*place);
        let dim = selected.is_some() && !is_sel;
        let fade = |color: Color32| {
            if dim {
                color.gamma_multiply(0.35)
            } else {
                color
            }
        };
        let phase = hash_phase(*bytes) * std::f32::consts::TAU;
        // 双层呼吸环:内圈快相位贴节点,外圈慢相位向外扩散,错拍呼吸
        let inner_alpha = 0.35 + 0.3 * (0.5 + 0.5 * (t * 2.0 + phase).sin());
        let outer_alpha = 0.30 + 0.3 * (0.5 + 0.5 * (-t * 1.3 + phase + 1.2).sin());
        let (k0, k1) = proj.visible_cycles(pos.x, rect);
        for k in k0..=k1 {
            let pos = pos + Vec2::new(k as f32 * cycle_px, 0.0);
            painter.circle_stroke(
                pos,
                r + 3.0,
                Stroke::new(1.2, fade(theme::c().map_node).gamma_multiply(inner_alpha)),
            );
            painter.circle_stroke(
                pos,
                r + 7.0,
                Stroke::new(1.0, fade(theme::c().map_node).gamma_multiply(outer_alpha)),
            );
            painter.circle_filled(
                pos,
                r,
                if hovered || is_sel {
                    theme::c().accent
                } else {
                    fade(theme::c().map_node)
                },
            );
            painter.circle_stroke(pos, r, Stroke::new(1.0, fade(theme::c().text)));
            if is_sel {
                // 选中端点常亮高亮环(区别于悬停的填充变色)
                painter.circle_stroke(
                    pos,
                    r + 6.0,
                    Stroke::new(2.0, theme::c().accent.gamma_multiply(0.8)),
                );
            }
            painter.text(
                pos + Vec2::new(0.0, r + 13.0),
                Align2::CENTER_CENTER,
                geoip::place_label(*place, i18n),
                FontId::proportional(11.0),
                if dim {
                    theme::c().text_dim.gamma_multiply(0.5)
                } else if hovered || is_sel {
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
        card::info_card(
            &painter,
            rect,
            place,
            conns,
            i18n,
            rdns,
            icon_tex,
            default_icon_tex,
        );
    }

    // 单击空白(未命中任何节点)清除选中;拖拽结束与双击不算单击
    if resp.clicked() && !resp.double_clicked() {
        Some(clicked_place.map_or(MapClick::Background, MapClick::Place))
    } else {
        None
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
