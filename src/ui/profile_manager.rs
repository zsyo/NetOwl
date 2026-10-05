//! 配置档管理弹窗:档列表(含规则数)/ 新建 / 重命名 / 复制 / 删除。
//! 操作直接落库并即时生效;当前档不可删除,档位切换经规则页工具栏
//! 下拉完成(本弹窗只做增删改,不切档)。操作结果注入规则页 feedback。

use eframe::egui;
use egui::RichText;
use rusqlite::Connection as Db;

use crate::i18n::I18n;
use crate::rules::RuleSet;
use crate::ui::{icons, theme, widgets};

use super::rules::Feedback;

/// 管理弹窗状态(规则页 PageState 持有,切页保持)
#[derive(Default)]
pub struct ProfileMgrState {
    pub open: bool,
    /// 正在重命名的档 id(行内编辑态)
    pub renaming: Option<i64>,
    /// 名称输入草稿(新建/重命名共用)
    pub draft: String,
    /// 待确认删除:(档 id, 名称)
    pub confirm_delete: Option<(i64, String)>,
}

/// 渲染管理弹窗;管理操作不改 config,落盘由档位切换路径负责
pub fn show_modal(
    ui: &mut egui::Ui,
    db: &Db,
    rules: &RuleSet,
    state: &mut ProfileMgrState,
    i18n: &I18n,
    feedback: &mut Option<Feedback>,
) {
    if !state.open {
        return;
    }
    // 本帧完成的操作(闭包外统一反馈与草稿清理):(是否失败, 操作名, 名称或错误)
    let mut done: Option<(bool, &'static str, String)> = None;

    let modal = egui::Modal::new(egui::Id::new("profile-manager")).show(ui.ctx(), |ui| {
        ui.set_width(420.0);
        ui.add_space(theme::sp::XS);
        ui.label(
            RichText::new(i18n.t("rules-profile-manage"))
                .size(theme::font::H2)
                .strong(),
        );
        ui.add_space(theme::sp::XS);

        let profiles = RuleSet::list_profiles(db);
        let current = rules.active_profile;
        egui::ScrollArea::vertical()
            .max_height(240.0)
            .auto_shrink(false)
            .show(ui, |ui| {
                for p in &profiles {
                    let n = RuleSet::count_rules(db, p.id);
                    ui.horizontal(|ui| {
                        profile_row(
                            ui, db, rules, state, i18n, p.id, &p.name, current, n, &mut done,
                        );
                        ui.end_row();
                    });
                }
            });

        ui.add_space(theme::sp::SM);
        // 新建行:输入名称 + 按钮
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.draft)
                    .desired_width(180.0)
                    .hint_text(i18n.t("rules-profile-name-hint")),
            );
            if ui
                .button(RichText::new(i18n.t("rules-profile-new")).size(theme::font::BODY))
                .clicked()
                && let Some(name) = trim_draft(state)
            {
                match RuleSet::create_profile(db, &name) {
                    Ok(_) => done = Some((false, "新建", name)),
                    Err(e) => done = Some((true, "新建", e.to_string())),
                }
            }
        });

        // 删除确认:追加在弹窗底部(确认前保留列表上下文)
        if let Some((id, name)) = state.confirm_delete.clone() {
            let n = RuleSet::count_rules(db, id);
            ui.separator();
            ui.label(
                RichText::new(i18n.t_with_args(
                    "rules-profile-delete-text",
                    &[("name", name.clone()), ("n", n.to_string())],
                ))
                .color(theme::c().danger),
            );
            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new(i18n.t("rules-profile-cancel")).size(theme::font::BODY))
                    .clicked()
                {
                    state.confirm_delete = None;
                }
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(i18n.t("rules-profile-delete-confirm"))
                                .size(theme::font::BODY)
                                .color(theme::c().on_accent),
                        )
                        .fill(theme::c().danger),
                    )
                    .clicked()
                {
                    match RuleSet::delete_profile(db, id) {
                        Ok(()) => done = Some((false, "删除", name.clone())),
                        Err(e) => done = Some((true, "删除", e.to_string())),
                    }
                    state.confirm_delete = None;
                }
            });
        }
    });

    if modal.should_close() {
        state.open = false;
        state.renaming = None;
        state.confirm_delete = None;
        state.draft.clear();
    }
    if let Some((is_err, op, detail)) = done {
        state.draft.clear();
        state.renaming = None;
        if is_err {
            tracing::warn!("[Rules] 配置档{op}失败:{detail}");
        } else {
            tracing::info!("[Rules] 配置档{op}完成:{detail}");
        }
        let key = if is_err {
            "rules-profile-op-failed"
        } else {
            "rules-profile-op-done"
        };
        *feedback = Some(Feedback::now(
            is_err,
            i18n.t_with_args(key, &[("detail", detail)]),
        ));
    }
}

