//! 历史页批量删除确认弹窗:聚合组/整进程删除的二次确认(danger 按钮),
//! Esc/遮罩点击/取消按钮均放弃删除。

use eframe::egui;
use egui::RichText;
use rusqlite::Connection as Db;

use crate::i18n::I18n;
use crate::storage::history_query;
use crate::ui::theme;

/// 批量删除确认弹窗(Modal);Esc/遮罩点击/取消按钮均放弃删除
pub(super) fn confirm_delete_modal(
    ui: &mut egui::Ui,
    state: &mut history_query::PageState,
    i18n: &I18n,
    db: &Db,
) {
    let Some(pending) = state.pending_delete.clone() else {
        return;
    };
    let mut confirmed = false;
    let mut cancelled = false;
    let modal = egui::Modal::new(egui::Id::new("history_delete_confirm")).show(ui.ctx(), |ui| {
        ui.set_width(340.0);
        ui.add_space(theme::sp::XS);
        ui.label(
            RichText::new(i18n.t("history-delete-confirm-title"))
                .size(theme::font::H2)
                .strong(),
        );
        ui.add_space(theme::sp::XS);
        let text = match (pending.proto, pending.remote_ip) {
            (Some(_), Some(ip)) => i18n.t_with_args(
                "history-delete-group-text",
                &[
                    ("process", pending.process.clone()),
                    ("remote", ip.to_string()),
                ],
            ),
            _ => i18n.t_with_args(
                "history-delete-process-text",
                &[("process", pending.process.clone())],
            ),
        };
        ui.label(
            RichText::new(text)
                .size(theme::font::BODY)
                .color(theme::c().text),
        );
        ui.add_space(theme::sp::MD);
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new(i18n.t("history-delete-cancel")).size(theme::font::BODY))
                .clicked()
            {
                cancelled = true;
            }
            confirmed |= ui
                .add(
                    egui::Button::new(
                        RichText::new(i18n.t("history-delete-confirm"))
                            .size(theme::font::BODY)
                            .color(theme::c().on_accent),
                    )
                    .fill(theme::c().danger),
                )
                .clicked();
        });
    });
    if confirmed {
        let result = match (pending.proto, pending.remote_ip) {
            (Some(p), Some(ip)) => {
                history_query::delete_aggregate_group(db, &pending.process, p, ip)
            }
            _ => history_query::delete_process(db, &pending.process),
        };
        match result {
            Ok(n) => {
                state.dirty = true;
                tracing::info!("[History] 已删除 {n} 条历史记录");
            }
            Err(e) => tracing::warn!("[History] 删除历史记录失败: {e}"),
        }
        state.pending_delete = None;
    } else if cancelled || modal.should_close() {
        state.pending_delete = None;
    }
}
