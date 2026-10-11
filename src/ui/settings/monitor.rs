//! 设置页监控分组:数据源、网络质量采样(含自定义目标管理)、历史保留期、
//! 新连接询问、静默模式三态、托盘常驻与开机自启动(注册表项由 App 层失败
//! 重试写入)。

use eframe::egui;
use egui::RichText;

use super::{section_card, setting_row};
use crate::collector::CollectorKind;
use crate::i18n::I18n;
use crate::net::quality::SAMPLE_TTL;
use crate::storage::config::{Config, QualityTarget};
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
                &i18n.t("settings-quality"),
                &i18n.t("settings-quality-hint"),
                |ui| {
                    *changed |= widgets::toggle::toggle_switch(
                        ui,
                        &mut config.general.quality_monitor,
                        egui::Id::new("settings-quality-toggle"),
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

/// 自定义质量采样目标列表:逐行 IP/端口/备注即时编辑,行尾删除;
/// IP 手输未完成时即时校验提示,解析失败的目标由采样侧跳过
pub(super) fn targets_section(
    ui: &mut egui::Ui,
    config: &mut Config,
    i18n: &I18n,
    quality: Option<&crate::net::quality::Quality>,
    changed: &mut bool,
) {
    section_card(
        ui,
        icons::GLOBE,
        &i18n.t("settings-quality-targets"),
        |ui| {
            // add_sized 占满可用宽:卡片内无 setting_row 类占满控件,不撑则
            // 卡片按内容收缩,与其它分组宽度不一致(Label::extend 仅表示
            // "不换行不截断、扩展父容器",不会占满)
            ui.add_sized(
                [ui.available_width(), 16.0],
                egui::Label::new(theme::dim_text(
                    &i18n.t("settings-quality-targets-hint"),
                    theme::font::XS,
                ))
                .wrap_mode(egui::TextWrapMode::Truncate),
            );
            let mut removed: Option<usize> = None;
            for (i, t) in config.general.quality_targets.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    *changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut t.ip)
                                .desired_width(150.0)
                                .hint_text("1.2.3.4"),
                        )
                        .changed();
                    *changed |= ui
                        .add(egui::DragValue::new(&mut t.port).range(0..=65535))
                        .changed();
                    *changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut t.label)
                                .desired_width(120.0)
                                .hint_text(&i18n.t("settings-quality-label-hint")),
                        )
                        .changed();
                    if widgets::button::icon_btn(
                        ui,
                        icons::TRASH,
                        Some(i18n.t("settings-quality-remove")),
                        true,
                        true,
                    )
                    .clicked()
                    {
                        removed = Some(i);
                    }
                    let ip = t.ip.trim();
                    if !ip.is_empty() && ip.parse::<std::net::Ipv4Addr>().is_err() {
                        ui.label(
                            RichText::new(i18n.t("settings-quality-invalid-ip"))
                                .size(theme::font::XS)
                                .color(theme::c().danger),
                        );
                    }
                    // 监测状态(行尾):RTT 为 EMA 平滑、丢包取最近窗口;
                    // "无响应" = 窗口内全失败(常见于禁 ping);"待采样" =
                    // 尚未完成首轮或样本已过期(采样开关关闭后停止刷新)
                    let sample = ip
                        .parse::<std::net::Ipv4Addr>()
                        .ok()
                        .and_then(|ip| quality.and_then(|q| q.lookup(ip)))
                        .filter(|s| s.at.elapsed() < SAMPLE_TTL * 2);
                    match sample {
                        Some(s) if s.rtt.is_some() => {
                            let text = i18n.t_with_args(
                                "settings-quality-sample",
                                &[
                                    ("rtt", s.rtt.unwrap_or(0).to_string()),
                                    ("loss", s.loss.to_string()),
                                ],
                            );
                            let color = if s.loss > 10 {
                                theme::c().status_warn
                            } else {
                                theme::c().text_dim
                            };
                            ui.label(RichText::new(text).size(theme::font::XS).color(color));
                        }
                        Some(_) => {
                            ui.label(
                                RichText::new(i18n.t("settings-quality-noreply"))
                                    .size(theme::font::XS)
                                    .color(theme::c().danger),
                            );
                        }
                        None if !ip.is_empty() => {
                            ui.label(
                                RichText::new(i18n.t("settings-quality-wait"))
                                    .size(theme::font::XS)
                                    .color(theme::c().text_dim),
                            );
                        }
                        None => {}
                    }
                });
            }
            if let Some(i) = removed {
                config.general.quality_targets.remove(i);
                *changed = true;
            }
            if ui
                .add(egui::Button::new(RichText::new(format!(
                    "{} {}",
                    icons::PLUS,
                    i18n.t("settings-quality-add")
                ))))
                .clicked()
            {
                config
                    .general
                    .quality_targets
                    .push(QualityTarget::default());
                *changed = true;
            }
        },
    );
}
