//! 设置页日志分组:文件开关与级别立即生效(直接调 logging),
//! 浏览窗口入口只置位状态,另附资源管理器定位。

use eframe::egui;
use egui::RichText;

use super::{section_card, setting_row};
use crate::i18n::I18n;
use crate::logging::LogLevel;
use crate::storage::config::Config;
use crate::ui::log_window;
use crate::ui::{icons, theme, widgets};

pub(super) fn section(
    ui: &mut egui::Ui,
    config: &mut Config,
    i18n: &I18n,
    log_window: &mut log_window::PageState,
    changed: &mut bool,
) {
    section_card(
        ui,
        icons::TERMINAL,
        &i18n.t("settings-section-logging"),
        |ui| {
            setting_row(
                ui,
                &i18n.t("settings-log-file-on"),
                &i18n.t("settings-log-file-hint"),
                |ui| {
                    if widgets::toggle::toggle_switch(
                        ui,
                        &mut config.general.log_to_file,
                        egui::Id::new("settings-log-file-toggle"),
                    )
                    .changed()
                    {
                        crate::logging::set_file_enabled(config.general.log_to_file);
                        *changed = true;
                    }
                },
            );
            ui.add_space(theme::sp::SM);
            setting_row(
                ui,
                &i18n.t("settings-log-level"),
                &i18n.t("settings-log-level-hint"),
                |ui| {
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
                                    tracing::info!("[Log] 日志档位 -> {}", level.as_str());
                                    *changed = true;
                                }
                            }
                        });
                },
            );
            ui.add_space(theme::sp::SM);
            setting_row(
                ui,
                &i18n.t("settings-log-retain"),
                &i18n.t("settings-log-retain-hint"),
                |ui| {
                    // 0 = 不自动清理,显示 "-" 与采样端口 0 值同口径;
                    // 清理由 App 层每日 0 点后与启动各跑一次(按修改时间)
                    let before = config.general.log_retain_days;
                    *changed |= ui
                        .add(
                            egui::DragValue::new(&mut config.general.log_retain_days)
                                .range(0..=365)
                                .custom_formatter(|v, _| {
                                    if v < 1.0 {
                                        "-".to_owned()
                                    } else {
                                        format!(
                                            "{} {}",
                                            v as u32,
                                            i18n.t("settings-log-retain-unit")
                                        )
                                    }
                                }),
                        )
                        .changed();
                    if config.general.log_retain_days != before {
                        tracing::debug!(
                            "[Settings] 日志归档保留期 -> {} 天",
                            config.general.log_retain_days
                        );
                    }
                },
            );
            ui.add_space(theme::sp::SM);
            setting_row(
                ui,
                &i18n.t("settings-log-view"),
                &i18n.t("settings-log-view-hint"),
                |ui| {
                    if ui.button(i18n.t("settings-log-open")).clicked() {
                        log_window::open(log_window);
                    }
                    if ui.button(i18n.t("settings-log-locate")).clicked() {
                        super::locate_log_file();
                    }
                },
            );
        },
    );
}
