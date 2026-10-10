//! 设置页监控分组:数据源、历史保留期、新连接询问、静默模式三态、
//! 托盘常驻与开机自启动(注册表项由 App 层失败重试写入)。

use eframe::egui;
use egui::RichText;

use super::{section_card, setting_row};
use crate::collector::CollectorKind;
use crate::i18n::I18n;
use crate::storage::config::Config;
use crate::ui::{icons, theme, widgets};

pub(super) fn section(ui: &mut egui::Ui, config: &mut Config, i18n: &I18n, changed: &mut bool) {
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
                &i18n.t("settings-lan-notify"),
                &i18n.t("settings-lan-notify-hint"),
                |ui| {
                    *changed |= widgets::toggle::toggle_switch(
                        ui,
                        &mut config.general.lan_notify,
                        egui::Id::new("settings-lan-notify-toggle"),
                    )
                    .changed();
                },
            );
            ui.add_space(theme::sp::SM);
            setting_row(
                ui,
                &i18n.t("settings-listen-notify"),
                &i18n.t("settings-listen-notify-hint"),
                |ui| {
                    *changed |= widgets::toggle::toggle_switch(
                        ui,
                        &mut config.general.listen_notify,
                        egui::Id::new("settings-listen-notify-toggle"),
                    )
                    .changed();
                },
            );
            ui.add_space(theme::sp::SM);
            setting_row(
                ui,
                &i18n.t("settings-quota"),
                &i18n.t("settings-quota-hint"),
                |ui| {
                    // 0 = 不启用;达到 80%/100% 阈值时右下角告警
                    let before = config.general.usage_quota_gb;
                    *changed |= ui
                        .add(
                            egui::DragValue::new(&mut config.general.usage_quota_gb)
                                .range(0..=1024)
                                .suffix(format!(" {}", i18n.t("settings-quota-unit"))),
                        )
                        .changed();
                    if config.general.usage_quota_gb != before {
                        tracing::debug!(
                            "[Settings] 月度配额 -> {} GB",
                            config.general.usage_quota_gb
                        );
                    }
                },
            );
            ui.add_space(theme::sp::SM);
            setting_row(
                ui,
                &i18n.t("settings-history-days"),
                &i18n.t("settings-history-days-hint"),
                |ui| {
                    // 0 = 不自动清理
                    let before = config.general.history_days;
                    *changed |= ui
                        .add(
                            egui::DragValue::new(&mut config.general.history_days)
                                .range(0..=365)
                                .suffix(format!(" {}", i18n.t("settings-history-days-unit"))),
                        )
                        .changed();
                    if config.general.history_days != before {
                        tracing::debug!(
                            "[Settings] 历史保留期 -> {} 天",
                            config.general.history_days
                        );
                    }
                },
            );
            ui.add_space(theme::sp::SM);
            setting_row(
                ui,
                &i18n.t("settings-ask"),
                &i18n.t("settings-ask-hint"),
                |ui| {
                    // 开启后未命中规则的公网新连接弹窗询问;静默模式下不生效
                    *changed |= widgets::toggle::toggle_switch(
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
                &i18n.t("settings-silent"),
                &i18n.t("settings-silent-hint"),
                |ui| {
                    // 静默模式三态:off 按"新连接询问"开关行为,allow/deny
                    // 静默放行/拒绝;写 config 后由 App 层统一联动兜底规则、
                    // WFP 过滤器与托盘子菜单勾选态
                    let items = [
                        (&*i18n.t("settings-silent-off"), icons::X_CIRCLE),
                        (&*i18n.t("settings-silent-allow"), icons::CHECK_CIRCLE),
                        (&*i18n.t("settings-silent-deny"), icons::BAN),
                    ];
                    let current = match config.general.silent_mode.as_str() {
                        "allow" => 1,
                        "deny" => 2,
                        _ => 0,
                    };
                    if let Some(i) = widgets::segmented::segmented(ui, &items, current) {
                        config.general.silent_mode = ["off", "allow", "deny"][i].to_owned();
                        *changed = true;
                    }
                },
            );
        },
    );
}
