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
    ctx_fonts: &[crate::platform::fonts::FontEntry],
    ctx_font_filter: &mut String,
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
            &i18n.t("settings-font"),
            &i18n.t("settings-font-hint"),
            |ui| {
                font_row(ui, config, i18n, ctx_fonts, ctx_font_filter, changed);
            },
        );
        ui.add_space(theme::sp::SM);
        setting_row(
            ui,
            &i18n.t("settings-count-format"),
            &i18n.t("settings-count-format-hint"),
            |ui| {
                // 大数字单位制:auto 跟随界面语言(中文万/亿、西文 K/M/B);
                // 覆盖选项用单位字符本身做标签,中英界面均自明
                let items = [
                    (&*i18n.t("count-format-auto"), ""),
                    (&*i18n.t("count-format-wan"), ""),
                    (&*i18n.t("count-format-kmb"), ""),
                ];
                let current = match config.general.count_format.as_str() {
                    "wan" => 1,
                    "kmb" => 2,
                    _ => 0,
                };
                if let Some(i) = widgets::segmented::segmented(ui, &items, current) {
                    config.general.count_format = ["auto", "wan", "kmb"][i].to_owned();
                    *changed = true;
                }
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

/// 界面字体行:标准 ComboBox 下拉(与语言下拉同型同宽),弹窗内顶部
/// 搜索框 + 滚动字体列表。点击项只写 config 并标脏,实际字体切换由
/// App 层 sync_font 统一下发(失败可 toast 明示)。首项固定
/// "默认(微软雅黑)";其余项按搜索词大小写不敏感包含过滤。按钮与
/// 列表中的长字体名省略显示,悬停气泡给全名;点按钮开合弹窗即清空
/// 搜索词,每次打开都从完整列表起步
fn font_row(
    ui: &mut egui::Ui,
    config: &mut Config,
    i18n: &I18n,
    fonts: &[crate::platform::fonts::FontEntry],
    filter: &mut String,
    changed: &mut bool,
) {
    use egui::TextWrapMode;
    use egui::containers::PopupCloseBehavior;

    // 与语言下拉同宽(控件区按 180 宽下拉预留);ComboBox 的 width
    // 只是下限(combo_box.rs:347),不限可用宽(控件区共 260)时长字体
    // 名会把按钮撑过 180,故用定宽槽位把可用宽也钉住
    const ROW_W: f32 = 180.0;
    let current = if config.general.font.is_empty() {
        i18n.t("font-default")
    } else {
        config.general.font.clone()
    };
    ui.allocate_ui(egui::vec2(ROW_W, 0.0), |ui| {
        let resp = egui::ComboBox::from_id_salt("settings-font-select")
            .width(ROW_W)
            // 折叠态长字体名省略(弹窗宽 = 按钮宽)
            .truncate()
            // 默认 CloseOnClick 点弹窗内任意处(含搜索框)都会关弹窗
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            // 弹窗内容总高上限:搜索框 + 240 高列表,略留余量
            .height(300.0)
            .selected_text(current.clone())
            .show_ui(ui, |ui| {
                // ComboBox 弹窗默认 Extend 不省略(combo_box.rs:411),
                // 长字体会顶出弹窗宽;改回 Truncate 省略
                ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
                // 搜索框:输入即过滤(大小写不敏感包含)
                let hint = i18n.t("font-search");
                ui.add(
                    egui::TextEdit::singleline(filter)
                        .hint_text(hint)
                        .desired_width(ui.available_width())
                        .margin(egui::Margin::symmetric(4, 2)),
                );
                ui.add_space(theme::sp::XS);
                // 列表:定高滚动,避免长列表把弹窗顶出屏幕。
                // auto_shrink 必须关:滚动区高度只取 min(可用高, 240),
                // 不随过滤结果长短变化。否则过滤出少量字体时弹窗先变矮,
                // 清空搜索词后可用高被压扁,列表看着像"数据集缩小"且恢复
                // 不了——egui Area 的 size 取自内容 min_size(area.rs),
                // 压扁后是稳定不动点,弹窗再也不长回来
                let needle = filter.trim().to_lowercase();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(240.0)
                    .show(ui, |ui| {
                        let default_name = i18n.t("font-default");
                        let mut items: Vec<(&str, bool)> =
                            vec![(&default_name, config.general.font.is_empty())];
                        items.extend(
                            fonts
                                .iter()
                                .filter(|e| {
                                    needle.is_empty() || e.name.to_lowercase().contains(&needle)
                                })
                                .map(|e| (e.name.as_str(), e.name == config.general.font)),
                        );
                        for (name, selected) in items {
                            let label =
                                RichText::new(name)
                                    .size(theme::font::BODY)
                                    .color(if selected {
                                        theme::c().accent
                                    } else {
                                        theme::c().text
                                    });
                            let r = ui.selectable_label(selected, label);
                            if r.clicked() {
                                let picked = if name == default_name {
                                    String::new()
                                } else {
                                    name.to_owned()
                                };
                                if config.general.font != picked {
                                    config.general.font = picked;
                                    *changed = true;
                                }
                            }
                            // 长字体名省略后悬停展示全名
                            r.on_hover_text(name);
                        }
                    });
            });
        // 点按钮开合弹窗即清空搜索词:开弹窗那一帧 popup 走 sizing
        // pass 不显示,清空对下一帧(首帧可见)生效,不会闪;合上时清掉
        // 残留,下次打开从完整列表起步
        if resp.response.clicked() {
            filter.clear();
        }
        // 折叠态悬停气泡:选中字体名被省略时展示全名
        resp.response.on_hover_text(current);
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
