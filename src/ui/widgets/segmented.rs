//! 分段选择器:互斥选项切换(视图切换/主题切换等)。

use eframe::egui;
use egui::{Button, CornerRadius, RichText, Stroke};

use super::super::theme;

/// 分段选择器:返回本轮被点击项的下标(无点击为 None)。
///
/// `items` 为 (文本, 图标) 列表,图标可传空串省略
pub fn segmented(ui: &mut egui::Ui, items: &[(&str, &str)], selected: usize) -> Option<usize> {
    let p = theme::c();
    let mut clicked = None;
    egui::Frame::new()
        .fill(p.faint)
        .corner_radius(CornerRadius::same(theme::RADIUS_MD))
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            // 组件自带水平布局:不依赖调用方包裹(页面默认布局是竖排,
            // 直接调用会逐项换行;外部已包 horizontal 的调用点嵌套无害)
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, (text, icon)) in items.iter().enumerate() {
                    let active = i == selected;
                    let glyph = if icon.is_empty() {
                        (*text).to_owned()
                    } else {
                        format!("{icon}  {text}")
                    };
                    let label = RichText::new(glyph).size(theme::font::SM).color(if active {
                        p.text
                    } else {
                        p.text_dim
                    });
                    let resp = ui.add(
                        Button::new(label)
                            .fill(if active {
                                p.bg_card
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .stroke(if active {
                                Stroke::new(1.0, p.stroke_strong)
                            } else {
                                // Frame 的 stroke 宽度计入总尺寸:非激活用同宽透明
                                // 描边占位,否则选中切换时按钮差 2px,推挤相邻按钮
                                Stroke::new(1.0, egui::Color32::TRANSPARENT)
                            })
                            .corner_radius(CornerRadius::same(theme::RADIUS_SM)),
                    );
                    if resp.clicked() {
                        clicked = Some(i);
                    }
                }
            });
        });
    clicked
}
