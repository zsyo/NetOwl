//! 地图面板共享小组件:小节标题、双向流量卡、流量排行行(占比条)。
//! 右侧 Inspector 专用;行内可变长文本统一"固定宽度容器 + Truncate"
//! (宽度扣除同行其余控件,否则溢出经 resizable 面板宽度记忆逐帧
//! 放大,见 map_inspector 模块注释)。

use std::collections::HashMap;

use eframe::egui;
use egui::{Button, Color32, CornerRadius, Frame, Label, Margin, RichText, Stroke};

use crate::model::fmt_bytes;
use crate::ui::icons;
use crate::ui::map_panel::{MapPanelState, ProcGroup, RankSort};
use crate::ui::text_width;
use crate::ui::theme;
use crate::ui::widgets;

/// 小节标题(排行/列表段)
pub(crate) fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(theme::sp::SM);
    ui.label(
        RichText::new(text.to_owned())
            .size(theme::font::SM)
            .strong()
            .color(theme::c().text_dim),
    );
}

/// 排行小节标题 + 右侧排序切换(总量/上传/下载);
/// 返回用户新选的排序键(None = 未变),由调用方写回对应排行的状态
pub(crate) fn rank_header(
    ui: &mut egui::Ui,
    text: &str,
    sort: RankSort,
    i18n: &crate::i18n::I18n,
) -> Option<RankSort> {
    ui.add_space(theme::sp::SM);
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(text.to_owned())
                .size(theme::font::SM)
                .strong()
                .color(theme::c().text_dim),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let items = [
                ("map-rank-total", icons::ARROW_DOWN_UP),
                ("map-rank-up", icons::ARROW_UP),
                ("map-rank-down", icons::ARROW_DOWN),
            ];
            let texts: Vec<(String, &str)> = items
                .iter()
                .map(|(key, glyph)| (i18n.t(key), *glyph))
                .collect();
            let view: Vec<(&str, &str)> = texts.iter().map(|(t, g)| (t.as_str(), *g)).collect();
            let idx = match sort {
                RankSort::Total => 0,
                RankSort::Out => 1,
                RankSort::In => 2,
            };
            if let Some(i) = widgets::segmented::segmented(ui, &view, idx) {
                picked = Some(match i {
                    1 => RankSort::Out,
                    2 => RankSort::In,
                    _ => RankSort::Total,
                });
            }
        });
    });
    picked
}

/// 下载/上传双卡片(方向图标 + 语义色大数字,样式仿历史页卡片)
pub(crate) fn traffic_cards(ui: &mut egui::Ui, down: u64, up: u64, i18n: &crate::i18n::I18n) {
    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 8.0;
        // 卡宽留余量:Frame 描边等细微外扩若顶满可用宽,超出部分会经
        // resizable 面板的宽度记忆逐帧放大(见模块注释)
        let w = (ui.available_width() - 16.0) / 2.0;
        traffic_card(
            ui,
            w,
            icons::ARROW_DOWN,
            &i18n.t("nav-rate-down"),
            &fmt_bytes(down),
            theme::c().inbound,
        );
        traffic_card(
            ui,
            w,
            icons::ARROW_UP,
            &i18n.t("nav-rate-up"),
            &fmt_bytes(up),
            theme::c().outbound,
        );
    });
}

fn traffic_card(ui: &mut egui::Ui, w: f32, glyph: &str, label: &str, value: &str, color: Color32) {
    // Frame 的响应宽 = 内容宽 + 内边距;内容需减去两侧内边距,
    // 否则两卡合计超出面板可用宽,经面板宽度记忆逐帧膨胀
    Frame::new()
        .fill(theme::c().bg_card)
        .stroke(Stroke::new(1.0, theme::c().stroke))
        .corner_radius(CornerRadius::same(theme::RADIUS_MD))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(w - 20.0);
            ui.set_min_height(46.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.style_mut().spacing.item_spacing.x = 3.0;
                    ui.label(RichText::new(glyph).size(theme::font::MICRO).color(color));
                    ui.label(theme::dim_text(label, theme::font::MICRO));
                });
                ui.label(
                    RichText::new(value.to_owned())
                        .size(theme::font::H2)
                        .strong()
                        .color(color),
                );
            });
        });
}

