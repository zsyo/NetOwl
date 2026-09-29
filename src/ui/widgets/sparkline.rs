//! 速率迷你走势图(多序列面积叠加)。

use eframe::egui;
use egui::{Color32, Pos2, Sense, Stroke, Vec2};

/// 多序列迷你走势图:同一坐标系叠加面积填充与折线描边,
/// 峰值按全序列统一归一;样本不足 2 个的序列只画基线。
pub fn sparklines(ui: &mut egui::Ui, series: &[(&[u64], Color32)], size: Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    let max = series
        .iter()
        .flat_map(|(d, _)| d.iter())
        .copied()
        .max()
        .unwrap_or(0)
        .max(1) as f32;
    for (data, color) in series {
        draw_series(&painter, rect, data, *color, max);
    }
}

/// 单序列绘制:相邻采样点与底部构成 quad 面积,再叠折线
fn draw_series(painter: &egui::Painter, rect: egui::Rect, data: &[u64], color: Color32, max: f32) {
    let n = data.len();
    if n < 2 {
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(1.0, color.gamma_multiply(0.4)),
        );
        return;
    }
    let step = rect.width() / (n - 1) as f32;
    let base = rect.bottom();
    let mut line: Vec<Pos2> = Vec::with_capacity(n);
    for (i, v) in data.iter().enumerate() {
        let x = rect.left() + i as f32 * step;
        let y = base - (rect.height() * (*v as f32 / max)).min(rect.height());
        line.push(egui::pos2(x, y));
    }
    let mut mesh = egui::Mesh::default();
    for i in 1..n {
        let (a, b) = (line[i - 1], line[i]);
        let idx = mesh.vertices.len() as u32;
        for v in [a, b, egui::pos2(b.x, base), egui::pos2(a.x, base)] {
            mesh.colored_vertex(v, color.gamma_multiply(0.28));
        }
        mesh.add_triangle(idx, idx + 1, idx + 2);
        mesh.add_triangle(idx, idx + 2, idx + 3);
    }
    painter.add(mesh);
    painter.add(egui::Shape::line(line, Stroke::new(1.2, color)));
}
