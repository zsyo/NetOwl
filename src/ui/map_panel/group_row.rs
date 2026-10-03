//! 地图左面板进程组头行:展开箭头、图标、名称(点击选中联动右侧
//! 详情)、会话累计上/下行副行、连接数徽章与行尾进程级阻断开关。

use std::collections::HashMap;

use eframe::egui;
use egui::{Button, CornerRadius, RichText, Vec2};

use super::{MapPanelState, ProcGroup};
use crate::i18n::I18n;
use crate::rules::Rule;
use crate::storage::history;
use crate::ui::icons;
use crate::ui::text_width;
use crate::ui::theme;
use crate::ui::widgets;

/// 进程组头行:展开箭头、图标、名称(点击选中联动右侧详情,命中区
/// 拉满剩余宽)与下方会话累计上/下行小字、连接数徽章与行尾进程级
/// 阻断开关(未知进程不可阻断,避免空进程条件生成全局规则)
#[allow(clippy::too_many_arguments)]
pub(super) fn group_row(
    ui: &mut egui::Ui,
    panels: &mut MapPanelState,
    rules: &mut crate::rules::RuleSet,
    db: &history::Db,
    i18n: &I18n,
    icon_tex: &HashMap<String, Option<egui::TextureHandle>>,
    default_icon_tex: Option<&egui::TextureHandle>,
    g: &ProcGroup,
    path: Option<&str>,
) {
    const ROW_H: f32 = 40.0;
    const SWITCH_W: f32 = 36.0;
    const SWITCH_H: f32 = 20.0;
    let unknown = g.name.is_empty();
    let display = if unknown {
        i18n.t("conn-proc-unknown")
    } else {
        g.name.clone()
    };
    let selected = panels.process.as_deref() == Some(g.name.as_str());
    let expanded = panels.expanded.contains(&g.name);
    let in_sum: u64 = g.conns.iter().map(|c| c.bytes_in).sum();
    let out_sum: u64 = g.conns.iter().map(|c| c.bytes_out).sum();

    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 4.0;
        // 行内小图标按钮用紧凑 padding(style.button_padding 默认 10x5 太宽)
        ui.style_mut().spacing.button_padding = egui::vec2(2.0, 0.0);
        let arrow = if expanded {
            icons::CHEVRON_DOWN
        } else {
            icons::CHEVRON_RIGHT
        };
        let fold = Button::new(
            RichText::new(arrow)
                .size(theme::font::MICRO)
                .color(theme::c().text_dim),
        )
        .frame(false)
        .min_size(Vec2::new(14.0, ROW_H));
        let fold_clicked = ui.add(fold).clicked();
        // 未展开(remove 失败)则展开,已展开则收起
        if fold_clicked && !panels.expanded.remove(&g.name) {
            panels.expanded.insert(g.name.clone());
        }
        let tex = path.and_then(|p| icon_tex.get(p)).and_then(|t| t.as_ref());
        widgets::process::proc_icon(ui, tex, default_icon_tex, 16.0);
        // 名称行 + 会话累计副行:受控选中样式(Button::selected 走
        // selection 底色,悬停底色由 widget 五态自动接管);按钮拉满
        // 命中区容器,徽章与开关由此贴到行尾
        let count_text = g.conns.len().to_string();
        let badge_w = text_width(ui, &count_text, theme::font::MICRO) + 18.0;
        let block_w = if unknown { 0.0 } else { SWITCH_W + 4.0 };
        let name_w = (ui.available_width() - badge_w - block_w - 4.0 * 2.0 - 4.0).max(60.0);
        ui.allocate_ui(egui::vec2(name_w, ROW_H), |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let text = RichText::new(display.clone())
                    .size(theme::font::BODY)
                    .color(if selected {
                        theme::c().text
                    } else {
                        theme::c().text_dim
                    });
                let btn = Button::new(text)
                    .truncate()
                    .selected(selected)
                    .corner_radius(CornerRadius::same(theme::RADIUS_SM))
                    .min_size(Vec2::new(ui.available_width(), 20.0));
                let resp = ui.add(btn);
                if resp.clicked() {
                    if selected {
                        panels.process = None;
                    } else {
                        panels.process = Some(g.name.clone());
                    }
                }
                resp.on_hover_text(display);
                // 会话累计(活跃连接字节聚合):随连接增减实时收敛
                ui.horizontal(|ui| {
                    ui.style_mut().spacing.item_spacing.x = 3.0;
                    ui.label(
                        RichText::new(icons::ARROW_DOWN)
                            .size(theme::font::XS)
                            .color(theme::c().inbound),
                    );
                    ui.label(
                        RichText::new(crate::model::fmt_bytes(in_sum))
                            .size(theme::font::XS)
                            .color(theme::c().text_dim),
                    );
                    ui.label(
                        RichText::new(icons::ARROW_UP)
                            .size(theme::font::XS)
                            .color(theme::c().outbound),
                    );
                    ui.label(
                        RichText::new(crate::model::fmt_bytes(out_sum))
                            .size(theme::font::XS)
                            .color(theme::c().text_dim),
                    );
                });
            });
        });
        widgets::badge::badge(ui, &count_text, widgets::badge::BadgeKind::Neutral);
        if unknown {
            return;
        }
        // 进程级阻断开关(off 绿 = 放行 / on 红 = 阻断):切换即建/删规则
        let existing = rules.process_block_rule(&g.name, path).map(|r| r.id);
        let mut on = existing.is_some();
        let resp = widgets::toggle::block_switch(
            ui,
            &mut on,
            true,
            SWITCH_W,
            SWITCH_H,
            egui::Id::new(("proc-block-toggle", g.name.clone())),
        );
        if resp.changed() {
            match (on, existing) {
                (true, None) => {
                    if let Err(e) = rules.insert(db, Rule::block(&g.name, None)) {
                        tracing::warn!("[Map] 进程 {} 阻断规则写入失败: {e}", g.name);
                    }
                }
                (false, Some(id)) => {
                    if let Err(e) = rules.delete(db, id) {
                        tracing::warn!("[Map] 进程 {} 阻断规则(id {id})删除失败: {e}", g.name);
                    }
                }
                _ => {}
            }
        }
        resp.on_hover_text(if on {
            i18n.t("map-unblock-process")
        } else {
            i18n.t("map-block-process")
        });
    });
}
