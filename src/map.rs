//! 流量地图画布:egui painter 自绘(背景网格、大陆线框、贝塞尔连线、
//! 流动粒子、节点聚合、悬停信息卡)。

use std::collections::BTreeMap;

use eframe::egui;
use egui::{
    Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2,
};
use egui::epaint::QuadraticBezierShape;

use crate::i18n::I18n;
use crate::model::{Connection, fmt_bytes};
use crate::theme;
use crate::world::{LANDMASSES, LOCAL, city};

/// 等距圆柱投影:经纬度 -> 画布坐标
struct Projection {
    origin: Pos2,
    scale: f32,
}

impl Projection {
    fn new(rect: Rect) -> Self {
        let scale = (rect.width() / 360.0).min(rect.height() / 180.0);
        let w = 360.0 * scale;
        let h = 180.0 * scale;
        let origin = Pos2::new(
            rect.left() + (rect.width() - w) * 0.5,
            rect.top() + (rect.height() - h) * 0.5,
        );
        Projection { origin, scale }
    }

    fn project(&self, lon: f32, lat: f32) -> Pos2 {
        Pos2::new(
            self.origin.x + (lon + 180.0) * self.scale,
            self.origin.y + (90.0 - lat) * self.scale,
        )
    }
}

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

/// 由 id/字节数派生稳定的 [0,1) 相位偏移,让粒子/脉冲错落
fn hash_phase(seed: u64) -> f32 {
    ((seed as f32 * 0.618_034) % 1.0).abs()
}

/// 城市节点聚合:连接数与累计流量
type Agg = BTreeMap<&'static str, (usize, u64)>;

pub fn draw(ui: &mut egui::Ui, conns: &[Connection], i18n: &I18n) {
    ui.horizontal(|ui| {
        ui.heading(theme::accent_text(&i18n.t("map-title"), 20.0));
        ui.label(theme::dim_text(&i18n.t("map-subtitle"), 13.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            legend(ui, theme::OUTBOUND, &i18n.t("map-legend-out"));
            ui.add_space(10.0);
            legend(ui, theme::INBOUND, &i18n.t("map-legend-in"));
        });
    });
    ui.add_space(6.0);

    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let painter = ui.painter_at(rect);
    let t = ui.input(|i| i.time) as f32;
    let proj = Projection::new(rect.shrink(10.0));
    let hover_pos = resp.hover_pos();

    draw_background(&painter, rect, &proj);
    draw_landmasses(&painter, &proj);

    let local = proj.project(LOCAL.lon, LOCAL.lat);
    let agg = aggregate(conns);

    for c in conns {
        let end = proj.project(city(c.city).lon, city(c.city).lat);
        let inbound = c.inbound_dominant();
        let (start, end) = if inbound { (end, local) } else { (local, end) };
        let ctrl = arc_ctrl(start, end);
        let color = if inbound { theme::INBOUND } else { theme::OUTBOUND };
        let hovered = hover_pos.is_some_and(|h| h.distance(end) < 20.0);
        let stroke = if hovered {
            Stroke::new(2.0, color.gamma_multiply(0.9))
        } else {
            Stroke::new(1.4, color.gamma_multiply(0.45))
        };
        painter.add(Shape::QuadraticBezier(QuadraticBezierShape {
            points: [start, ctrl, end],
            closed: false,
            fill: Color32::TRANSPARENT,
            stroke: stroke.into(),
        }));
        // 粒子与尾迹
        let phase = (t * 0.22 + hash_phase(c.id)) % 1.0;
        for k in 0..3u32 {
            let pt = bezier(start, ctrl, end, (phase - 0.02 * k as f32).rem_euclid(1.0));
            painter.circle_filled(pt, 2.6 - 0.7 * k as f32, color.gamma_multiply(0.9 - 0.3 * k as f32));
        }
    }

    // 城市节点:半径随连接数增长,外圈脉冲;命中悬停的城市记下来
    let mut hovered_key = None;
    for (key, (count, bytes)) in &agg {
        let c = city(key);
        let pos = proj.project(c.lon, c.lat);
        let r = 4.0 + 2.2 * (*count as f32).sqrt();
        let hovered = hover_pos.is_some_and(|h| h.distance(pos) < r + 8.0);
        if hovered {
            hovered_key = Some(*key);
        }
        let pulse_alpha = 0.35 + 0.3 * (0.5 + 0.5 * (t * 2.0 + hash_phase(*bytes) * std::f32::consts::TAU).sin());
        painter.circle_stroke(pos, r + 5.0, Stroke::new(1.5, theme::MAP_NODE.gamma_multiply(pulse_alpha)));
        painter.circle_filled(pos, r, if hovered { theme::ACCENT } else { theme::MAP_NODE });
        painter.circle_stroke(pos, r, Stroke::new(1.0, theme::TEXT));
        painter.text(
            pos + Vec2::new(0.0, r + 13.0),
            Align2::CENTER_CENTER,
            i18n.t(&format!("city-{key}")),
            FontId::proportional(11.0),
            if hovered { theme::TEXT } else { theme::TEXT_DIM },
        );
    }

    // 本机节点(标签放上方,避开东亚密集城市的下方标签)
    painter.circle_stroke(local, 10.0, Stroke::new(1.5, theme::ACCENT.gamma_multiply(0.5)));
    painter.circle_filled(local, 6.0, theme::ACCENT);
    painter.text(
        local + Vec2::new(0.0, -16.0),
        Align2::CENTER_BOTTOM,
        i18n.t("map-local"),
        FontId::proportional(11.0),
        theme::TEXT,
    );

    if let Some(key) = hovered_key {
        info_card(&painter, rect, key, conns, i18n);
    }
}