/// Top 进程行:图标 + 名称(点击选中联动进程详情,Truncate 自适应)+
/// 右侧下载/上传分色字节(上传超过下载时以警示色提示)+
/// 下方流量占比条(相对当前排序键的第一名,占比由调用方传入)
pub(crate) fn proc_rank_row(
    ui: &mut egui::Ui,
    panels: &mut MapPanelState,
    i18n: &crate::i18n::I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    g: &ProcGroup,
    bar_ratio: f32,
) {
    let selected = panels.process.as_deref() == Some(g.name.as_str());
    let display = if g.name.is_empty() {
        i18n.t("conn-proc-unknown")
    } else {
        g.name.clone()
    };
    let path = g.conns.iter().find_map(|c| c.proc_path.as_deref());
    let in_sum: u64 = g.conns.iter().map(|c| c.bytes_in).sum();
    let out_sum: u64 = g.conns.iter().map(|c| c.bytes_out).sum();
    let in_text = fmt_bytes(in_sum);
    let out_text = fmt_bytes(out_sum);
    let in_w = text_width(ui, &in_text, theme::font::XS);
    let out_w = text_width(ui, &out_text, theme::font::XS);
    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 4.0;
        let tex = path.and_then(|p| icon_tex.get(p)).and_then(|t| t.as_ref());
        match tex.or(default_icon_tex) {
            Some(t) => {
                ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(16.0, 16.0)));
            }
            None => {
                ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            }
        }
        let name_w = (ui.available_width() - in_w - out_w - 4.0 * 3.0).max(60.0);
        ui.allocate_ui(egui::vec2(name_w, 20.0), |ui| {
            let text = RichText::new(format!(
                "{display} {}",
                i18n.t_with_args(
                    "conns-count-suffix",
                    &[("count", g.conns.len().to_string())]
                )
            ))
            .size(theme::font::SM)
            .color(if selected {
                theme::c().accent
            } else {
                theme::c().text
            });
            let btn = Button::new(text)
                .truncate()
                .frame(false)
                .corner_radius(CornerRadius::same(theme::RADIUS_SM));
            if ui.add(btn).clicked() {
                panels.process = Some(g.name.clone());
            }
        });
        // 右侧区从右往左排:上传、下载;上传占优视为可疑(可能是
        // 未经用户预期的外发流量),用警示色代替常规方向色
        let out_color = if out_sum > in_sum {
            theme::c().status_warn
        } else {
            theme::c().outbound
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.style_mut().spacing.item_spacing.x = 6.0;
            ui.label(
                RichText::new(&out_text)
                    .size(theme::font::XS)
                    .color(out_color),
            );
            ui.label(
                RichText::new(&in_text)
                    .size(theme::font::XS)
                    .color(theme::c().inbound),
            );
        });
    });
    rank_bar(ui, bar_ratio);
}

/// 排行的域名行:名称(Truncate 自适应)+ 右侧下载/上传分色字节
/// (上传超过下载时以警示色提示)+ 流量占比条(由调用方传入)
pub(crate) fn bytes_row(
    ui: &mut egui::Ui,
    title: &str,
    bytes_in: u64,
    bytes_out: u64,
    bar_ratio: f32,
) {
    let in_text = fmt_bytes(bytes_in);
    let out_text = fmt_bytes(bytes_out);
    let arrow_w = text_width(ui, icons::ARROW_DOWN, theme::font::XS);
    let in_w = text_width(ui, &in_text, theme::font::XS) + arrow_w;
    let out_w = text_width(ui, &out_text, theme::font::XS) + arrow_w;
    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 4.0;
        let title_w = (ui.available_width() - in_w - out_w - 4.0 * 3.0).max(60.0);
        ui.allocate_ui(egui::vec2(title_w, 16.0), |ui| {
            ui.add(
                Label::new(
                    RichText::new(title.to_owned())
                        .size(theme::font::SM)
                        .color(theme::c().text),
                )
                .truncate(),
            );
        });
        let out_color = if bytes_out > bytes_in {
            theme::c().status_warn
        } else {
            theme::c().outbound
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.style_mut().spacing.item_spacing.x = 3.0;
            rate_pair(ui, &out_text, out_color, icons::ARROW_UP);
            rate_pair(ui, &in_text, theme::c().inbound, icons::ARROW_DOWN);
        });
    });
    rank_bar(ui, bar_ratio);
}

/// 一组方向速率:方向箭头 + 数值(同色,避免仅靠颜色区分上传/下载)。
/// 调用处为 right_to_left 布局:先加的靠右,故先数值后箭头得"箭头在左"
fn rate_pair(ui: &mut egui::Ui, text: &str, color: Color32, arrow: &str) {
    ui.label(RichText::new(text).size(theme::font::XS).color(color));
    ui.label(RichText::new(arrow).size(theme::font::XS).color(color));
}

/// 流量占比条:faint 底轨 + accent 前景(ratio 0 起步保留 2% 可视宽)
fn rank_bar(ui: &mut egui::Ui, ratio: f32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.0), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(theme::RADIUS_PILL),
        theme::c().faint,
    );
    if ratio > f32::EPSILON {
        let fg = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width() * ratio.clamp(0.02, 1.0), rect.height()),
        );
        ui.painter().rect_filled(
            fg,
            CornerRadius::same(theme::RADIUS_PILL),
            theme::c().accent.gamma_multiply(0.7),
        );
    }
}
