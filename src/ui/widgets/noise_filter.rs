//! 噪音过滤下拉:历史页与连接页筛选栏共用(漏斗按钮 + 隐藏本地/
//! 局域网两项可勾选)。选中态以前置对钩标示,不用 selectable_label
//! 的选中底色——两项同选时底色相同,对钩才能直观区分。

use eframe::egui;
use egui::{Color32, RichText, containers::PopupCloseBehavior, containers::menu::MenuConfig};

use super::super::{TOOLBAR_ROW_H, icons, theme};
use crate::i18n::I18n;

/// 菜单行高(与工具栏行高一致,保证点击目标)
const ROW_H: f32 = TOOLBAR_ROW_H;

/// 漏斗按钮 + 两项可勾选;任一项开启时按钮 accent 高亮。勾选点击后
/// 菜单保持(CloseOnClickOutside),可连续勾两项,点外部/Esc 关闭。
/// 返回本帧是否有切换(调用方据此刷新查询并持久化 config)
pub fn noise_filter(
    ui: &mut egui::Ui,
    i18n: &I18n,
    hide_local: &mut bool,
    hide_lan: &mut bool,
) -> bool {
    let mut changed = false;
    let mut button = egui::Button::new(
        RichText::new(format!(
            "{}  {}  {}",
            icons::FUNNEL,
            i18n.t("filter-noise"),
            icons::CARET_DOWN_FILL
        ))
        .size(theme::font::BODY),
    )
    .corner_radius(egui::CornerRadius::same(theme::RADIUS_SM));
    if *hide_local || *hide_lan {
        button = button.fill(theme::c().accent_soft);
    }
    egui::containers::menu::MenuButton::from_button(button)
        .config(MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside))
        .ui(ui, |ui| {
            ui.with_layout(
                egui::Layout::top_down(egui::Align::LEFT).with_cross_justify(true),
                |ui| {
                    if check_item(ui, *hide_local, i18n.t("filter-hide-local")) {
                        *hide_local = !*hide_local;
                        changed = true;
                    }
                    ui.add_space(theme::sp::XS);
                    if check_item(ui, *hide_lan, i18n.t("filter-hide-lan")) {
                        *hide_lan = !*hide_lan;
                        changed = true;
                    }
                },
            );
        });
    changed
}

/// 可勾选菜单项:整行可点(悬停高亮),前置对钩标示选中(未勾选留
/// 透明占位保持文字对齐)。返回是否被点击
fn check_item(ui: &mut egui::Ui, checked: bool, text: String) -> bool {
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_H),
        egui::Sense::click(),
    );
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(theme::RADIUS_SM),
            theme::c().hover_bg,
        );
    }
    let painter = ui.painter();
    let center_y = rect.center().y;
    let mut text_x = rect.left() + theme::sp::SM;
    // 对钩:勾选取 accent,未勾选透明占位
    let check_color = if checked {
        theme::c().accent
    } else {
        Color32::TRANSPARENT
    };
    let galley = painter.layout_no_wrap(
        icons::CHECK_LG.to_owned(),
        egui::FontId::proportional(theme::font::SM),
        check_color,
    );
    painter.galley(
        egui::pos2(text_x, center_y - galley.rect.height() * 0.5),
        galley,
        check_color,
    );
    text_x += 18.0;
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::proportional(theme::font::BODY),
        theme::c().text,
    );
    painter.galley(
        egui::pos2(text_x, center_y - galley.rect.height() * 0.5),
        galley,
        theme::c().text,
    );
    resp.clicked()
}
