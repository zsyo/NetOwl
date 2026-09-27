//! 规则页:规则列表(启停/上移下移/编辑/删除)与编辑弹窗;
//! 模型与求值引擎见 rules.rs。

use eframe::egui;
use egui::{CornerRadius, RichText};
use rusqlite::Connection as Db;

use crate::i18n::I18n;
use crate::model::Protocol;
use crate::rules::{self, Action, Direction, RemoteKind, Rule, RuleSet};
use crate::theme;
use crate::wfp;

/// 工具栏行内控件最小交互高度(与历史页同因:统一行高垂直居中)
const TOOLBAR_ROW_H: f32 = 26.0;
/// 编辑弹窗控件列宽度
const FIELD_WIDTH: f32 = 220.0;

/// 规则页状态:编辑弹窗草稿(切页保持)
pub struct PageState {
    pub draft: Option<Draft>,
}

impl PageState {
    pub fn new() -> Self {
        PageState { draft: None }
    }
}

/// 编辑弹窗草稿;id < 0 表示新建
pub struct Draft {
    pub id: i64,
    pub name: String,
    /// 编辑已有规则时保留原启停状态(启停在表格开关操作)
    enabled: bool,
    pub action: Action,
    pub direction: Direction,
    pub proto: Option<Protocol>,
    pub process: String,
    pub remote_kind: RemoteKind,
    pub remote_value: String,
    pub port: u16,
    /// 保存校验错误(词条键)
    pub error: Option<&'static str>,
}

impl Draft {
    fn new_rule() -> Draft {
        Draft {
            id: -1,
            name: String::new(),
            enabled: true,
            action: Action::Block,
            direction: Direction::Any,
            proto: None,
            process: String::new(),
            remote_kind: RemoteKind::Any,
            remote_value: String::new(),
            port: 0,
            error: None,
        }
    }

    fn from_rule(r: &Rule) -> Draft {
        Draft {
            id: r.id,
            name: r.name.clone(),
            enabled: r.enabled,
            action: r.action,
            direction: r.direction,
            proto: r.proto,
            process: r.process.clone(),
            remote_kind: r.remote_kind,
            remote_value: r.remote_value.clone(),
            port: r.port,
            error: None,
        }
    }

    fn to_rule(&self, priority: i64) -> Rule {
        Rule {
            id: self.id,
            name: self.name.trim().to_owned(),
            enabled: self.enabled,
            priority,
            action: self.action,
            direction: self.direction,
            proto: self.proto,
            process: self.process.trim().to_owned(),
            remote_kind: self.remote_kind,
            remote_value: self.remote_value.trim().to_owned(),
            port: self.port,
        }
    }
}

/// 规则页主入口
pub fn show(
    ui: &mut egui::Ui,
    state: &mut PageState,
    i18n: &I18n,
    db: &Db,
    rules: &mut RuleSet,
    wfp_status: &wfp::Status,
) {
    ui.heading(theme::accent_text(&i18n.t("rules-title"), 20.0));
    ui.label(theme::dim_text(&i18n.t("rules-subtitle"), 13.0));
    ui.add_space(4.0);
    wfp_status_line(ui, i18n, wfp_status);
    ui.add_space(6.0);

    ui.style_mut().spacing.interact_size.y = TOOLBAR_ROW_H;
    ui.horizontal(|ui| {
        if ui
            .button(RichText::new(i18n.t("rules-new")).size(13.0))
            .clicked()
        {
            state.draft = Some(Draft::new_rule());
        }
    });
    ui.add_space(8.0);

    rules_table(ui, state, i18n, db, rules);
    edit_window(ui, state, i18n, db, rules);
}

/// 拦截引擎状态行:未提权/失败时用警示色提示(只读标注模式)
fn wfp_status_line(ui: &mut egui::Ui, i18n: &I18n, status: &wfp::Status) {
    match status {
        wfp::Status::Active(n) => {
            let text = i18n.t_with_args("wfp-status-active", &[("n", n.to_string())]);
            ui.label(theme::dim_text(
                &format!("{};{}", text, i18n.t("wfp-active-hint")),
                12.0,
            ));
        }
        wfp::Status::NoAdmin => {
            ui.label(
                RichText::new(i18n.t("wfp-status-noadmin"))
                    .size(12.0)
                    .color(theme::c().danger),
            );
        }
        wfp::Status::Failed(e) => {
            let text = i18n.t_with_args("wfp-status-failed", &[("err", e.clone())]);
            ui.label(RichText::new(text).size(12.0).color(theme::c().danger));
        }
        wfp::Status::Off => {
            ui.label(theme::dim_text(&i18n.t("wfp-status-off"), 12.0));
        }
    }
}

