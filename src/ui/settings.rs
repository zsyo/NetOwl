//! 设置页:语言/主题/数据源/历史保留期/询问/托盘/日志。

use eframe::egui;
use egui::RichText;

use super::{log_window, theme};
use crate::collector::CollectorKind;
use crate::i18n::I18n;
use crate::logging::LogLevel;
use crate::storage::config::Config;

/// 设置页:语言切换(词条即时生效)、主题、数据源;返回是否直接改动了配置。
/// 日志区:级别下拉与文件开关立即生效(直接调 logging),窗口入口只置位状态
pub(super) fn settings_ui(
    ui: &mut egui::Ui,
    config: &mut Config,
    i18n: &mut I18n,
    log_window: &mut log_window::PageState,
) -> bool {
    ui.heading(theme::accent_text(&i18n.t("settings-title"), 20.0));
    ui.add_space(16.0);
    ui.label(
        RichText::new(i18n.t("settings-language"))
            .size(14.0)
            .strong()
            .color(theme::c().text),
    );
    ui.add_space(4.0);

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
                let label = RichText::new(name).size(14.0).color(if selected {
                    theme::c().accent
                } else {
                    theme::c().text
                });
                if ui.selectable_label(selected, label).clicked() {
                    i18n.set_language(code);
                }
            }
        });
    ui.add_space(6.0);
    ui.label(theme::dim_text(&i18n.t("settings-language-hint"), 12.0));
    ui.add_space(16.0);

    // 界面主题:切换立即生效(调色板与 Visuals 同步刷新)
    ui.label(
        RichText::new(i18n.t("settings-theme"))
            .size(14.0)
            .strong()
            .color(theme::c().text),
    );
    ui.add_space(4.0);
    let theme_name = if theme::is_dark() {
        i18n.t("theme-dark")
    } else {
        i18n.t("theme-light")
    };
    egui::ComboBox::from_id_salt("settings-theme-select")
        .width(180.0)
        .selected_text(theme_name)
        .show_ui(ui, |ui| {
            for (dark, name) in [(true, i18n.t("theme-dark")), (false, i18n.t("theme-light"))] {
                let selected = theme::is_dark() == dark;
                let label = RichText::new(name).size(14.0).color(if selected {
                    theme::c().accent
                } else {
                    theme::c().text
                });
                if ui.selectable_label(selected, label).clicked() {
                    theme::set_theme(dark, ui.ctx());
                }
            }
        });
    ui.add_space(6.0);
    ui.label(theme::dim_text(&i18n.t("settings-theme-hint"), 12.0));
    ui.add_space(16.0);

    // 数据源:真实采集 / 模拟演示,切换后由 logic 检测配置变化并重建采集器
    ui.label(
        RichText::new(i18n.t("settings-datasource"))
            .size(14.0)
            .strong()
            .color(theme::c().text),
    );
    ui.add_space(4.0);
    let current = CollectorKind::from_config(&config.general.collector);
    let datasource_name = |i18n: &I18n, kind: CollectorKind| match kind {
        CollectorKind::Real => i18n.t("datasource-real"),
        CollectorKind::Mock => i18n.t("datasource-mock"),
    };
    egui::ComboBox::from_id_salt("settings-datasource-select")
        .width(180.0)
        .selected_text(datasource_name(i18n, current))
        .show_ui(ui, |ui| {
            for kind in [CollectorKind::Real, CollectorKind::Mock] {
                let selected = current == kind;
                let label =
                    RichText::new(datasource_name(i18n, kind))
                        .size(14.0)
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
    ui.add_space(6.0);
    ui.label(theme::dim_text(&i18n.t("settings-datasource-hint"), 12.0));
    ui.add_space(16.0);

    // 历史数据自动清理天数(0 = 不自动清理)
    ui.label(
        RichText::new(i18n.t("settings-history-days"))
            .size(14.0)
            .strong()
            .color(theme::c().text),
    );
    ui.add_space(4.0);
    let days = ui.add(
        egui::DragValue::new(&mut config.general.history_days)
            .range(0..=365)
            .suffix(format!(" {}", i18n.t("settings-history-days-unit"))),
    );
    ui.add_space(6.0);
    ui.label(theme::dim_text(&i18n.t("settings-history-days-hint"), 12.0));
    ui.add_space(16.0);

    // 新连接询问弹窗(默认关闭;开启后未命中规则的公网新连接弹窗询问)
    ui.label(
        RichText::new(i18n.t("settings-ask"))
            .size(14.0)
            .strong()
            .color(theme::c().text),
    );
    ui.add_space(4.0);
    let ask = ui.checkbox(
        &mut config.general.ask_connections,
        i18n.t("settings-ask-on"),
    );
    ui.add_space(6.0);
    ui.label(theme::dim_text(&i18n.t("settings-ask-hint"), 12.0));
    ui.add_space(16.0);

    // 托盘图标常驻(注册表 IsPromoted,写入失败静默保持系统默认;新会话生效)
    ui.label(
        RichText::new(i18n.t("settings-tray-pin"))
            .size(14.0)
            .strong()
            .color(theme::c().text),
    );
    ui.add_space(4.0);
    let tray = ui.checkbox(
        &mut config.general.tray_pinned,
        i18n.t("settings-tray-pin-on"),
    );
    ui.add_space(6.0);
    ui.label(theme::dim_text(&i18n.t("settings-tray-pin-hint"), 12.0));
    ui.add_space(16.0);

    // 运行日志:级别下拉立即 reload;文件开关切换建/停写线程;查看入口
    // 打开日志浏览窗口(内存层收集,诊断用;参照 wallwarp)
    ui.label(
        RichText::new(i18n.t("settings-log"))
            .size(14.0)
            .strong()
            .color(theme::c().text),
    );
    ui.add_space(4.0);
    let mut changed_log = false;
    ui.horizontal(|ui| {
        ui.label(theme::dim_text(&i18n.t("settings-log-level"), 13.0));
        let current = LogLevel::parse(&config.general.log_level);
        egui::ComboBox::from_id_salt("settings-log-level")
            .width(110.0)
            .selected_text(i18n.t(log_window::level_key(current)))
            .show_ui(ui, |ui| {
                for level in LogLevel::ALL {
                    if ui
                        .selectable_label(
                            current == level,
                            RichText::new(i18n.t(log_window::level_key(level))).size(13.0),
                        )
                        .clicked()
                    {
                        config.general.log_level = level.as_str().to_owned();
                        crate::logging::set_level(level);
                    }
                }
            });
        ui.add_space(12.0);
        if ui
            .checkbox(
                &mut config.general.log_to_file,
                i18n.t("settings-log-file-on"),
            )
            .changed()
        {
            crate::logging::set_file_enabled(config.general.log_to_file);
            changed_log = true;
        }
        ui.add_space(12.0);
        if ui.button(i18n.t("settings-log-view")).clicked() {
            log_window::open(log_window);
        }
    });
    ui.add_space(6.0);
    ui.label(theme::dim_text(&i18n.t("settings-log-hint"), 12.0));
    days.changed() || ask.changed() || tray.changed() || changed_log
}
