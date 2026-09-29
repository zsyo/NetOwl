//! 规则页:规则列表(启停/上移下移/编辑/删除)与编辑弹窗;
//! 模型与求值引擎见 rules.rs。

use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, CornerRadius, FontId, RichText};
use rusqlite::Connection as Db;

use crate::i18n::I18n;
use crate::model::Protocol;
use crate::rules::wfp;
use crate::rules::{self, Action, Direction, RemoteKind, Rule, RuleSet};
use crate::ui::{TOOLBAR_ROW_H, icons, theme, widgets};

/// 编辑弹窗控件列宽度
const FIELD_WIDTH: f32 = 220.0;
/// 工具栏反馈消息展示时长
const FEEDBACK_TIMEOUT: Duration = Duration::from_secs(6);
/// 导出对话框默认文件名
const EXPORT_FILE_NAME: &str = "netowl-rules.json";

/// 规则页状态:编辑弹窗草稿(切页保持)
pub struct PageState {
    pub draft: Option<Draft>,
    /// 最近一次导入/导出的反馈消息(工具栏右侧,超时自动消失)
    pub feedback: Option<Feedback>,
}

impl PageState {
    pub fn new() -> Self {
        PageState {
            draft: None,
            feedback: None,
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
    fn now(is_err: bool, text: String) -> Feedback {
        Feedback {
            is_err,
            text,
            at: Instant::now(),
        }
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
            local_port: 0,
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
    widgets::header::page_header(ui, &i18n.t("rules-title"), &i18n.t("rules-subtitle"));
    ui.add_space(theme::sp::XS);
    wfp_status_line(ui, i18n, wfp_status);
    ui.add_space(theme::sp::SM);

    ui.style_mut().spacing.interact_size.y = TOOLBAR_ROW_H;
    ui.horizontal(|ui| {
        if primary_btn(ui, format!("{}  {}", icons::PLUS_LG, i18n.t("rules-new"))).clicked() {
            state.draft = Some(Draft::new_rule());
        }
        if ui
            .button(RichText::new(i18n.t("rules-export")).size(theme::font::BODY))
            .clicked()
        {
            do_export(rules, state, i18n);
        }
        if ui
            .button(RichText::new(i18n.t("rules-import")).size(theme::font::BODY))
            .clicked()
        {
            do_import(db, rules, state, i18n);
        }
        feedback_label(ui, state);
    });
    ui.add_space(theme::sp::SM);

    rules_table(ui, state, i18n, db, rules);
    edit_window(ui, state, i18n, db, rules);
}

/// 导出:原生保存对话框选路径,持久规则写为 JSON 文件
fn do_export(rules: &RuleSet, state: &mut PageState, i18n: &I18n) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("JSON", &["json"])
        .set_file_name(EXPORT_FILE_NAME)
        .save_file()
    else {
        return;
    };
    let n = rules.rules.iter().filter(|r| r.id > 0).count();
    let result = std::fs::write(&path, rules.export_json()).map(|_| n);
    state.feedback = Some(match result {
        Ok(n) => Feedback::now(
            false,
            i18n.t_with_args("rules-export-done", &[("n", n.to_string())]),
        ),
        Err(e) => Feedback::now(
            true,
            i18n.t_with_args("rules-export-failed", &[("err", e.to_string())]),
        ),
    });
}

/// 导入:原生打开对话框选 JSON 文件,规则追加为最低优先级
fn do_import(db: &Db, rules: &mut RuleSet, state: &mut PageState, i18n: &I18n) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("JSON", &["json"])
        .pick_file()
    else {
        return;
    };
    let result = std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| rules.import_json(db, &text));
    state.feedback = Some(match result {
        Ok((n, skipped)) => Feedback::now(
            false,
            i18n.t_with_args(
                "rules-import-done",
                &[("n", n.to_string()), ("skipped", skipped.to_string())],
            ),
        ),
        Err(e) => Feedback::now(true, i18n.t_with_args("rules-import-failed", &[("err", e)])),
    });
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

/// 表格列宽(逻辑点);表头与数据列同宽,add_sized 居中
const COL_ENABLED: f32 = 44.0;
const COL_NAME: f32 = 170.0;
const COL_ACTION: f32 = 56.0;
const COL_DIRECTION: f32 = 64.0;
const COL_PROTO: f32 = 56.0;
const COL_PROCESS: f32 = 160.0;
const COL_REMOTE: f32 = 160.0;
const COL_PORT: f32 = 44.0;
const COL_OPS: f32 = 120.0;

fn header_cell(ui: &mut egui::Ui, w: f32, text: String) {
    ui.add_sized(
        [w, 16.0],
        egui::Label::new(
            RichText::new(text)
                .size(theme::font::SM)
                .strong()
                .color(theme::c().text_dim),
        ),
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
        ui.add_space(theme::sp::XL);
        ui.label(theme::dim_text(&i18n.t("rules-empty"), theme::font::H3));
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let table_left = ui.max_rect().left();
            let table_right = ui.max_rect().right();
            egui::Grid::new("rules_grid")
                .num_columns(9)
                .spacing([14.0, 9.0])
                .show(ui, |ui| {
                    header_cell(ui, COL_ENABLED, i18n.t("rules-col-enabled"));
                    header_cell(ui, COL_NAME, i18n.t("rules-col-name"));
                    header_cell(ui, COL_ACTION, i18n.t("rules-col-action"));
                    header_cell(ui, COL_DIRECTION, i18n.t("rules-col-direction"));
                    header_cell(ui, COL_PROTO, i18n.t("col-proto"));
                    header_cell(ui, COL_PROCESS, i18n.t("col-process"));
                    header_cell(ui, COL_REMOTE, i18n.t("rules-col-remote"));
                    header_cell(ui, COL_PORT, i18n.t("col-port"));
                    header_cell(ui, COL_OPS, i18n.t("rules-col-ops"));
                    ui.end_row();

                    // 删除会缩短 rules 数组,同帧继续按旧索引渲染会越界
                    // 崩溃:删除后立即结束本帧表格,下一帧按新列表重建
                    let mut removed = false;
                    for i in 0..rules.rules.len() {
                        let rule = rules.rules[i].clone();
                        let row_top = ui.cursor().top();
                        let mut enabled = rule.enabled;
                        let toggle = widgets::toggle::toggle_switch(ui, &mut enabled);
                        if toggle.changed() {
                            let _ = rules.set_enabled(db, rule.id, enabled);
                        }
                        // 会话临时规则(负 id):名称加标注并以弱化色显示
                        let is_temp = rule.id < 0;
                        let name_text = if is_temp {
                            format!("{} ({})", rule.name, i18n.t("rules-temp-badge"))
                        } else {
                            rule.name.clone()
                        };
                        ui.add_sized(
                            [COL_NAME, 18.0],
                            egui::Label::new(
                                RichText::new(name_text).size(theme::font::BODY).color(
                                    if is_temp {
                                        theme::c().text_dim
                                    } else {
                                        theme::c().text
                                    },
                                ),
                            )
                            .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                        let (action_key, action_kind) = match rule.action {
                            Action::Allow => ("rule-action-allow", widgets::badge::BadgeKind::Ok),
                            Action::Block => {
                                ("rule-action-block", widgets::badge::BadgeKind::Danger)
                            }
                        };
                        ui.allocate_ui_with_layout(
                            egui::vec2(COL_ACTION, 18.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                widgets::badge::badge(ui, &i18n.t(action_key), action_kind);
                            },
                        );
                        ui.add_sized(
                            [COL_DIRECTION, 18.0],
                            egui::Label::new(theme::dim_text(
                                &direction_name(i18n, rule.direction),
                                theme::font::BODY,
                            )),
                        );
                        ui.allocate_ui_with_layout(
                            egui::vec2(COL_PROTO, 18.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                widgets::badge::badge(
                                    ui,
                                    &proto_name(i18n, rule.proto),
                                    widgets::badge::BadgeKind::Neutral,
                                );
                            },
                        );
                        ui.add_sized(
                            [COL_PROCESS, 18.0],
                            egui::Label::new(theme::dim_text(
                                &process_display(&rule),
                                theme::font::BODY,
                            ))
                            .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                        ui.add_sized(
                            [COL_REMOTE, 18.0],
                            egui::Label::new(theme::dim_text(
                                &remote_display(&rule),
                                theme::font::BODY,
                            ))
                            .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                        ui.add_sized(
                            [COL_PORT, 18.0],
                            egui::Label::new(theme::dim_text(
                                &port_display(rule.port),
                                theme::font::BODY,
                            )),
                        );
                        ui.allocate_ui_with_layout(
                            egui::vec2(COL_OPS, 26.0),
                            egui::Layout::left_to_right(egui::Align::Min),
                            |ui| {
                                ui.style_mut().spacing.item_spacing.x = 2.0;
                                if icon_btn(ui, icons::ARROW_UP, i18n.t("rules-move-up"), false)
                                    .clicked()
                                {
                                    let _ = rules.move_rule(db, rule.id, -1);
                                }
                                if icon_btn(ui, icons::ARROW_DOWN, i18n.t("rules-move-down"), false)
                                    .clicked()
                                {
                                    let _ = rules.move_rule(db, rule.id, 1);
                                }
                                if icon_btn(ui, icons::PENCIL, i18n.t("rules-edit"), false)
                                    .clicked()
                                {
                                    state.draft = Some(Draft::from_rule(&rule));
                                }
                                if icon_btn(ui, icons::TRASH, i18n.t("rules-delete"), true)
                                    .clicked()
                                {
                                    if let Err(e) = rules.delete(db, rule.id) {
                                        tracing::warn!("[Rules] 删除规则 {} 失败: {e}", rule.id);
                                    }
                                    removed = true;
                                }
                            },
                        );
                        ui.end_row();
                        // 行悬停高亮(与连接页同款)
                        let row_bottom = ui.cursor().top() - 9.0;
                        let row_rect = egui::Rect::from_min_max(
                            egui::pos2(table_left, row_top),
                            egui::pos2(table_right, row_bottom),
                        );
                        if ui.rect_contains_pointer(row_rect) {
                            widgets::table::row_background(ui, row_rect, theme::c().hover_bg);
                        }
                        if removed {
                            break;
                        }
                    }
                });
        });
}

/// 主按钮:强调色填充 + 反色文字(页内首要动作)
fn primary_btn(ui: &mut egui::Ui, text: String) -> egui::Response {
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

/// 图标操作钮:24px 方形,悬停显底色;danger 用于删除(悬停警示)
fn icon_btn(ui: &mut egui::Ui, glyph: &str, tip: String, danger: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 22.0), egui::Sense::click());
    if resp.hovered() {
        let bg = if danger {
            theme::c().danger.gamma_multiply(0.12)
        } else {
            theme::c().hover_bg
        };
        ui.painter()
            .rect_filled(rect, CornerRadius::same(theme::RADIUS_SM), bg);
    }
    let color = if resp.hovered() {
        if danger {
            theme::c().danger
        } else {
            theme::c().text
        }
    } else {
        theme::c().text_dim
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(theme::font::SM),
        color,
    );
    resp.on_hover_text(tip)
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
                                    .selectable_label(draft.direction == d, direction_name(i18n, d))
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
                            .custom_formatter(|v, _| port_display(v as u16)),
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