fn header(ui: &mut egui::Ui, text: String) {
    ui.label(
        RichText::new(text)
            .size(12.0)
            .strong()
            .color(theme::c().text_dim),
    );
}

/// 规则表(自上而下即优先级从高到低)
fn rules_table(
    ui: &mut egui::Ui,
    state: &mut PageState,
    i18n: &I18n,
    db: &Db,
    rules: &mut RuleSet,
) {
    if rules.rules.is_empty() {
        ui.add_space(20.0);
        ui.label(theme::dim_text(&i18n.t("rules-empty"), 14.0));
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            egui::Grid::new("rules_grid")
                .num_columns(9)
                .spacing([14.0, 7.0])
                .striped(true)
                .show(ui, |ui| {
                    for key in [
                        "rules-col-enabled",
                        "rules-col-name",
                        "rules-col-action",
                        "rules-col-direction",
                        "col-proto",
                        "col-process",
                        "rules-col-remote",
                        "col-port",
                        "rules-col-ops",
                    ] {
                        header(ui, i18n.t(key));
                    }
                    ui.end_row();

                    for i in 0..rules.rules.len() {
                        let rule = rules.rules[i].clone();
                        let mut enabled = rule.enabled;
                        if ui.checkbox(&mut enabled, "").changed() {
                            let _ = rules.set_enabled(db, rule.id, enabled);
                        }
                        ui.add(
                            egui::Label::new(
                                RichText::new(&rule.name).size(13.0).color(theme::c().text),
                            )
                            .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                        let (action_key, action_color) = match rule.action {
                            Action::Allow => ("rule-action-allow", theme::c().status_ok),
                            Action::Block => ("rule-action-block", theme::c().danger),
                        };
                        ui.label(
                            RichText::new(i18n.t(action_key))
                                .size(13.0)
                                .color(action_color),
                        );
                        ui.label(theme::dim_text(&direction_name(i18n, rule.direction), 13.0));
                        ui.label(theme::dim_text(&proto_name(i18n, rule.proto), 13.0));
                        ui.add(
                            egui::Label::new(theme::dim_text(&process_display(&rule), 13.0))
                                .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                        ui.add(
                            egui::Label::new(theme::dim_text(&remote_display(&rule), 13.0))
                                .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                        ui.label(theme::dim_text(&port_display(rule.port), 13.0));
                        ui.horizontal(|ui| {
                            if small_btn(ui, i18n.t("rules-move-up")).clicked() {
                                let _ = rules.move_rule(db, rule.id, -1);
                            }
                            if small_btn(ui, i18n.t("rules-move-down")).clicked() {
                                let _ = rules.move_rule(db, rule.id, 1);
                            }
                            if small_btn(ui, i18n.t("rules-edit")).clicked() {
                                state.draft = Some(Draft::from_rule(&rule));
                            }
                            if small_btn(ui, i18n.t("rules-delete")).clicked()
                                && let Err(e) = rules.delete(db, rule.id)
                            {
                                eprintln!("[Rules] 删除规则 {} 失败: {e}", rule.id);
                            }
                        });
                        ui.end_row();
                    }
                });
        });
}

fn small_btn(ui: &mut egui::Ui, text: String) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).size(12.0))
            .corner_radius(CornerRadius::same(theme::RADIUS_SM)),
    )
}

fn direction_name(i18n: &I18n, d: Direction) -> String {
    match d {
        Direction::Any => i18n.t("rule-direction-any"),
        Direction::Out => i18n.t("rule-direction-out"),
        Direction::In => i18n.t("rule-direction-in"),
    }
}

fn proto_name(i18n: &I18n, p: Option<Protocol>) -> String {
    match p {
        None => i18n.t("rule-proto-any"),
        Some(v) => v.as_str().to_owned(),
    }
}

fn action_text(i18n: &I18n, a: Action) -> String {
    match a {
        Action::Allow => i18n.t("rule-action-allow"),
        Action::Block => i18n.t("rule-action-block"),
    }
}

fn remote_kind_name(i18n: &I18n, k: RemoteKind) -> String {
    match k {
        RemoteKind::Any => i18n.t("rule-remote-any"),
        RemoteKind::Ip => i18n.t("rule-remote-ip"),
        RemoteKind::Domain => i18n.t("rule-remote-domain"),
    }
}

