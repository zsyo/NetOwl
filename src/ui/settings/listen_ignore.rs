//! 监听提醒忽略程序管理弹窗:程序名列表 + 行内忽略开关 + 删除;
//! 开关关闭仅停用该行(恢复提醒),行保留可随时再开启。数据存
//! config.general.listen_ignores,改动经 changed 标记走防抖落盘。

use eframe::egui;
use egui::RichText;

use crate::i18n::I18n;
use crate::storage::config::{Config, ListenIgnore};
use crate::ui::{ListenIgnoreWin, icons, theme, widgets};

/// 渲染管理弹窗(monitor 卡"忽略程序管理"按钮打开);
/// 返回是否改动了配置
pub(super) fn show_modal(
    ui: &mut egui::Ui,
    config: &mut Config,
    state: &mut ListenIgnoreWin,
    i18n: &I18n,
) -> bool {
    if !state.open {
        return false;
    }
    let mut changed = false;
    // 循环中删除按索引延迟到帧末(targets_section 同模式)
    let mut removed: Option<usize> = None;
    let modal = egui::Modal::new(egui::Id::unique("listen-ignore-mgr")).show(ui.ctx(), |ui| {
        ui.set_width(400.0);
        ui.add_space(theme::sp::XS);
        ui.label(
            RichText::new(i18n.t("settings-listen-ignore-title"))
                .size(theme::font::H2)
                .strong(),
        );
        ui.label(theme::dim_text(
            &i18n.t("settings-listen-ignore-hint"),
            theme::font::XS,
        ));
        ui.add_space(theme::sp::XS);

        egui::ScrollArea::vertical()
            .max_height(260.0)
            .auto_shrink(false)
            .show(ui, |ui| {
                if config.general.listen_ignores.is_empty() {
                    ui.add_space(theme::sp::SM);
                    ui.label(theme::dim_text(
                        &i18n.t("settings-listen-ignore-empty"),
                        theme::font::SM,
                    ));
                    return;
                }
                // 表头:程序名 | 忽略 | 删除
                ui.horizontal(|ui| {
                    ui.label(theme::dim_text(
                        &i18n.t("settings-listen-ignore-col-name"),
                        theme::font::XS,
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(theme::sp::LG);
                        ui.label(theme::dim_text(
                            &i18n.t("settings-listen-ignore-col-ignore"),
                            theme::font::XS,
                        ));
                    });
                });
                for (i, e) in config.general.listen_ignores.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&e.name)
                                .monospace()
                                .size(theme::font::SM)
                                .color(theme::c().text),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::button::icon_btn(
                                ui,
                                icons::TRASH,
                                Some(i18n.t("settings-quality-remove")),
                                true,
                                true,
                            )
                            .clicked()
                            {
                                removed = Some(i);
                            }
                            changed |= widgets::toggle::toggle_switch(
                                ui,
                                &mut e.ignore,
                                egui::Id::unique(("listen-ignore-toggle", i)),
                            )
                            .changed();
                        });
                    });
                }
            });

        ui.add_space(theme::sp::SM);
        // 添加行:输入 + 回车/按钮提交;同名已存在时置为开启忽略
        let mut submit = false;
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut state.input)
                    .desired_width(200.0)
                    .hint_text("svchost.exe")
                    .align(egui::Align2::LEFT_CENTER),
            );
            submit |= resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui
                .button(RichText::new(i18n.t("settings-listen-ignore-add")).size(theme::font::BODY))
                .clicked()
            {
                submit = true;
            }
        });
        if submit {
            let name = state.input.trim().to_owned();
            if !name.is_empty() {
                if let Some(e) = config
                    .general
                    .listen_ignores
                    .iter_mut()
                    .find(|e| e.name.eq_ignore_ascii_case(&name))
                {
                    e.ignore = true;
                } else {
                    config
                        .general
                        .listen_ignores
                        .push(ListenIgnore { name, ignore: true });
                }
                state.input.clear();
                changed = true;
            }
        }
    });

    if modal.should_close() {
        state.open = false;
        state.input.clear();
    }
    if let Some(i) = removed {
        config.general.listen_ignores.remove(i);
        changed = true;
    }
    changed
}
