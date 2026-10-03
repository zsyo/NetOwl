//! 规则编辑弹窗:名称/动作/方向/协议/进程/远端/端口表单与校验落库。
//! 窗口闭包内只操作草稿本体,落库与关闭在闭包外执行(避免借用冲突)。

use eframe::egui;
use egui::RichText;
use rusqlite::Connection as Db;

use super::draft::Draft;
use super::labels;
use super::{PageState, primary_btn};
use crate::i18n::I18n;
use crate::model::Protocol;
use crate::rules::{self, Action, Direction, RemoteKind, RuleSet};
use crate::ui::theme;

/// 编辑弹窗控件列宽度
const FIELD_WIDTH: f32 = 220.0;

/// 编辑弹窗(居中);关闭按钮与取消等价(丢弃草稿)
pub(super) fn edit_window(
    ui: &mut egui::Ui,
    state: &mut PageState,
    i18n: &I18n,
    db: &Db,
    rules: &mut RuleSet,
) {
    let Some(draft) = state.draft.as_mut() else {
        return;
    };
    let title_key = if draft.id < 0 {
        "rules-new-title"
    } else {
        "rules-edit-title"
    };
    let mut open = true;
    let mut save = false;
    let mut close = false;
    egui::Window::new(
        RichText::new(i18n.t(title_key))
            .size(theme::font::H2)
            .strong(),
    )
    .open(&mut open)
    .collapsible(false)
    .resizable(false)
    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
    .show(ui, |ui| {
        egui::Grid::new("rule_edit_grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                field_label(ui, i18n.t("rules-col-name"));
                ui.add(
                    egui::TextEdit::singleline(&mut draft.name)
                        .desired_width(FIELD_WIDTH)
                        .font(egui::FontId::proportional(theme::font::BODY)),
                );
                ui.end_row();

                field_label(ui, i18n.t("rules-col-action"));
                combo(
                    ui,
                    "rule-action",
                    labels::action_text(i18n, draft.action),
                    |ui| {
                        for a in [Action::Allow, Action::Block] {
                            if ui
                                .selectable_label(draft.action == a, labels::action_text(i18n, a))
                                .clicked()
                            {
                                draft.action = a;
                            }
                        }
                    },
                );
                ui.end_row();

                field_label(ui, i18n.t("rules-col-direction"));
                ui.vertical(|ui| {
                    combo(
                        ui,
                        "rule-direction",
                        labels::direction_name(i18n, draft.direction),
                        |ui| {
                            for d in [Direction::Any, Direction::Out, Direction::In] {
                                if ui
                                    .selectable_label(
                                        draft.direction == d,
                                        labels::direction_name(i18n, d),
                                    )
                                    .clicked()
                                {
                                    draft.direction = d;
                                }
                            }
                        },
                    );
                    ui.label(theme::dim_text(
                        &i18n.t("rules-direction-hint"),
                        theme::font::XS,
                    ));
                });
                ui.end_row();

                field_label(ui, i18n.t("col-proto"));
                combo(
                    ui,
                    "rule-proto",
                    labels::proto_name(i18n, draft.proto),
                    |ui| {
                        for p in [None, Some(Protocol::Tcp), Some(Protocol::Udp)] {
                            if ui
                                .selectable_label(draft.proto == p, labels::proto_name(i18n, p))
                                .clicked()
                            {
                                draft.proto = p;
                            }
                        }
                    },
                );
                ui.end_row();

                field_label(ui, i18n.t("col-process"));
                ui.vertical(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.process)
                            .desired_width(FIELD_WIDTH)
                            .font(egui::FontId::proportional(theme::font::BODY)),
                    );
                    ui.label(theme::dim_text(
                        &i18n.t("rules-process-hint"),
                        theme::font::XS,
                    ));
                });
                ui.end_row();

                field_label(ui, i18n.t("rules-col-remote"));
                ui.vertical(|ui| {
                    combo(
                        ui,
                        "rule-remote-kind",
                        labels::remote_kind_name(i18n, draft.remote_kind),
                        |ui| {
                            for k in [RemoteKind::Any, RemoteKind::Ip, RemoteKind::Domain] {
                                if ui
                                    .selectable_label(
                                        draft.remote_kind == k,
                                        labels::remote_kind_name(i18n, k),
                                    )
                                    .clicked()
                                {
                                    draft.remote_kind = k;
                                }
                            }
                        },
                    );
                    if draft.remote_kind != RemoteKind::Any {
                        let hint = if draft.remote_kind == RemoteKind::Ip {
                            i18n.t("rules-remote-ip-hint")
                        } else {
                            i18n.t("rules-remote-domain-hint")
                        };
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.remote_value)
                                .hint_text(hint)
                                .desired_width(FIELD_WIDTH)
                                .font(egui::FontId::proportional(theme::font::BODY)),
                        );
                    }
                });
                ui.end_row();

                field_label(ui, i18n.t("col-port"));
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut draft.port)
                            .range(0..=65535)
                            .custom_formatter(|v, _| labels::port_display(v as u16)),
                    );
                    ui.label(theme::dim_text(
                        &i18n.t("rules-port-any-hint"),
                        theme::font::XS,
                    ));
                });
                ui.end_row();
            });
        ui.add_space(6.0);
        if let Some(err) = &draft.error {
            ui.label(
                RichText::new(i18n.t(err))
                    .size(theme::font::SM)
                    .color(theme::c().danger),
            );
        }
        ui.add_space(4.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if primary_btn(ui, i18n.t("rules-save")).clicked() {
                match validate(draft) {
                    Ok(()) => save = true,
                    Err(key) => draft.error = Some(key),
                }
            }
            if ui
                .button(RichText::new(i18n.t("rules-cancel")).size(theme::font::BODY))
                .clicked()
            {
                close = true;
            }
        });
    });
    if !open {
        state.draft = None;
    }
    if close {
        state.draft = None;
    }
    if save {
        let ok = state
            .draft
            .as_mut()
            .is_some_and(|d| save_draft(d, db, rules));
        if ok {
            state.draft = None;
        }
    }
}

