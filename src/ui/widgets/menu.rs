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

/// “复制选中文本”菜单项:仅在有快照时出现(快照 = 拖选期间经
/// CopyCatcher 捕获的选中文本,与选中态生死解耦——egui 会在点击
/// 菜单时清除 selection,若直接读 selection,按住跨帧会导致菜单重排、
/// 点击错位;快照在菜单集合保持稳定)。点击 = 写剪贴板 + 清除选中
/// 高亮与快照(一次性,点后菜单不再出现该项),返回 true 由调用方推气泡
pub fn copy_selected_item(ui: &mut egui::Ui, i18n: &I18n, selected: &mut Option<String>) -> bool {
    if selected.is_none() || !menu_item(ui, i18n.t("menu-copy-selected"), true).clicked() {
        return false;
    }
    if let Some(text) = selected.take() {
        ui.ctx().copy_text(text);
    }
    // 选中态随本次菜单操作结束(清除高亮)
    ui.ctx()
        .plugin::<egui::text_selection::LabelSelectionState>()
        .lock()
        .clear_selection();
    true
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
