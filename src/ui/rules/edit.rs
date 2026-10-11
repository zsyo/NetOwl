//! 规则编辑弹窗:名称/动作/方向/协议/进程/远端/端口表单与校验落库。
//! 窗口闭包内只操作草稿本体,落库与关闭在闭包外执行(避免借用冲突)。

use eframe::egui;
use egui::RichText;
use rusqlite::Connection as Db;

use super::PageState;
use super::draft::Draft;
use super::labels;
use crate::i18n::I18n;
use crate::model::Protocol;
use crate::rules::{self, Action, Direction, RemoteKind, RuleSet};
use crate::ui::theme;
use crate::ui::widgets::button::primary_btn_sized;

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
    .default_width(360.0)
    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
    .show(ui, |ui| {
        // 名称
        form_row(ui, i18n.t("rules-col-name"), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut draft.name)
                    .desired_width(FIELD_WIDTH)
                    .font(egui::FontId::proportional(theme::font::BODY)),
            );
        });
        // 动作
        form_row(ui, i18n.t("rules-col-action"), |ui| {
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
        });
        // 方向
        form_row(ui, i18n.t("rules-col-direction"), |ui| {
            combo(
                ui,
                "rule-direction",
                labels::direction_name(i18n, draft.direction),
                |ui| {
                    for d in [Direction::Any, Direction::Out, Direction::In] {
                        if ui
                            .selectable_label(draft.direction == d, labels::direction_name(i18n, d))
                            .clicked()
                        {
                            draft.direction = d;
                        }
                    }
                },
            );
        });
        form_hint(ui, &i18n.t("rules-direction-hint"));
        // 协议
        form_row(ui, i18n.t("col-proto"), |ui| {
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
        });
        // 进程
        form_row(ui, i18n.t("col-process"), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut draft.process)
                    .desired_width(FIELD_WIDTH)
                    .font(egui::FontId::proportional(theme::font::BODY)),
            );
        });
        form_hint(ui, &i18n.t("rules-process-hint"));
        // 远端
        form_row(ui, i18n.t("rules-col-remote"), |ui| {
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
        });
        if draft.remote_kind != RemoteKind::Any {
            let hint = if draft.remote_kind == RemoteKind::Ip {
                i18n.t("rules-remote-ip-hint")
            } else {
                i18n.t("rules-remote-domain-hint")
            };
            form_row_no_label(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut draft.remote_value)
                        .hint_text(hint)
                        .desired_width(FIELD_WIDTH)
                        .font(egui::FontId::proportional(theme::font::BODY)),
                );
            });
            if draft.remote_kind == RemoteKind::Domain {
                form_hint(ui, &i18n.t("rules-remote-domain-desc"));
            }
        }
        // 端口
        form_row(ui, i18n.t("col-port"), |ui| {
            // DragValue 默认无边框按钮样式,包一层与输入框同源的
            // 底色描边(取当前主题 Visuals)
            let vis = &ui.style().visuals;
            egui::Frame::new()
                .fill(vis.text_edit_bg_color())
                .stroke(vis.widgets.inactive.bg_stroke)
                .corner_radius(vis.widgets.inactive.corner_radius)
                .show(ui, |ui| {
                    ui.add_sized(
                        [64.0, 24.0],
                        egui::DragValue::new(&mut draft.port)
                            .range(0..=65535)
                            .custom_formatter(|v, _| labels::port_display(v as u16)),
                    )
                });
            ui.add_space(theme::sp::SM);
            ui.label(theme::dim_text(
                &i18n.t("rules-port-any-hint"),
                theme::font::XS,
            ));
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
        // 取消/保存按钮组水平居中:两按钮定宽,左右各放对称占位。
        // 占位必须把行内全部 item_spacing(三个子件间共 3 个)算进
        // 总宽,否则行宽每帧超出窗口——egui Window 宽度只增不减,
        // 弹窗会持续涨到全屏宽(实测)
        ui.horizontal(|ui| {
            let sp = ui.spacing().item_spacing.x;
            let group = CANCEL_W + sp + SAVE_W;
            let side = ((ui.available_width() - group - 2.0 * sp) / 2.0).max(0.0);
            ui.allocate_exact_size(egui::vec2(side, ROW_H), egui::Sense::hover());
            if ui
                .add_sized(
                    [CANCEL_W, ROW_H],
                    egui::Button::new(
                        RichText::new(i18n.t("rules-cancel")).size(theme::font::BODY),
                    ),
                )
                .clicked()
            {
                close = true;
            }
            if primary_btn_sized(ui, i18n.t("rules-save"), true, egui::vec2(SAVE_W, ROW_H))
                .clicked()
            {
                match validate(draft) {
                    Ok(()) => save = true,
                    Err(key) => draft.error = Some(key),
                }
            }
            ui.allocate_exact_size(egui::vec2(side, ROW_H), egui::Sense::hover());
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

/// 弹窗表单左列(字段名)宽度:容纳中英字段名
const LABEL_W: f32 = 64.0;
/// 表单控件行高(与删除按钮 24px 同源)
const ROW_H: f32 = 24.0;
/// 底部按钮定宽(组宽 = 两宽 + 行间距,居中占位按此计算)
const CANCEL_W: f32 = 64.0;
const SAVE_W: f32 = 68.0;

/// 表单行:左列字段名为定宽占位 + painter 绝对绘制(垂直居中)+
/// 右控件。不用 Label 子件——horizontal 的 cross 对齐会把不同高的
/// 子件按各自内容错位放置,占位等高后定位完全确定
fn form_row(ui: &mut egui::Ui, label: String, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(LABEL_W, ROW_H), egui::Sense::hover());
        ui.painter().text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(theme::font::BODY),
            theme::c().text_dim,
        );
        add(ui);
    });
}

/// 控件列延续行(无字段名,左列占位保持控件列对齐)
fn form_row_no_label(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_exact_size(egui::vec2(LABEL_W, ROW_H), egui::Sense::hover());
        add(ui);
    });
}

/// 控件列下方说明(缩进至控件列)
fn form_hint(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(LABEL_W);
        ui.label(theme::dim_text(text, theme::font::XS));
    });
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
            if valid_domain(&draft.remote_value) {
                Ok(())
            } else {
                Err("rules-err-remote-domain")
            }
        }
    }
}

/// 域名条件格式校验:ASCII 字母/数字/连字符/点、每段 1..=63 且连字符不
/// 居首尾、总长 <=253、至少两段(FQDN,单标签永不命中 rDNS 结果)。
/// DNS 大小写不敏感;IDN 域名需 punycode(xn-- 前缀)形式,rDNS 返回的
/// 也是 ASCII 记录
fn valid_domain(input: &str) -> bool {
    let s = input.trim().to_lowercase();
    if s.is_empty() || s.len() > 253 || !s.contains('.') {
        return false;
    }
    s.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
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
