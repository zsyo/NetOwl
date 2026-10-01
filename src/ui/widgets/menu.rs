//! 右键菜单与下拉共用的菜单项按钮。

use eframe::egui;
use egui::RichText;

use crate::ui::theme;

/// 菜单项按钮(可选禁用);禁用原因由调用方经 on_disabled_hover_text 挂载
pub(crate) fn menu_item(ui: &mut egui::Ui, text: String, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).size(theme::font::BODY)),
    )
}
