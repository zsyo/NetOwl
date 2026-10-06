//! 历史页行右键菜单:明细行(删除该条/定位/复制)与汇总行(删除该进程
//! 全部/定位/复制)。删除动作由调用方执行,菜单函数不持 state。

use eframe::egui;

use crate::i18n::I18n;
use crate::storage::history_query::{DetailRow, SummaryRow};
use crate::ui::widgets;

/// 明细行右键菜单;返回"删除该条"是否被点击(删除由调用方执行,
/// 菜单函数不持 state 避免与行数据借用冲突)
pub(super) fn detail_menu(ui: &mut egui::Ui, r: &DetailRow, i18n: &I18n) -> bool {
    let delete = widgets::menu::menu_item(ui, i18n.t("history-menu-delete-row"), true).clicked();
    widgets::menu::locate_copy_items(
        ui,
        i18n,
        r.proc_path.as_deref(),
        Some((r.remote_ip.to_string().as_str(), r.remote_port)),
    );
    delete
}

/// 汇总行右键菜单;返回"删除该进程全部"是否被点击(转确认弹窗)
pub(super) fn summary_menu(ui: &mut egui::Ui, r: &SummaryRow, i18n: &I18n) -> bool {
    let delete =
        widgets::menu::menu_item(ui, i18n.t("history-menu-delete-process"), true).clicked();
    widgets::menu::locate_copy_items(ui, i18n, r.proc_path.as_deref(), None);
    delete
}
