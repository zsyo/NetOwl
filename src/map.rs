//! 流量地图画布:egui painter 自绘(底图渲染见 basemap,此处负责
//! 贝塞尔连线、流动粒子、节点聚合与悬停信息卡)。

use std::collections::BTreeMap;

use std::collections::HashMap;

use eframe::egui;
use egui::epaint::QuadraticBezierShape;
use egui::{
    Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind,
    TextureHandle, Vec2,
};

use crate::basemap::{self, Projection, View};
use crate::geoip;
use crate::i18n::I18n;
use crate::model::{Connection, Place, fmt_bytes};
use crate::rdns;
use crate::theme;

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

    for c in conns {
        // 归属未知(内网/保留段/未收录)的连接不上图,连接列表仍完整可见;
        // 节点端取最短方向等效经度(跨太平洋走短弧),主几何按可见世界副本
        // 平移铺开,屏幕边缘两侧由相邻副本自然接续
        let Some(place) = c.city else { continue };
        let (place_lon, place_lat) = geoip::place_pos(place);
        let end_lon = local_pos.0 + wrap_delta(place_lon - local_pos.0);
        let end = proj.project(end_lon, place_lat);
        let inbound = c.inbound_dominant();
        let (start, end) = if inbound { (end, local) } else { (local, end) };
        let ctrl = arc_ctrl(start, end);
        let color = if inbound {
            theme::c().inbound
        } else {
            theme::c().outbound
        };
        let hovered = hover_pos.is_some_and(|h| wrap_dist(h, end, cycle_px) < 20.0);
        let stroke = if hovered {
            Stroke::new(2.0, color.gamma_multiply(0.9))
        } else {
            Stroke::new(1.4, color.gamma_multiply(0.45))
        };
        // 曲线横向 bbox 为端点包围盒(控制点 x 居中),据此求可见副本区间
        let (min_x, max_x) = (start.x.min(end.x), start.x.max(end.x));
        let k0 = ((rect.left() - max_x) / cycle_px).floor() as i32;
        let k1 = ((rect.right() - min_x) / cycle_px).floor() as i32;
        // 粒子与尾迹(主副本算一次,副本平移)
        let phase = (t * 0.22 + hash_phase(c.id)) % 1.0;
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
        info_card(&painter, rect, place, conns, i18n, rdns, icon_tex);
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

/// 悬停归属节点的信息卡:节点名、总流量、最多 6 条连接明细
fn info_card(
    painter: &egui::Painter,
    canvas: Rect,
    place: Place,
    conns: &[Connection],
    i18n: &I18n,
    rdns: &rdns::Rdns,
    icon_tex: &HashMap<String, Option<TextureHandle>>,
) {
    const WIDTH: f32 = 310.0;
    const LINE_H: f32 = 17.0;
    const HEAD_H: f32 = 42.0;
    const MAX_ROWS: usize = 6;

    let rows: Vec<&Connection> = conns
        .iter()
        .filter(|conn| conn.city == Some(place))
        .collect();
    let shown = rows.len().min(MAX_ROWS);
    let extra = rows.len() - shown;
    let total: u64 = rows.iter().map(|conn| conn.total_bytes()).sum();
    let height = HEAD_H + shown as f32 * LINE_H + if extra > 0 { LINE_H } else { 0.0 } + 8.0;
    let card = Rect::from_min_size(
        Pos2::new(canvas.left() + 14.0, canvas.top() + 14.0),
        Vec2::new(WIDTH, height),
    );

    painter.rect_filled(
        card,
        CornerRadius::same(theme::RADIUS_LG),
        theme::c().bg_float,
    );
    painter.rect_stroke(
        card,
        CornerRadius::same(theme::RADIUS_LG),
        Stroke::new(1.0, theme::c().stroke),
        StrokeKind::Inside,
    );

    painter.text(
        Pos2::new(card.left() + 14.0, card.top() + 12.0),
        Align2::LEFT_TOP,
        geoip::place_label(place, i18n),
        FontId::proportional(16.0),
        theme::c().text,
    );
    painter.text(
        Pos2::new(card.right() - 14.0, card.top() + 14.0),
        Align2::RIGHT_TOP,
        fmt_bytes(total),
        FontId::proportional(12.0),
        theme::c().text_dim,
    );

    let mut y = card.top() + HEAD_H - 4.0;
    for conn in rows.iter().take(MAX_ROWS) {
        // 进程图标(14px);无图标时文本左缘保持一致,信息卡行不留空位
        let mut text_x = card.left() + 14.0;
        if let Some(tex) = conn
            .proc_path
            .as_deref()
            .and_then(|p| icon_tex.get(p))
            .and_then(|t| t.as_ref())
        {
            let icon_rect = Rect::from_min_size(
                Pos2::new(text_x, y + LINE_H / 2.0 - 7.0),
                Vec2::new(14.0, 14.0),
            );
            painter.image(
                tex.id(),
                icon_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
            text_x += 18.0;
        }
        let process = format!("{} ({})", conn.process, conn.pid);
        painter.text(
            Pos2::new(text_x, y + 8.0),
            Align2::LEFT_CENTER,
            process,
            FontId::proportional(12.0),
            theme::c().text,
        );
        // rDNS 域名优先(卡片行宽有限,超长截断),无 PTR 回退地址:端口
        let remote = match rdns.lookup(conn.remote_ip) {
            Some(host) => format!(
                "{} {}",
                rdns::display(host, conn.remote_port, 24),
                conn.proto.as_str()
            ),
            None => format!("{} {}", conn.remote_display(), conn.proto.as_str()),
        };
        painter.text(
            Pos2::new(card.right() - 14.0, y + 8.0),
            Align2::RIGHT_CENTER,
            remote,
            FontId::monospace(11.0),
            theme::c().text_dim,
        );
        y += LINE_H;
    }
    if extra > 0 {
        painter.text(
            Pos2::new(card.right() - 14.0, y + 8.0),
            Align2::RIGHT_CENTER,
            i18n.t_with_args("map-info-more", &[("n", extra.to_string())]),
            FontId::proportional(11.0),
            theme::c().text_dim,
        );
    }
}

fn legend(ui: &mut egui::Ui, color: Color32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(12.0, 12.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
    ui.label(theme::dim_text(label, 12.0));
}
