//! 日志浏览窗口:独立 viewport 展示内存层日志(参照 wallwarp 同名功能)。
//! 展示档位独立于控制台与文件层;窗口关闭即停止收集并清空本地缓冲。
//! 工具栏:展示级别下拉、自动滚动、关键字过滤、清空;日志区按等级着色。

use eframe::egui;
use egui::RichText;
use tracing::Level;

use crate::i18n::I18n;
use crate::logging::{self, LogLevel};
use crate::ui::theme;

/// 窗口本地缓冲上限(与内存层环形缓冲一致)
const MAX_LINES: usize = 5000;
/// 单行渲染高度(show_rows 虚拟化要求行高一致)
const ROW_HEIGHT: f32 = 19.0;

/// 窗口状态(会话内,不持久化;关闭即清空本地缓冲)
pub struct PageState {
    pub open: bool,
    /// 内存层增量拉取游标(已读到的最后一条 seq)
    cursor: u64,
    lines: Vec<logging::LogLine>,
    /// 展示档位(仅控制本窗口收集,与控制台/文件层互不影响)
    shown_level: LogLevel,
    filter: String,
    auto_scroll: bool,
}

impl PageState {
    pub fn new() -> Self {
        PageState {
            open: false,
            cursor: 0,
            lines: Vec::new(),
            shown_level: LogLevel::Info,
            filter: String::new(),
            auto_scroll: true,
        }
    }
}

/// 级别词条键(设置页与日志窗口共用)
pub fn level_key(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Off => "log-level-off",
        LogLevel::Error => "log-level-error",
        LogLevel::Warn => "log-level-warn",
        LogLevel::Info => "log-level-info",
        LogLevel::Debug => "log-level-debug",
        LogLevel::Trace => "log-level-trace",
    }
}

/// 打开窗口(设置页入口):置位并开启内存层收集
pub fn open(state: &mut PageState) {
    state.open = true;
    logging::window_layer::set_shown_level(Some(state.shown_level));
}

fn close(state: &mut PageState) {
    state.open = false;
    state.lines.clear();
    state.cursor = 0;
    logging::window_layer::set_shown_level(None);
}

/// 每帧渲染(主窗口 ui() 末尾调用;未打开时不创建视口)。
/// immediate 视口在本帧同步执行,闭包内直接借用状态
pub fn show(ctx: &egui::Context, state: &mut PageState, i18n: &I18n) {
    if !state.open {
        return;
    }
    let fresh = logging::window_layer::read_since(state.cursor);
    if let Some(last) = fresh.last() {
        state.cursor = last.seq;
    }
    state.lines.extend(fresh);
    if state.lines.len() > MAX_LINES {
        let overflow = state.lines.len() - MAX_LINES;
        state.lines.drain(..overflow);
    }

    let builder = egui::ViewportBuilder::default()
        .with_title(i18n.t("settings-log"))
        .with_inner_size([780.0, 520.0]);
    ctx.show_viewport_immediate(
        egui::ViewportId(egui::Id::new("netowl-log-window")),
        builder,
        |ui, _class| {
            // 标题栏关闭按钮:停止收集并清空本地缓冲
            if ui.input(|i| i.viewport().close_requested()) {
                close(state);
                return;
            }
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(theme::c().bg_base)
                        .inner_margin(egui::Margin::same(10)),
                )
                .show(ui, |ui| {
                    toolbar(ui, state, i18n);
                    ui.add_space(8.0);
                    log_list(ui, state, i18n);
                });
        },
    );
}

/// 顶栏:展示级别下拉、自动滚动、过滤输入、清空按钮
fn toolbar(ui: &mut egui::Ui, state: &mut PageState, i18n: &I18n) {
    // 历史页工具栏同款修正:行高从 interact_size.y 起步会导致混排错位
    ui.style_mut().spacing.interact_size.y = 26.0;
    ui.horizontal(|ui| {
        ui.label(theme::dim_text(&i18n.t("log-window-level"), 13.0));
        let before = state.shown_level;
        egui::ComboBox::from_id_salt("log-window-level")
            .width(110.0)
            .selected_text(i18n.t(level_key(state.shown_level)))
            .show_ui(ui, |ui| {
                for level in LogLevel::ALL {
                    if ui
                        .selectable_label(
                            state.shown_level == level,
                            RichText::new(i18n.t(level_key(level))).size(13.0),
                        )
                        .clicked()
                    {
                        state.shown_level = level;
                    }
                }
            });
        if state.shown_level != before {
            logging::window_layer::set_shown_level(Some(state.shown_level));
        }
        ui.add_space(8.0);
        ui.checkbox(&mut state.auto_scroll, i18n.t("log-window-autoscroll"));
        ui.add_space(8.0);
        let clear = ui.button(i18n.t("log-window-clear"));
        ui.add_space(8.0);
        ui.label(theme::dim_text(&i18n.t("log-window-filter"), 13.0));
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .desired_width(ui.available_width() - 90.0)
                .hint_text(i18n.t("log-window-filter-placeholder")),
        );
        if clear.clicked() {
            state.lines.clear();
            logging::window_layer::clear();
        }
    });
}

/// 日志滚动区:关键字过滤(大小写不敏感)+ 等级着色;show_rows 虚拟化,
/// 仅渲染可见行(本地缓冲上限 5000 行)
fn log_list(ui: &mut egui::Ui, state: &mut PageState, i18n: &I18n) {
    let filter = state.filter.trim().to_lowercase();
    let rows: Vec<usize> = state
        .lines
        .iter()
        .enumerate()
        .filter(|(_, line)| filter.is_empty() || line.message.to_lowercase().contains(&filter))
        .map(|(i, _)| i)
        .collect();

    if rows.is_empty() {
        let key = if filter.is_empty() {
            "log-window-empty"
        } else {
            "log-window-no-match"
        };
        ui.centered_and_justified(|ui| {
            ui.label(theme::dim_text(&i18n.t(key), 14.0));
        });
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .stick_to_bottom(state.auto_scroll)
        .show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
            for i in range {
                let line = &state.lines[i];
                let (y, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), ROW_HEIGHT),
                    egui::Sense::hover(),
                );
                let painter = ui.painter_at(y);
                let mut x = y.left() + 4.0;
                painter.circle_filled(
                    egui::Pos2::new(x + 4.0, y.center().y),
                    3.0,
                    level_color(line.level),
                );
                x += 14.0;
                painter.text(
                    egui::Pos2::new(x, y.center().y),
                    egui::Align2::LEFT_CENTER,
                    &line.time,
                    egui::FontId::monospace(11.0),
                    theme::c().text_dim,
                );
                x += 128.0;
                painter.text(
                    egui::Pos2::new(x, y.center().y),
                    egui::Align2::LEFT_CENTER,
                    format!("{:5}", line.level),
                    egui::FontId::monospace(11.0),
                    level_color(line.level),
                );
                x += 52.0;
                painter.text(
                    egui::Pos2::new(x, y.center().y),
                    egui::Align2::LEFT_CENTER,
                    &line.message,
                    egui::FontId::proportional(12.0),
                    theme::c().text,
                );
            }
        });
}

/// 等级语义色:ERROR 红 / WARN 黄 / INFO 强调色 / DEBUG·TRACE 弱化
fn level_color(level: Level) -> egui::Color32 {
    match level {
        Level::ERROR => theme::c().danger,
        Level::WARN => theme::c().status_warn,
        Level::INFO => theme::c().accent,
        Level::DEBUG | Level::TRACE => theme::c().text_dim,
    }
}