/// 弹窗用统一宽度下拉
fn combo(ui: &mut egui::Ui, salt: &str, selected: String, content: impl FnOnce(&mut egui::Ui)) {
    egui::ComboBox::from_id_salt(salt)
        .width(FIELD_WIDTH)
        .selected_text(RichText::new(selected).size(theme::font::BODY))
        .show_ui(ui, content);
}

fn field_label(ui: &mut egui::Ui, text: String) {
    ui.label(
        RichText::new(text)
            .size(theme::font::BODY)
            .strong()
            .color(theme::c().text_dim),
    );
}

/// 保存前校验;失败返回错误词条键
fn validate(draft: &Draft) -> Result<(), &'static str> {
    if draft.name.trim().is_empty() {
        return Err("rules-err-name");
    }
    match draft.remote_kind {
        RemoteKind::Any => Ok(()),
        RemoteKind::Ip => {
            if rules::parse_net(&draft.remote_value).is_some() {
                Ok(())
            } else {
                Err("rules-err-remote")
            }
        }
        RemoteKind::Domain => {
            if draft.remote_value.trim().is_empty() {
                Err("rules-err-remote-empty")
            } else {
                Ok(())
            }
        }
    }
}

/// 校验通过后落库;id < 0 新建,否则覆盖更新;落库失败在草稿上显示错误
fn save_draft(draft: &mut Draft, db: &Db, rules: &mut RuleSet) -> bool {
    let result = if draft.id < 0 {
        rules.insert(db, draft.to_rule(0))
    } else {
        let priority = rules
            .rules
            .iter()
            .find(|r| r.id == draft.id)
            .map(|r| r.priority)
            .unwrap_or(0);
        rules.update(db, &draft.to_rule(priority))
    };
    match result {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("[Rules] 保存规则失败: {e}");
            draft.error = Some("rules-err-save");
            false
        }
    }
}
