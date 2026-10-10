//! 设置页常规分组:界面语言(运行时扫描 locales)、深浅主题、悬浮球
//! 开关、托盘图标常驻与开机自启动。

use eframe::egui;
use egui::RichText;

use super::{section_card, setting_row};
use crate::i18n::I18n;
use crate::storage::config::Config;
use crate::ui::{HotkeyCapture, icons, theme, widgets};

pub(super) fn section(
    ui: &mut egui::Ui,
    config: &mut Config,
    i18n: &mut I18n,
    hotkey_capture: &mut HotkeyCapture,
    changed: &mut bool,
) {
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
                hotkey_row(ui, config, i18n, hotkey_capture, changed);
            },
        );
    });
}

/// 全局快捷键行:自定义捕获(点击录入框 -> 按下组合键即记录)而非预设
/// 下拉;Esc 取消,裸键(无修饰键)忽略。录制结果由 App 层 sync_hotkey
/// 热注销重注册,失败(组合被占用)toast 明示
fn hotkey_row(
    ui: &mut egui::Ui,
    config: &mut Config,
    i18n: &I18n,
    capture: &mut HotkeyCapture,
    changed: &mut bool,
) {
    // 捕获中逐帧扫描键盘;Win 键不在 egui 修饰符里,见 capture_combo
    if capture.active {
        match capture_combo(ui, capture) {
            ComboCapture::Done(combo) => {
                capture.active = false;
                if config.general.hotkey != combo {
                    config.general.hotkey = combo;
                    *changed = true;
                }
            }
            ComboCapture::Cancel => capture.active = false,
            ComboCapture::None => {}
        }
    }
    // 录入框按钮:显示当前组合(Windows 惯例 Win 而非 Super),捕获中
    // 显示等待提示;再次点击取消捕获
    let text = if capture.active {
        i18n.t("settings-hotkey-capturing")
    } else if config.general.hotkey.is_empty() {
        i18n.t("settings-hotkey-none")
    } else {
        crate::platform::global_hotkey::display(&config.general.hotkey)
    };
    let resp = ui.add_sized(
        [140.0, 26.0],
        egui::Button::new(RichText::new(text).size(theme::font::BODY)).selected(capture.active),
    );
    if resp.clicked() {
        capture.active = !capture.active;
    }
    // 关闭按钮:仅已设置组合时显示(空串 = 不注册)
    if !config.general.hotkey.is_empty() && !capture.active {
        ui.add_space(theme::sp::SM);
        if ui
            .add_sized(
                [60.0, 26.0],
                egui::Button::new(
                    RichText::new(i18n.t("settings-hotkey-off")).size(theme::font::BODY),
                ),
            )
            .clicked()
        {
            config.general.hotkey = String::new();
            *changed = true;
        }
    }
}

/// 单帧捕获结果
enum ComboCapture {
    /// 无(继续等待)
    None,
    /// Esc 取消
    Cancel,
    /// 录制到组合(序列化形式,如 "ctrl+alt+n"/"win+d")
    Done(String),
}

/// 扫描本帧键盘事件录制组合键:Ctrl/Alt/Shift 取 Modifiers 状态,
/// Win 键 egui 不跟踪为修饰符(egui-winit 仅在 mac 映射 super),靠
/// SuperLeft/SuperRight 的按下/释放事件跨帧维持;首个带修饰键的
/// 非修饰键即记录,裸键忽略(全局热键至少带一个修饰键)
fn capture_combo(ui: &mut egui::Ui, capture: &mut HotkeyCapture) -> ComboCapture {
    use egui::{Event, Key};
    let mut win = capture.win_held;
    let mut result = ComboCapture::None;
    for ev in ui.input(|i| i.events.clone()) {
        let Event::Key {
            key,
            pressed,
            modifiers,
            ..
        } = ev
        else {
            continue;
        };
        match key {
            Key::SuperLeft | Key::SuperRight => win = pressed,
            Key::Escape if pressed => {
                capture.win_held = win;
                return ComboCapture::Cancel;
            }
            _ if pressed => {
                let name = key.name().to_ascii_lowercase();
                let capturable = (name.len() == 1 && name.as_bytes()[0].is_ascii_alphanumeric())
                    || matches!(
                        name.as_str(),
                        "f1" | "f2"
                            | "f3"
                            | "f4"
                            | "f5"
                            | "f6"
                            | "f7"
                            | "f8"
                            | "f9"
                            | "f10"
                            | "f11"
                            | "f12"
                    );
                if !capturable {
                    continue;
                }
                // 序列化顺序 = 显示顺序(display 归一化):主流应用式
                // Ctrl+Shift+Alt+Win+键
                let mut combo = String::new();
                if modifiers.ctrl {
                    combo.push_str("ctrl+");
                }
                if modifiers.shift {
                    combo.push_str("shift+");
                }
                if modifiers.alt {
                    combo.push_str("alt+");
                }
                if win {
                    combo.push_str("win+");
                }
                if !combo.is_empty() {
                    combo.push_str(&name);
                    result = ComboCapture::Done(combo);
                }
            }
            _ => {}
        }
    }
    capture.win_held = win;
    result
}
