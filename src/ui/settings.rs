//! 设置页:分组卡片布局(外观/监控/日志),行式设置项(左标签 + 右控件)。

use eframe::egui;
use egui::{Align, Layout, RichText};

use super::{icons, log_window, theme, widgets};
use crate::collector::CollectorKind;
use crate::i18n::I18n;
use crate::logging::LogLevel;
use crate::storage::config::Config;

/// 设置卡片最大宽度(避免超宽屏上行控件散布过远)
const CARD_MAX_W: f32 = 640.0;

/// 设置页:语言切换(词条即时生效)、主题、数据源;返回是否直接改动了配置。
/// 日志区:级别下拉与文件开关立即生效(直接调 logging),窗口入口只置位状态
pub(super) fn settings_ui(
    ui: &mut egui::Ui,
    config: &mut Config,
    i18n: &mut I18n,
    log_window: &mut log_window::PageState,
) -> bool {
    widgets::header::page_header(ui, &i18n.t("settings-title"), "");
    ui.add_space(theme::sp::MD);

    let mut changed = false;

    // ---- 外观:语言 / 主题 ----
    section_card(
        ui,
        icons::PALETTE,
        &i18n.t("settings-section-appearance"),
        |ui| {
            setting_row(
                ui,
                &i18n.t("settings-language"),
                &i18n.t("settings-language-hint"),
                |ui| {
                    let current_name = i18n
                        .available_langs
                        .iter()
                        .find(|info| info.code == i18n.current_lang)
                        .map(|info| info.name.clone())
                        .unwrap_or_else(|| i18n.current_lang.clone());
                    egui::ComboBox::from_id_salt("settings-language-select")
                        .width(180.0)
                        .selected_text(current_name)
                        .show_ui(ui, |ui| {
                            for (code, name) in i18n.lang_codes_and_names() {
                                let selected = code == i18n.current_lang;
                                let label = RichText::new(name).size(theme::font::BODY).color(
                                    if selected {
                                        theme::c().accent
                                    } else {
                                        theme::c().text
                                    },
                                );
                                if ui.selectable_label(selected, label).clicked() {
                                    i18n.set_language(code);
                                }
                            }
                        });
                },
            );
            ui.add_space(theme::sp::SM);
            setting_row(
                ui,
                &i18n.t("settings-theme"),
                &i18n.t("settings-theme-hint"),
                |ui| {
                    // 主题分段选择:切换立即生效(调色板与 Visuals 同步刷新),
                    // 落盘由 App 层 sync_theme_to_config 检测
                    let items = [
                        (&*i18n.t("theme-dark"), icons::MOON),
                        (&*i18n.t("theme-light"), icons::BRIGHTNESS_HIGH),
                    ];
                    let current = if theme::is_dark() { 0 } else { 1 };
                    if let Some(i) = widgets::segmented::segmented(ui, &items, current) {
                        theme::set_theme(i == 0, ui.ctx());
                    }
                },
            );
        },
    );
    ui.add_space(theme::sp::MD);

    // ---- 监控:数据源 / 历史保留期 / 新连接询问 / 托盘常驻 ----
    section_card(
        ui,
        icons::ETHERNET,
        &i18n.t("settings-section-monitoring"),
        |ui| {
            setting_row(
                ui,
                &i18n.t("settings-datasource"),
                &i18n.t("settings-datasource-hint"),
                |ui| {
                    // 切换后由 logic 检测配置变化并重建采集器
                    let current = CollectorKind::from_config(&config.general.collector);
                    let name = |i18n: &I18n, kind: CollectorKind| match kind {
                        CollectorKind::Real => i18n.t("datasource-real"),
                        CollectorKind::Mock => i18n.t("datasource-mock"),
                    };
                    egui::ComboBox::from_id_salt("settings-datasource-select")
                        .width(180.0)
                        .selected_text(name(i18n, current))
                        .show_ui(ui, |ui| {
                            for kind in [CollectorKind::Real, CollectorKind::Mock] {
                                let selected = current == kind;
                                let label = RichText::new(name(i18n, kind))
                                    .size(theme::font::BODY)
                                    .color(if selected {
                                        theme::c().accent
                                    } else {
                                        theme::c().text
                                    });
                                if ui.selectable_label(selected, label).clicked() {
                                    config.general.collector = kind.as_config().to_owned();
                                }
                            }
                        });
                },
            );
            ui.add_space(theme::sp::SM);
            setting_row(
                ui,
                &i18n.t("settings-history-days"),
                &i18n.t("settings-history-days-hint"),
                |ui| {
                    // 0 = 不自动清理
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut config.general.history_days)
                                .range(0..=365)
                                .suffix(format!(" {}", i18n.t("settings-history-days-unit"))),
                        )
                        .changed();
                },
            );
            ui.add_space(theme::sp::SM);
            setting_row(
                ui,
                &i18n.t("settings-ask"),
                &i18n.t("settings-ask-hint"),
                |ui| {
                    // 开启后未命中规则的公网新连接弹窗询问
                    changed |= widgets::toggle::toggle_switch(
                        ui,
                        &mut config.general.ask_connections,
                        egui::Id::new("settings-ask-toggle"),
                    )
                    .changed();
                },
            );
            ui.add_space(theme::sp::SM);
            setting_row(
                ui,
                &i18n.t("settings-tray-pin"),
                &i18n.t("settings-tray-pin-hint"),
                |ui| {
                    // 注册表 IsPromoted,写入失败静默保持系统默认;新会话生效
                    changed |= widgets::toggle::toggle_switch(
                        ui,
                        &mut config.general.tray_pinned,
                        egui::Id::new("settings-tray-toggle"),
                    )
                    .changed();
                },
            );
        },
    );
    ui.add_space(theme::sp::MD);

    // ---- 日志:级别 / 文件开关 / 查看 ----
    section_card(
        ui,
        icons::TERMINAL,
        &i18n.t("settings-section-logging"),
        |ui| {
            ui.horizontal(|ui| {
                // 右侧控件更宽(下拉 + 开关 + 按钮),左列相应多留空间
                let left_w = (ui.available_width() - 390.0).max(120.0);
                ui.allocate_ui(egui::vec2(left_w, 0.0), |ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(i18n.t("settings-log"))
                                .size(theme::font::BODY)
                                .strong()
                                .color(theme::c().text),
                        );
                        ui.add(
                            egui::Label::new(theme::dim_text(
                                &i18n.t("settings-log-hint"),
                                theme::font::XS,
                            ))
                            .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                    });
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(i18n.t("settings-log-view")).clicked() {
                        log_window::open(log_window);
                    }
                    if ui
                        .checkbox(
                            &mut config.general.log_to_file,
                            i18n.t("settings-log-file-on"),
                        )
                        .changed()
                    {
                        crate::logging::set_file_enabled(config.general.log_to_file);
                        changed = true;
                    }
                    let current = LogLevel::parse(&config.general.log_level);
                    egui::ComboBox::from_id_salt("settings-log-level")
                        .width(110.0)
                        .selected_text(i18n.t(log_window::level_key(current)))
                        .show_ui(ui, |ui| {
                            for level in LogLevel::ALL {
                                if ui
                                    .selectable_label(
                                        current == level,
                                        RichText::new(i18n.t(log_window::level_key(level)))
                                            .size(theme::font::BODY),
                                    )
                                    .clicked()
                                {
                                    config.general.log_level = level.as_str().to_owned();
                                    crate::logging::set_level(level);
                                    changed = true;
                                }
                            }
                        });
                });
            });
        },
    );

    changed
}

