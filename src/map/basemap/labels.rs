//! 底图名称标签:按缩放分级显隐,国家名英文大写加字距,海洋名蓝色,
//! 省名(中国)中性色弱化。

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect};

use super::Projection;
use crate::map::world::{LabelKind, MapLabel};
use crate::ui::theme;

/// 名称标签:按缩放分级显隐(<1.6x 只显示 rank0,>3.5x 全部),
/// 国家名英文大写加字距,海洋名蓝色,省名(中国)中性色小一号
pub(super) fn draw_labels(
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
