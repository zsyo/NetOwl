//! 规则页:规则列表(启停/上移下移/编辑/删除)与编辑弹窗;
//! 模型与求值引擎见 crate::rules。

mod draft;
mod edit;
mod import_export;
mod labels;
mod table;

use std::time::{Duration, Instant};

use eframe::egui;
use egui::{CornerRadius, RichText};
use rusqlite::Connection as Db;

use super::profile_manager;
use crate::i18n::I18n;
use crate::rules::wfp;
use crate::rules::{Profile, RuleSet};
use crate::ui::{TOOLBAR_ROW_H, icons, theme, widgets};

/// 工具栏反馈消息展示时长
const FEEDBACK_TIMEOUT: Duration = Duration::from_secs(6);

/// 规则页状态:编辑弹窗草稿(切页保持)
pub struct PageState {
    pub draft: Option<draft::Draft>,
    /// 最近一次导入/导出的反馈消息(工具栏右侧,超时自动消失)
    pub feedback: Option<Feedback>,
    /// 配置档管理弹窗状态
    pub profiles: profile_manager::ProfileMgrState,
    /// 档位下拉列表缓存:每帧 list_profiles 查库改按需重载;管理弹窗
    /// 打开期间直接失效(弹窗内增删改档),关闭后下一帧重建
    pub(crate) profiles_cache: Option<Vec<Profile>>,
}

impl PageState {
    pub fn new() -> Self {
        PageState {
            draft: None,
            feedback: None,
            profiles: profile_manager::ProfileMgrState::default(),
            profiles_cache: None,
        }
    }
}

/// 工具栏反馈消息
pub struct Feedback {
    is_err: bool,
    text: String,
    at: Instant,
}

impl Feedback {
    pub fn now(is_err: bool, text: String) -> Feedback {
        Feedback {
            is_err,
            text,
            at: Instant::now(),
        }
    }
}

/// 规则页主入口;返回是否改动了配置(档位切换,由 App 层标脏落盘)
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    state: &mut PageState,
    i18n: &I18n,
    db: &Db,
    config: &mut crate::storage::config::Config,
    rules: &mut RuleSet,
    wfp_status: &wfp::Status,
    row_hover: &mut widgets::table::RowHover,
) -> bool {
    let mut changed = false;
    widgets::header::page_header(ui, &i18n.t("rules-title"), &i18n.t("rules-subtitle"));
    ui.add_space(theme::sp::XS);
    wfp_status_line(ui, i18n, wfp_status);
    ui.add_space(theme::sp::SM);

    ui.horizontal(|ui| {
        // 行高抬升只作用于本工具栏行(style_mut 泄漏到整页会把表格
        // Grid 的最小行高一并抬到 26)
        ui.style_mut().spacing.interact_size.y = TOOLBAR_ROW_H;
        if primary_btn(ui, format!("{}  {}", icons::PLUS_LG, i18n.t("rules-new"))).clicked() {
            state.draft = Some(draft::Draft::new_rule());
        }
        if ui
            .button(RichText::new(i18n.t("rules-export")).size(theme::font::BODY))
            .clicked()
        {
            import_export::do_export(rules, state, i18n);
        }
        if ui
            .button(RichText::new(i18n.t("rules-import")).size(theme::font::BODY))
            .clicked()
        {
            import_export::do_import(db, rules, state, i18n);
        }
        ui.add_space(theme::sp::MD);
        // 档位切换:重载目标档规则(临时规则保留),写 config 落盘
        if state.profiles.open || state.profiles_cache.is_none() {
            state.profiles_cache = Some(RuleSet::list_profiles(db));
        }
        let profiles = state.profiles_cache.as_ref().unwrap();
        let current_name = profiles
            .iter()
            .find(|p| p.id == rules.active_profile)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        egui::ComboBox::from_id_salt("rules-profile-select")
            .width(140.0)
            .selected_text(RichText::new(current_name).size(theme::font::BODY))
            .show_ui(ui, |ui| {
                for p in profiles {
                    let selected = p.id == rules.active_profile;
                    let label = RichText::new(&p.name)
                        .size(theme::font::BODY)
                        .color(if selected {
                            theme::c().accent
                        } else {
                            theme::c().text
                        });
                    if ui.selectable_label(selected, label).clicked() && !selected {
                        rules.switch_profile(db, p.id);
                        config.general.profile_id = p.id;
                        changed = true;
                    }
                }
            });
        if ui
            .button(RichText::new(i18n.t("rules-manage")).size(theme::font::BODY))
            .clicked()
        {
            state.profiles.open = true;
        }
        feedback_label(ui, state);
    });
    ui.add_space(theme::sp::SM);

    table::rules_table(ui, state, i18n, db, rules, row_hover);
    edit::edit_window(ui, state, i18n, db, rules);
    profile_manager::show_modal(
        ui,
        db,
        rules,
        &mut state.profiles,
        i18n,
        &mut state.feedback,
    );
    changed
}

/// 主按钮:强调色填充 + 反色文字(页内首要动作)
pub(super) fn primary_btn(ui: &mut egui::Ui, text: String) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(text)
                .size(theme::font::BODY)
                .strong()
                .color(theme::c().on_accent),
        )
        .fill(theme::c().accent)
        .corner_radius(CornerRadius::same(theme::RADIUS_MD)),
    )
}

/// 工具栏右侧反馈消息(成功绿色/失败警示色,超时自动消失)
fn feedback_label(ui: &mut egui::Ui, state: &mut PageState) {
    if state
        .feedback
        .as_ref()
        .is_some_and(|fb| fb.at.elapsed() > FEEDBACK_TIMEOUT)
    {
        state.feedback = None;
    }
    if let Some(fb) = &state.feedback {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let color = if fb.is_err {
                theme::c().danger
            } else {
                theme::c().status_ok
            };
            ui.label(RichText::new(&fb.text).size(theme::font::SM).color(color));
        });
    }
}

/// 拦截引擎状态行:图标 + 说明(未提权/失败时用警示色提示,只读标注模式)
fn wfp_status_line(ui: &mut egui::Ui, i18n: &I18n, status: &wfp::Status) {
    let (glyph, color, text) = match status {
        wfp::Status::Active(n) => {
            let t = i18n.t_with_args("wfp-status-active", &[("n", n.to_string())]);
            (
                icons::SHIELD_CHECK,
                theme::c().text_dim,
                format!("{};{}", t, i18n.t("wfp-active-hint")),
            )
        }
        wfp::Status::NoAdmin => (
            icons::SHIELD_FILL_X,
            theme::c().danger,
            i18n.t("wfp-status-noadmin"),
        ),
        wfp::Status::Failed(e) => (
            icons::SHIELD_FILL_X,
            theme::c().danger,
            i18n.t_with_args("wfp-status-failed", &[("err", e.clone())]),
        ),
        wfp::Status::Off => (icons::SHIELD, theme::c().text_dim, i18n.t("wfp-status-off")),
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(glyph).size(theme::font::SM).color(color));
        ui.label(RichText::new(text).size(theme::font::SM).color(color));
    });
}
