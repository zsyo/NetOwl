//! 历史页行右键菜单:明细行(删除该条/定位/复制)与汇总行(删除该进程
//! 全部/定位/复制)。删除动作由调用方执行,菜单函数不持 state。

use eframe::egui;

use crate::i18n::I18n;
use crate::platform::paths;
use crate::storage::history_query::{DetailRow, SummaryRow};
use crate::ui::widgets;

/// 明细行右键菜单;返回"删除该条"是否被点击(删除由调用方执行,
/// 菜单函数不持 state 避免与行数据借用冲突)
pub(super) fn detail_menu(ui: &mut egui::Ui, r: &DetailRow, i18n: &I18n) -> bool {
    let delete = widgets::menu::menu_item(ui, i18n.t("history-menu-delete-row"), true).clicked();
    if widgets::menu::menu_item(ui, i18n.t("menu-locate"), r.proc_path.is_some()).clicked()
        && let Some(path) = &r.proc_path
    {
        paths::select_in_explorer(std::path::Path::new(path));
    }
    if widgets::menu::menu_item(ui, i18n.t("menu-copy-remote"), true).clicked() {
        ui.ctx()
            .copy_text(format!("{}:{}", r.remote_ip, r.remote_port));
    }
    if widgets::menu::menu_item(ui, i18n.t("menu-copy-path"), r.proc_path.is_some()).clicked()
        && let Some(path) = &r.proc_path
    {
        ui.ctx().copy_text(path.clone());
    }
    delete
}

/// 汇总行右键菜单;返回"删除该进程全部"是否被点击(转确认弹窗)
pub(super) fn summary_menu(ui: &mut egui::Ui, r: &SummaryRow, i18n: &I18n) -> bool {
    let delete =
        widgets::menu::menu_item(ui, i18n.t("history-menu-delete-process"), true).clicked();
    if widgets::menu::menu_item(ui, i18n.t("menu-locate"), r.proc_path.is_some()).clicked()
        && let Some(path) = &r.proc_path
    {
        paths::select_in_explorer(std::path::Path::new(path));
    }
    if widgets::menu::menu_item(ui, i18n.t("menu-copy-path"), r.proc_path.is_some()).clicked()
        && let Some(path) = &r.proc_path
    {
        ui.ctx().copy_text(path.clone());
    }
    delete
}