/// 设置分组卡片:标题行(图标 + 标题)+ 内容
fn section_card<R>(
    ui: &mut egui::Ui,
    icon: &str,
    title: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) {
    widgets::card::card(ui, |ui| {
        ui.set_max_width(CARD_MAX_W);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(icon)
                    .size(theme::font::H3)
                    .color(theme::c().accent),
            );
            ui.label(
                RichText::new(title)
                    .size(theme::font::H3)
                    .strong()
                    .color(theme::c().text),
            );
        });
        ui.add_space(theme::sp::SM);
        add(ui);
    });
}

/// 设置项行:左侧标签与说明(限宽防与右侧控件重叠),右侧控件(垂直居中)
fn setting_row<R>(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    control: impl FnOnce(&mut egui::Ui) -> R,
) {
    ui.horizontal(|ui| {
        let left_w = (ui.available_width() - CTRL_AREA_W).max(120.0);
        ui.allocate_ui(egui::vec2(left_w, 0.0), |ui| {
            ui.vertical(|ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(label)
                            .size(theme::font::BODY)
                            .strong()
                            .color(theme::c().text),
                    )
                    .wrap_mode(egui::TextWrapMode::Truncate),
                );
                ui.add(
                    egui::Label::new(theme::dim_text(hint, theme::font::XS))
                        .wrap_mode(egui::TextWrapMode::Truncate),
                );
            });
        });
        ui.with_layout(Layout::right_to_left(Align::Center), control);
    });
}

/// 右侧控件区预留宽度(最宽控件为 180 宽下拉,含余量)
const CTRL_AREA_W: f32 = 260.0;