/// 档位行:当前档高亮 + 规则数;编辑态(重命名)与查看态两种形态
#[allow(clippy::too_many_arguments)]
fn profile_row(
    ui: &mut egui::Ui,
    db: &Db,
    rules: &RuleSet,
    state: &mut ProfileMgrState,
    i18n: &I18n,
    id: i64,
    name: &str,
    current: i64,
    rule_count: usize,
    done: &mut Option<(bool, &'static str, String)>,
) {
    if state.renaming == Some(id) {
        ui.add(
            egui::TextEdit::singleline(&mut state.draft)
                .desired_width(180.0)
                .hint_text(i18n.t("rules-profile-name-hint")),
        );
        if ui
            .button(RichText::new(i18n.t("rules-profile-save")).size(theme::font::BODY))
            .clicked()
            && let Some(name) = trim_draft(state)
        {
            match RuleSet::rename_profile(db, id, &name) {
                Ok(()) => *done = Some((false, "重命名", name)),
                Err(e) => *done = Some((true, "重命名", e.to_string())),
            }
        }
        if ui
            .button(RichText::new(i18n.t("rules-profile-cancel")).size(theme::font::BODY))
            .clicked()
        {
            state.renaming = None;
        }
        return;
    }

    let is_current = id == current;
    let label = if is_current {
        format!("{name} ({})", i18n.t("rules-profile-current"))
    } else {
        name.to_owned()
    };
    ui.label(
        RichText::new(label)
            .size(theme::font::BODY)
            .color(if is_current {
                theme::c().accent
            } else {
                theme::c().text
            })
            .strong(),
    );
    ui.label(theme::dim_text(
        &i18n.t_with_args("rules-profile-rule-count", &[("n", rule_count.to_string())]),
        theme::font::SM,
    ));
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        // 删除:当前档禁用(引擎运行中的档不可整档移除)
        let del_btn =
            egui::Button::new(RichText::new(icons::TRASH.to_owned()).size(theme::font::BODY));
        if ui.add_enabled(!is_current, del_btn).clicked() {
            state.confirm_delete = Some((id, name.to_owned()));
        }
        if widgets::button::icon_btn(ui, icons::COPY, None, false, true).clicked() {
            let name = i18n.t_with_args("rules-profile-copy-name", &[("name", name.to_owned())]);
            match RuleSet::copy_profile(db, id, &name) {
                Ok(_) => *done = Some((false, "复制", name)),
                Err(e) => *done = Some((true, "复制", e.to_string())),
            }
        }
        if widgets::button::icon_btn(ui, icons::PENCIL, None, false, true).clicked() {
            state.renaming = Some(id);
            state.draft = name.to_owned();
        }
    });
    let _ = rules;
}

/// 草稿非空校验并取出(trim 后);空输入不动作
fn trim_draft(state: &ProfileMgrState) -> Option<String> {
    let name = state.draft.trim().to_owned();
    (!name.is_empty()).then_some(name)
}
