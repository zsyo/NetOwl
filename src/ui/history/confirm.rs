//! 历史页批量删除确认弹窗:聚合组/整进程删除的二次确认(danger 按钮),
//! Esc/遮罩点击/取消按钮均放弃删除。

use eframe::egui;
use egui::RichText;

use crate::i18n::I18n;
use crate::storage::history_query;
use crate::ui::theme;

/// 批量删除确认弹窗(Modal);Esc/遮罩点击/取消按钮均放弃删除
pub(super) fn confirm_delete_modal(
    ui: &mut egui::Ui,
    state: &mut history_query::PageState,
    i18n: &I18n,
) {
    let Some(pending) = state.pending_delete.clone() else {
        return;
    };
    let mut confirmed = false;
    let mut cancelled = false;
    let modal = egui::Modal::new(egui::Id::unique("history_delete_confirm")).show(ui.ctx(), |ui| {
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
        // 删除在工作线程执行(独立连接):大库全表 DELETE 秒级,同步
        // 执行会冻结 UI;确认即关弹窗,列表转加载态,结果由 app 层收割
        let (tx, rx) = std::sync::mpsc::channel();
        let job = pending.clone();
        std::thread::spawn(move || {
            let db = crate::storage::db::open();
            let result = match (job.proto, job.remote_ip) {
                (Some(p), Some(ip)) => {
                    history_query::delete_aggregate_group(&db, &job.process, p, ip)
                }
                _ => history_query::delete_process(&db, &job.process),
            };
            // 发送失败 = 接收端已关闭(进程退出),静默退出
            let _ = tx.send(result);
        });
        state.delete_rx = Some(rx);
        state.rows = history_query::Rows::empty(state.view);
        state.loading = true;
        state.pending_delete = None;
    } else if cancelled || modal.should_close() {
        state.pending_delete = None;
    }
}
