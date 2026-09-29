//! 公共 UI 组件库:跨页面复用的绘制小件。
//!
//! 组件只做绘制与交互返回,状态由调用方持有(架构规范:数据与绘制分离);
//! 全部取色经 [`theme::c()`](super::theme::c),字号/间距用刻度常量。

use eframe::egui;
use egui::{Color32, Pos2, Sense, Stroke, Vec2};

/// 速率迷你走势图(面积填充 + 折线描边)。
///
/// `data` 为时间正序的采样序列,按峰值归一化;样本不足 2 个时只画基线。
pub fn sparkline(ui: &mut egui::Ui, data: &[u64], color: Color32, size: Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    let max = data.iter().copied().max().unwrap_or(0).max(1) as f32;
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
