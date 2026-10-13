//! 右键菜单与下拉共用的菜单项按钮,及行右键菜单公共项组
//! (定位程序/复制远端/复制路径/复制选中文本)。

use eframe::egui;
use egui::RichText;

use crate::i18n::I18n;
use crate::ui::theme;

/// 菜单项按钮(可选禁用);禁用原因由调用方经 on_disabled_hover_text 挂载
pub(crate) fn menu_item(ui: &mut egui::Ui, text: String, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).size(theme::font::BODY)),
    )
}

/// “复制选中文本”菜单项:点击返回 true(由调用方置 App 的
/// copy_selected_pending,下一帧 logic 注入 Event::Copy,见 frame.rs)。
/// 启用态取 LabelSelectionState::has_selection——egui 不公开选中文本
/// 内容(私有 text_to_copy),复制由 egui 在 Label 绘制时自行完成
pub fn copy_selected_item(ui: &mut egui::Ui, i18n: &I18n) -> bool {
    let has = ui
        .ctx()
        .plugin::<egui::text_selection::LabelSelectionState>()
        .lock()
        .has_selection();
    menu_item(ui, i18n.t("menu-copy-selected"), has).clicked()
}

/// 行右键菜单公共尾组:定位程序 / 复制远端(`remote` 传 Some 才出现)/
/// 复制路径;动作就地执行(资源管理器定位与剪贴板),无状态返回
pub fn locate_copy_items(
    ui: &mut egui::Ui,
    i18n: &I18n,
    proc_path: Option<&str>,
    remote: Option<(&str, u16)>,
) {
    if menu_item(ui, i18n.t("menu-locate"), proc_path.is_some()).clicked()
        && let Some(path) = proc_path
    {
        crate::platform::paths::select_in_explorer(std::path::Path::new(path));
    }
    if let Some((ip, port)) = remote
        && menu_item(ui, i18n.t("menu-copy-remote"), true).clicked()
    {
        ui.ctx().copy_text(format!("{ip}:{port}"));
    }
    if menu_item(ui, i18n.t("menu-copy-path"), proc_path.is_some()).clicked()
        && let Some(path) = proc_path
    {
        ui.ctx().copy_text(path.to_owned());
    }
}