/// "*" 表示该维度不限定
fn process_display(rule: &Rule) -> String {
    if rule.process.is_empty() {
        "*".to_owned()
    } else {
        rule.process.clone()
    }
}

fn remote_display(rule: &Rule) -> String {
    if rule.remote_kind == RemoteKind::Any {
        "*".to_owned()
    } else {
        rule.remote_value.clone()
    }
}

fn port_display(port: u16) -> String {
    if port == 0 {
        "-".to_owned()
    } else {
        port.to_string()
    }
}

/// 编辑弹窗(居中);关闭按钮与取消等价(丢弃草稿)。
/// 窗口闭包内只操作草稿本体,落库与关闭在闭包外执行(避免借用冲突)
fn edit_window(
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
    egui::Window::new(RichText::new(i18n.t(title_key)).size(16.0).strong())
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
                            .font(egui::FontId::proportional(13.0)),
                    );
                    ui.end_row();

                    field_label(ui, i18n.t("rules-col-action"));
                    combo(ui, "rule-action", action_text(i18n, draft.action), |ui| {
                        for a in [Action::Allow, Action::Block] {
                            if ui
                                .selectable_label(draft.action == a, action_text(i18n, a))
                                .clicked()
                            {
                                draft.action = a;
                            }
                        }
                    });
                    ui.end_row();

                    field_label(ui, i18n.t("rules-col-direction"));
                    ui.vertical(|ui| {
                        combo(
                            ui,
                            "rule-direction",
                            direction_name(i18n, draft.direction),
                            |ui| {
                                for d in [Direction::Any, Direction::Out, Direction::In] {
                                    if ui
                                        .selectable_label(
                                            draft.direction == d,
                                            direction_name(i18n, d),
                                        )
                                        .clicked()
                                    {
                                        draft.direction = d;
                                    }
                                }
                            },
                        );
                        ui.label(theme::dim_text(&i18n.t("rules-direction-hint"), 11.0));
                    });
                    ui.end_row();

                    field_label(ui, i18n.t("col-proto"));
                    combo(ui, "rule-proto", proto_name(i18n, draft.proto), |ui| {
                        for p in [None, Some(Protocol::Tcp), Some(Protocol::Udp)] {
                            if ui
                                .selectable_label(draft.proto == p, proto_name(i18n, p))
                                .clicked()
                            {
                                draft.proto = p;
                            }
                        }
                    });
                    ui.end_row();

                    field_label(ui, i18n.t("col-process"));
                    ui.vertical(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.process)
                                .desired_width(FIELD_WIDTH)
                                .font(egui::FontId::proportional(13.0)),
                        );
                        ui.label(theme::dim_text(&i18n.t("rules-process-hint"), 11.0));
                    });
                    ui.end_row();

                    field_label(ui, i18n.t("rules-col-remote"));
                    ui.vertical(|ui| {
                        combo(
                            ui,
                            "rule-remote-kind",
                            remote_kind_name(i18n, draft.remote_kind),
                            |ui| {
                                for k in [RemoteKind::Any, RemoteKind::Ip, RemoteKind::Domain] {
                                    if ui
                                        .selectable_label(
                                            draft.remote_kind == k,
                                            remote_kind_name(i18n, k),
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
                                    .font(egui::FontId::proportional(13.0)),
                            );
                        }
                    });
                    ui.end_row();

                    field_label(ui, i18n.t("col-port"));
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut draft.port)
                                .range(0..=65535)
                                .custom_formatter(|v, _| port_display(v as u16)),
                        );
                        ui.label(theme::dim_text(&i18n.t("rules-port-any-hint"), 11.0));
                    });
                    ui.end_row();
                });
            ui.add_space(6.0);
            if let Some(err) = &draft.error {
                ui.label(
                    RichText::new(i18n.t(err))
                        .size(12.0)
                        .color(theme::c().danger),
                );
            }
            ui.add_space(4.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(RichText::new(i18n.t("rules-save")).size(13.0))
                    .clicked()
                {
                    match validate(draft) {
                        Ok(()) => save = true,
                        Err(key) => draft.error = Some(key),
                    }
                }
                if ui
                    .button(RichText::new(i18n.t("rules-cancel")).size(13.0))
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
        .selected_text(RichText::new(selected).size(13.0))
        .show_ui(ui, content);
}

fn field_label(ui: &mut egui::Ui, text: String) {
    ui.label(
        RichText::new(text)
            .size(13.0)
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
            eprintln!("[Rules] 保存规则失败: {e}");
            draft.error = Some("rules-err-save");
            false
        }
    }
}
