//! 地图页右面板(Inspector):概览、端点详情与进程详情三态。
//! 无选中时显示全局概览(计数、总流量与 Top 进程/域名);选中端点
//! 显示该端点的相关进程;选中进程显示路径、签名、阻断开关与连接明细。
//! 选中态由地图端点点击与左列表进程点击驱动。
//!
//! 行内可变长文本统一"固定宽度容器 + Truncate":拉满剩余宽的控件若
//! 不设容器会把后续控件挤出面板,经 resizable 面板的宽度记忆逐帧放大
//! (面板宽度记忆取自内容矩形)。

mod place;
mod process;
mod summary;

use eframe::egui;
use egui::{Button, CornerRadius, Label, RichText, ScrollArea, Stroke};

use crate::model::Connection;
use crate::storage::config::Config;
use crate::ui::UiCtx;
use crate::ui::icons;
use crate::ui::theme;

/// 右侧 Inspector 面板:按选中对象切换视图
pub fn inspector_panel(ui: &mut egui::Ui, ctx: &mut UiCtx) {
    let panels = &mut *ctx.map_panels;
    // Esc 清除选中(端点与进程一并清除,回到概览);焦点在输入框
    // (左列表搜索词)时跳过,Esc 留给文本框,不连带清除选中
    let editing = ui.memory(|m| m.focused().is_some());
    if !editing && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        panels.place = None;
        panels.process = None;
        return;
    }
    let rules = &mut *ctx.rules;
    let i18n: &crate::i18n::I18n = ctx.i18n;
    let conns: &[Connection] = ctx.conns;
    let db = ctx.history_db;
    let rdns = ctx.rdns;
    let icon_tex = ctx.icon_tex;
    let default_icon_tex = ctx.default_icon_tex;
    let config: &Config = ctx.config;
    let history = &mut *ctx.history;
    let conn_search = &mut *ctx.conn_search;
    let nav_request = &mut *ctx.nav_request;

    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let process = panels.process.clone();
        match (panels.place, process) {
            (_, Some(name)) => process::view(
                ui,
                panels,
                rules,
                db,
                i18n,
                rdns,
                icon_tex,
                default_icon_tex,
                config,
                conns,
                &name,
                history,
                conn_search,
                nav_request,
            ),
            (Some(place), None) => place::view(
                ui,
                panels,
                i18n,
                rdns,
                icon_tex,
                default_icon_tex,
                config,
                conns,
                place,
            ),
            (None, None) => summary::view(
                ui,
                panels,
                i18n,
                rdns,
                icon_tex,
                default_icon_tex,
                config,
                conns,
            ),
        }
    });
}

/// 标题行:图标 + 大标题(Truncate 自适应)+ 右侧清除选中按钮;
/// 返回按钮是否被点击
pub(super) fn title_row(
    ui: &mut egui::Ui,
    title: &str,
    tex: Option<&egui::TextureHandle>,
    clear_tip: String,
) -> bool {
    let clear_w = 26.0;
    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 6.0;
        // 清除按钮紧凑 padding(style.button_padding 默认 10x5 太宽)
        ui.style_mut().spacing.button_padding = egui::vec2(4.0, 2.0);
        if let Some(t) = tex {
            ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(20.0, 20.0)));
        }
        let title_w = (ui.available_width() - clear_w - 6.0).max(60.0);
        ui.allocate_ui(egui::vec2(title_w, 22.0), |ui| {
            ui.add(
                Label::new(
                    RichText::new(title.to_owned())
                        .size(theme::font::PANEL_TITLE)
                        .strong()
                        .color(theme::c().text),
                )
                .truncate(),
            );
        });
        let btn = Button::new(
            RichText::new(icons::X_LG)
                .size(theme::font::XS)
                .color(theme::c().text_dim),
        )
        .stroke(Stroke::new(1.0, theme::c().stroke))
        .corner_radius(CornerRadius::same(theme::RADIUS_SM));
        ui.add(btn).on_hover_text(clear_tip).clicked()
    })
    .inner
}
