//! 悬浮窗右键菜单:独立窗口自绘,开关项点击后菜单保持,普通项点击后关闭。

use eframe::egui;
use egui::{Color32, Rect, Sense, Stroke};

use super::view::panel_bg;
use crate::i18n::I18n;
use crate::storage::config::FloatingBallConfig;
use crate::ui::Page;
use crate::ui::{icons, theme};

/// 菜单窗口尺寸(逻辑点)
pub(super) const MENU_W: f32 = 176.0;
pub(super) const MENU_H: f32 = 168.0;
/// 菜单行高
const ROW_H: f32 = 26.0;

/// 菜单动作(菜单项点击产生,show() 在 viewport 闭包后统一应用;
/// menu_body 不直接改 state/cfg,避免多 viewport 闭包借用冲突)
pub(super) enum MenuAction {
    /// 唤出主窗口并落到指定页
    Show(Page),
    ToggleTopmost,
    ToggleAutoHide,
    /// 关闭悬浮窗
    Close,
}

/// 菜单窗口内容:背景 + 分组分隔的菜单项;点击产生动作
pub(super) fn menu_body(
    ui: &mut egui::Ui,
    cfg: &FloatingBallConfig,
    i18n: &I18n,
    action: &mut Option<MenuAction>,
) {
    let rect = ui.max_rect();
    panel_bg(ui, rect);
    let row_rect = |cy: f32| {
        Rect::from_min_size(
            egui::pos2(rect.left() + theme::sp::SM, cy),
            egui::vec2(rect.width() - theme::sp::SM * 2.0, ROW_H),
        )
    };
    let mut cy = rect.top() + theme::sp::SM;

    // 显示窗口
    if item_row(
        ui,
        egui::Id::new("ball-menu-show"),
        row_rect(cy),
        i18n.t("ball-menu-show"),
        None,
    ) {
        *action = Some(MenuAction::Show(Page::Map));
    }
    cy += ROW_H + theme::sp::XS;
    cy = group_separator(ui, rect, cy);
    cy += theme::sp::XS;

    // 开关组:窗口置顶显示 / 贴边自动隐藏
    if item_row(
        ui,
        egui::Id::new("ball-menu-topmost"),
        row_rect(cy),
        i18n.t("ball-menu-topmost"),
        Some(cfg.always_on_top),
    ) {
        *action = Some(MenuAction::ToggleTopmost);
    }
    cy += ROW_H;
    if item_row(
        ui,
        egui::Id::new("ball-menu-auto-hide"),
        row_rect(cy),
        i18n.t("ball-menu-auto-hide"),
        Some(cfg.auto_hide_edge),
    ) {
        *action = Some(MenuAction::ToggleAutoHide);
    }
    cy += ROW_H + theme::sp::XS;
    cy = group_separator(ui, rect, cy);
    cy += theme::sp::XS;

    // 设置 / 关闭悬浮窗
    if item_row(
        ui,
        egui::Id::new("ball-menu-settings"),
        row_rect(cy),
        i18n.t("ball-menu-settings"),
        None,
    ) {
        *action = Some(MenuAction::Show(Page::Settings));
    }
    cy += ROW_H;
    if item_row(
        ui,
        egui::Id::new("ball-menu-close"),
        row_rect(cy),
        i18n.t("ball-menu-close"),
        None,
    ) {
        *action = Some(MenuAction::Close);
    }
}

/// 分组间隔分隔线(左右留边距,上下由调用方补间距);返回下一 y
fn group_separator(ui: &mut egui::Ui, rect: Rect, cy: f32) -> f32 {
    ui.painter().line_segment(
        [
            egui::pos2(rect.left() + theme::sp::SM, cy),
            egui::pos2(rect.right() - theme::sp::SM, cy),
        ],
        Stroke::new(1.0, theme::c().stroke),
    );
    cy + 1.0
}

/// 菜单行:悬停高亮 + 可选勾选标记(未勾选留占位对齐)+ 文字;返回点击
fn item_row(
    ui: &mut egui::Ui,
    id: egui::Id,
    rect: Rect,
    text: String,
    checked: Option<bool>,
) -> bool {
    let resp = ui.interact(rect, id, Sense::click());
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
    let mut text_x = rect.left() + theme::sp::SM + 4.0;
    if let Some(checked) = checked {
        // 勾选标记:勾选时文字色,未勾选时占位对齐(透明)
        let color = if checked {
            theme::c().text
        } else {
            Color32::TRANSPARENT
        };
        let galley = painter.layout_no_wrap(
            icons::CHECK_LG.to_string(),
            egui::FontId::proportional(theme::font::SM),
            color,
        );
        painter.galley(
            egui::pos2(text_x, center_y - galley.rect.height() * 0.5),
            galley,
            color,
        );
        text_x += 18.0;
    }
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::proportional(theme::font::SM),
        theme::c().text,
    );
    painter.galley(
        egui::pos2(text_x, center_y - galley.rect.height() * 0.5),
        galley,
        theme::c().text,
    );
    resp.clicked()
}
