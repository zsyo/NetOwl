//! 设置页:分组卡片布局(外观/监控/日志/关于),行式设置项(左标签 +
//! 右控件)。四个分组各自成文件(appearance/monitor/logging/about)。

mod about;
mod appearance;
mod logging;
mod monitor;

use eframe::egui;
use egui::{Align, Layout, RichText};

use super::{UiCtx, log_window, theme, widgets};
use crate::i18n::I18n;
use crate::platform::update::ReleaseInfo;
use crate::storage::config::Config;

/// 设置页:语言切换(词条即时生效)、主题、数据源;返回是否直接改动了配置。
/// 日志区:级别下拉与文件开关立即生效(直接调 logging),窗口入口只置位状态
pub(super) fn settings_ui(ui: &mut egui::Ui, ctx: &mut UiCtx) -> bool {
    let UiCtx {
        config,
        i18n,
        log_window,
        update_check_request,
        update_checking,
        update_result,
        ..
    } = ctx;
    // &mut UiCtx 解构出的引用字段带两层 &mut,借类型注解 coerce 回单层
    let config: &mut Config = config;
    let i18n: &mut I18n = i18n;
    let log_window: &mut log_window::PageState = log_window;
    let update_check_request: &mut bool = update_check_request;
    let update_result: &mut Option<Result<Option<ReleaseInfo>, String>> = update_result;
    let update_checking = *update_checking;
    widgets::header::page_header(ui, &i18n.t("settings-title"), "");
    ui.add_space(theme::sp::MD);

    let mut changed = false;
    // 页头固定,卡片内容随窗口高度滚动(设置项多,窗口较小时
    // 需要滚动到下方分组)
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            // ---- 外观:语言 / 主题 ----
            appearance::section(ui, config, i18n, &mut changed);
            ui.add_space(theme::sp::MD);

            // ---- 监控:数据源 / 历史保留期 / 新连接询问 / 托盘常驻 ----
            monitor::section(ui, config, i18n, &mut changed);
            ui.add_space(theme::sp::MD);

            // ---- 日志:文件开关 / 级别 / 查看入口,每行语义单一 ----
            logging::section(ui, config, i18n, log_window, &mut changed);
            ui.add_space(theme::sp::MD);

            // ---- 关于:当前版本 / 更新渠道 / 检查更新 ----
            about::section(
                ui,
                config,
                i18n,
                update_check_request,
                update_checking,
                update_result,
                &mut changed,
            );

            changed
        })
        .inner
}

/// 设置分组卡片:标题行(图标 + 标题)+ 内容;
/// 卡片宽度随内容行自然撑满中央区可用宽(行内左右两端布局)
pub(super) fn section_card<R>(
    ui: &mut egui::Ui,
    icon: &str,
    title: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) {
    widgets::card::card(ui, |ui| {
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
pub(super) fn setting_row<R>(
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

/// 资源管理器定位日志:latest.log 存在则选中,否则打开日志目录。
/// 工作目录在启动时已切到数据根,拼 LOGS_DIR 即为目标目录
fn locate_log_file() {
    use crate::platform::paths::{open_in_explorer, select_in_explorer};

    let dir = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(crate::platform::paths::LOGS_DIR);
    let latest = dir.join("latest.log");
    if latest.exists() {
        select_in_explorer(&latest);
    } else {
        let _ = std::fs::create_dir_all(&dir);
        open_in_explorer(&dir);
    }
}
