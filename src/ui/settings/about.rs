//! 设置页关于分组:当前版本、更新渠道(正式/预览)与检查更新。

use eframe::egui;
use egui::RichText;

use super::{section_card, setting_row};
use crate::i18n::I18n;
use crate::platform::update::{self, ReleaseInfo};
use crate::storage::config::Config;
use crate::ui::{icons, theme, widgets};

/// 关于分组:版本显示、渠道 segmented、检查按钮与结果
pub(super) fn section(
    ui: &mut egui::Ui,
    config: &mut Config,
    i18n: &I18n,
    check_request: &mut bool,
    checking: bool,
    result: &mut Option<Result<Option<ReleaseInfo>, String>>,
    changed: &mut bool,
) {
    section_card(
        ui,
        icons::INFO_CIRCLE,
        &i18n.t("settings-section-about"),
        |ui| {
            setting_row(
                ui,
                &i18n.t("about-version"),
                &i18n.t("about-version-hint"),
                |ui| {
                    ui.label(
                        RichText::new(format!("v{}", update::CURRENT_VERSION))
                            .size(theme::font::BODY)
                            .font(egui::FontId::monospace(theme::font::BODY))
                            .color(theme::c().text),
                    );
                },
            );
            ui.add_space(theme::sp::SM);
            // 更新渠道(持久化);切换后上次结果在新渠道语境下失效,清空
            setting_row(
                ui,
                &i18n.t("about-channel"),
                &i18n.t("about-channel-hint"),
                |ui| {
                    let preview = config.general.update_channel == "preview";
                    let items = [
                        (&*i18n.t("channel-stable"), ""),
                        (&*i18n.t("channel-preview"), ""),
                    ];
                    if let Some(i) = widgets::segmented::segmented(ui, &items, usize::from(preview))
                    {
                        let next = if i == 1 { "preview" } else { "stable" };
                        if config.general.update_channel != next {
                            config.general.update_channel = next.to_owned();
                            *result = None;
                            *changed = true;
                        }
                    }
                },
            );
            ui.add_space(theme::sp::SM);
            // 检查更新:right_to_left 依次 [检查按钮 | 前往下载(发现新版
            // 时)| 状态文本],在途时按钮禁用防重复请求
            setting_row(
                ui,
                &i18n.t("about-check"),
                &i18n.t("about-check-hint"),
                |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::button::primary_btn(ui, i18n.t("about-check-btn"), !checking)
                            .clicked()
                        {
                            *check_request = true;
                        }
                        if let Some(info) = newer_release(result)
                            && widgets::button::primary_btn(ui, i18n.t("about-download"), true)
                                .clicked()
                        {
                            update::open_download_page(&info.html_url);
                        }
                        status_text(ui, result, checking, i18n);
                    });
                },
            );
        },
    );
}

/// 状态文本:检查中 / 发现新版本(accent)/ 已是最新 / 暂无发布(弱化)/
/// 失败(警示色);尚未检查过不占位
fn status_text(
    ui: &mut egui::Ui,
    result: &Option<Result<Option<ReleaseInfo>, String>>,
    checking: bool,
    i18n: &I18n,
) {
    let (text, color) = match result {
        None => {
            if checking {
                (i18n.t("about-checking"), theme::c().text_dim)
            } else {
                return;
            }
        }
        Some(Ok(Some(info))) => {
            if update::is_newer(&info.tag_name, update::CURRENT_VERSION) {
                (
                    i18n.t_with_args("about-new-version", &[("ver", info.tag_name.clone())]),
                    theme::c().accent,
                )
            } else {
                (i18n.t("about-latest"), theme::c().text_dim)
            }
        }
        Some(Ok(None)) => (i18n.t("about-no-release"), theme::c().text_dim),
        Some(Err(e)) => (
            i18n.t_with_args("about-failed", &[("err", e.clone())]),
            theme::c().danger,
        ),
    };
    // 直接加入右到左布局参与垂直居中——经 allocate_ui/add_sized 的分配块
    // 会顶对齐于行而错位于按钮;超长由子区剩余宽度自动 Truncate,不
    // 侵入左侧标签区
    ui.add(
        egui::Label::new(RichText::new(text).size(theme::font::SM).color(color))
            .wrap_mode(egui::TextWrapMode::Truncate),
    );
}

/// 结果中存在比当前版本新的发布(供"前往下载"按钮显隐)
fn newer_release(result: &Option<Result<Option<ReleaseInfo>, String>>) -> Option<ReleaseInfo> {
    match result {
        Some(Ok(Some(info))) if update::is_newer(&info.tag_name, update::CURRENT_VERSION) => {
            Some(info.clone())
        }
        _ => None,
    }
}