fn draw_background(painter: &egui::Painter, rect: Rect, proj: &Projection) {
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_LG), theme::BG_MAP);
    for lon in (-180..=180).step_by(30) {
        let p0 = proj.project(lon as f32, 90.0);
        let p1 = proj.project(lon as f32, -90.0);
        painter.line_segment([p0, p1], Stroke::new(1.0, theme::MAP_GRID));
    }
    for lat in (-60..=60).step_by(30) {
        let p0 = proj.project(-180.0, lat as f32);
        let p1 = proj.project(180.0, lat as f32);
        painter.line_segment([p0, p1], Stroke::new(1.0, theme::MAP_GRID));
    }
}

/// 大陆轮廓线框:逐段绘制(epaint 0.36 的 PathShape 填充仅支持凸多边形,
/// 且自交路径的 feather 描边会产生飞线,故用相邻点连线)
fn draw_landmasses(painter: &egui::Painter, proj: &Projection) {
    let stroke = Stroke::new(1.0, theme::MAP_COAST);
    for land in LANDMASSES {
        let points: Vec<Pos2> = land.iter().map(|(lon, lat)| proj.project(*lon, *lat)).collect();
        for pair in points.windows(2) {
            painter.line_segment([pair[0], pair[1]], stroke);
        }
        if let (Some(first), Some(last)) = (points.first(), points.last()) {
            painter.line_segment([*last, *first], stroke);
        }
    }
}

fn aggregate(conns: &[Connection]) -> Agg {
    let mut agg: Agg = BTreeMap::new();
    for c in conns {
        let entry = agg.entry(c.city).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += c.total_bytes();
    }
    agg
}

/// 悬停城市的信息卡:城市名、总流量、最多 6 条连接明细
fn info_card(
    painter: &egui::Painter,
    canvas: Rect,
    key: &'static str,
    conns: &[Connection],
    i18n: &I18n,
) {
    const WIDTH: f32 = 310.0;
    const LINE_H: f32 = 17.0;
    const HEAD_H: f32 = 42.0;
    const MAX_ROWS: usize = 6;

    let rows: Vec<&Connection> = conns.iter().filter(|conn| conn.city == key).collect();
    let shown = rows.len().min(MAX_ROWS);
    let extra = rows.len() - shown;
    let total: u64 = rows.iter().map(|conn| conn.total_bytes()).sum();
    let height = HEAD_H + shown as f32 * LINE_H + if extra > 0 { LINE_H } else { 0.0 } + 8.0;
    let card = Rect::from_min_size(
        Pos2::new(canvas.left() + 14.0, canvas.top() + 14.0),
        Vec2::new(WIDTH, height),
    );

    painter.rect_filled(card, CornerRadius::same(theme::RADIUS_LG), theme::BG_FLOAT);
    painter.rect_stroke(card, CornerRadius::same(theme::RADIUS_LG), Stroke::new(1.0, theme::STROKE), StrokeKind::Inside);

    painter.text(
        Pos2::new(card.left() + 14.0, card.top() + 12.0),
        Align2::LEFT_TOP,
        i18n.t(&format!("city-{key}")),
        FontId::proportional(16.0),
        theme::TEXT,
    );
    painter.text(
        Pos2::new(card.right() - 14.0, card.top() + 14.0),
        Align2::RIGHT_TOP,
        fmt_bytes(total),
        FontId::proportional(12.0),
        theme::TEXT_DIM,
    );

    let mut y = card.top() + HEAD_H - 4.0;
    for conn in rows.iter().take(MAX_ROWS) {
        let process = format!("{} ({})", conn.process, conn.pid);
        painter.text(Pos2::new(card.left() + 14.0, y + 8.0), Align2::LEFT_CENTER, process, FontId::proportional(12.0), theme::TEXT);
        let remote = format!("{}:{} {}", conn.remote_ip, conn.remote_port, conn.proto.as_str());
        painter.text(Pos2::new(card.right() - 14.0, y + 8.0), Align2::RIGHT_CENTER, remote, FontId::monospace(11.0), theme::TEXT_DIM);
        y += LINE_H;
    }
    if extra > 0 {
        painter.text(
            Pos2::new(card.right() - 14.0, y + 8.0),
            Align2::RIGHT_CENTER,
            i18n.t_with_args("map-info-more", &[("n", extra.to_string())]),
            FontId::proportional(11.0),
            theme::TEXT_DIM,
        );
    }
}

fn legend(ui: &mut egui::Ui, color: Color32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(12.0, 12.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
    ui.label(theme::dim_text(label, 12.0));
}
