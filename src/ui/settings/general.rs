//! 设置页常规分组:界面语言(运行时扫描 locales)、深浅主题、悬浮球
//! 开关、托盘图标常驻与开机自启动。

use eframe::egui;
use egui::RichText;

use super::{section_card, setting_row};
use crate::i18n::I18n;
use crate::storage::config::Config;
use crate::ui::{icons, theme, widgets};

pub(super) fn section(ui: &mut egui::Ui, config: &mut Config, i18n: &mut I18n, changed: &mut bool) {
    section_card(ui, icons::GEAR, &i18n.t("settings-section-general"), |ui| {
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
                            let label =
                                RichText::new(name)
                                    .size(theme::font::BODY)
                                    .color(if selected {
                                        theme::c().accent
                                    } else {
                                        theme::c().text
                                    });
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
        ui.add_space(theme::sp::SM);
        setting_row(
            ui,
            &i18n.t("settings-floating-ball"),
            &i18n.t("settings-floating-ball-hint"),
            |ui| {
                // 悬浮球窗口由 App 层按开关显隐;贴边位置记忆随开关保留
                *changed |= widgets::toggle::toggle_switch(
                    ui,
                    &mut config.floating_ball.enabled,
                    egui::Id::new("settings-ball-toggle"),
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
                *changed |= widgets::toggle::toggle_switch(
                    ui,
                    &mut config.general.tray_pinned,
                    egui::Id::new("settings-tray-toggle"),
                )
                .changed();
            },
        );
        ui.add_space(theme::sp::SM);
        setting_row(
            ui,
            &i18n.t("settings-autostart"),
            &i18n.t("settings-autostart-hint"),
            |ui| {
                // 注册表 Run 键,写入失败由 App 层定时重试;自启动走静默到托盘
                *changed |= widgets::toggle::toggle_switch(
                    ui,
                    &mut config.general.autostart,
                    egui::Id::new("settings-autostart-toggle"),
                )
                .changed();
            },
        );
        ui.add_space(theme::sp::SM);
        setting_row(
            ui,
            &i18n.t("settings-hotkey"),
            &i18n.t("settings-hotkey-hint"),
            |ui| {
                // 预设组合下拉(关闭 + 三个 Ctrl+Alt 组合);切换由 App 层
                // 热注销重注册,失败(组合被占用)toast 明示
                // 克隆当前值:闭包内要写回 config,不能持有其借用
                let current = config.general.hotkey.clone();
                let label = |p: &str| {
                    if p.is_empty() {
                        i18n.t("settings-hotkey-off")
                    } else {
                        crate::platform::global_hotkey::display(p)
                    }
                };
                egui::ComboBox::from_id_salt("settings-hotkey-select")
                    .width(180.0)
                    .selected_text(RichText::new(label(&current)).size(theme::font::BODY))
                    .show_ui(ui, |ui| {
                        for p in crate::platform::global_hotkey::PRESETS {
                            let text = RichText::new(label(p)).size(theme::font::BODY);
                            if ui.selectable_label(p == &current, text).clicked() {
                                config.general.hotkey = (*p).to_owned();
                                *changed = true;
                            }
                        }
                    });
            },
        );
    });
}
