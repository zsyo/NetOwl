//! 日志浏览窗口:独立 viewport 展示内存层日志(参照 wallwarp 同名功能)。
//! 展示档位独立于控制台与文件层;窗口关闭即停止收集并清空本地缓冲。
//! 工具栏:展示级别下拉、自动滚动、关键字过滤、清空;日志区按等级着色。

use eframe::egui;
use egui::RichText;
use tracing::Level;

use crate::i18n::I18n;
use crate::logging::{self, LogLevel};
use crate::ui::{TOOLBAR_ROW_H, theme};

/// 窗口本地缓冲上限(消息长行换行渲染,非虚拟化,过大影响帧耗时)
const MAX_LINES: usize = 1000;

/// 窗口状态(会话内,不持久化;关闭即清空本地缓冲)
pub struct PageState {
    pub open: bool,
    /// viewport 是否已创建(首帧预建;托盘"打开实时日志"在全隐身
    /// 状态下据此先恢复主窗口驱动首帧,见 app 层 CMD_LOG_OPEN)
    created: bool,
    /// 上次下发给窗口的可见性(builder 仅在创建窗口时生效,
    /// 之后靠 ViewportCommand::Visible 切换)
    applied_open: bool,
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
            created: false,
            applied_open: false,
            cursor: 0,
            lines: Vec::new(),
            shown_level: LogLevel::Info,
            filter: String::new(),
            auto_scroll: true,
        }
    }
}

/// 窗口 viewport id(首帧预建常驻,运行中不再新建)
fn log_viewport_id() -> egui::ViewportId {
    egui::ViewportId(egui::Id::new("netowl-log-window"))
}

/// viewport 是否已创建:全隐身状态(主窗口隐藏且悬浮窗关闭)下首帧
/// 只跑 logic 不跑 ui,viewport 建不出来;托盘"打开实时日志"需据此
/// 先恢复主窗口
pub fn is_created(state: &PageState) -> bool {
    state.created
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

/// 每帧渲染(主窗口 ui() 末尾调用)。viewport 首帧预建常驻,运行中
/// 不再新建窗口:eframe 0.36 的定时唤醒帧(new_events)未设置
/// EventLoopGuard,该上下文里新建 immediate viewport 窗口会被静默
/// 跳过,egui 断言回调未执行直接 panic——与悬浮窗同模式,窗口只在
/// 首帧创建,显示与否靠 ViewportCommand::Visible 切换
pub fn show(ctx: &egui::Context, state: &mut PageState, i18n: &I18n) {
    state.created = true;
    if state.open {
        let fresh = logging::window_layer::read_since(state.cursor);
        if let Some(last) = fresh.last() {
            state.cursor = last.seq;
        }
        state.lines.extend(fresh);
        if state.lines.len() > MAX_LINES {
            let overflow = state.lines.len() - MAX_LINES;
            state.lines.drain(..overflow);
        }
    }
    if state.applied_open != state.open {
        ctx.send_viewport_cmd_to(
            log_viewport_id(),
            egui::ViewportCommand::Visible(state.open),
        );
        state.applied_open = state.open;
    }

    let builder = egui::ViewportBuilder::default()
        .with_title(i18n.t("settings-log"))
        .with_inner_size([780.0, 520.0])
        .with_visible(state.open);
    ctx.show_viewport_immediate(log_viewport_id(), builder, |ui, _class| {
        // 预建态(未打开)只维持窗口存在,不渲染内容
        if !state.open {
            return;
        }
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
    });
}

/// 顶栏:展示级别下拉、自动滚动、过滤输入、清空按钮
fn toolbar(ui: &mut egui::Ui, state: &mut PageState, i18n: &I18n) {
    ui.horizontal(|ui| {
        // 行高抬升只作用于本工具栏行(style_mut 泄漏到整页会改变
        // 后续所有布局的最小交互高度)
        ui.style_mut().spacing.interact_size.y = TOOLBAR_ROW_H;
        ui.label(theme::dim_text(
            &i18n.t("log-window-level"),
            theme::font::BODY,
        ));
        let before = state.shown_level;
        egui::ComboBox::from_id_salt("log-window-level")
            .width(110.0)
            .selected_text(i18n.t(level_key(state.shown_level)))
            .show_ui(ui, |ui| {
                for level in LogLevel::ALL {
                    if ui
                        .selectable_label(
                            state.shown_level == level,
                            RichText::new(i18n.t(level_key(level))).size(theme::font::BODY),
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
        ui.label(theme::dim_text(
            &i18n.t("log-window-filter"),
            theme::font::BODY,
        ));
        crate::ui::widgets::search_box::search_box(
            ui,
            &mut state.filter,
            i18n.t("log-window-filter-placeholder"),
            ui.available_width() - 90.0,
        );
        if clear.clicked() {
            state.lines.clear();
            logging::window_layer::clear();
        }
    });
}

/// 日志滚动区:关键字过滤(大小写不敏感)+ 等级着色;消息体自动换行
/// (Label 默认 wrap,长行不再截断),时间与级别列等宽字体固定宽度
fn log_list(ui: &mut egui::Ui, state: &mut PageState, i18n: &I18n) {
    let filter = state.filter.trim().to_lowercase();
    let rows: Vec<&logging::LogLine> = state
        .lines
        .iter()
        .filter(|line| filter.is_empty() || line.message.to_lowercase().contains(&filter))
        .collect();

    if rows.is_empty() {
        let key = if filter.is_empty() {
            "log-window-empty"
        } else {
            "log-window-no-match"
        };
        ui.centered_and_justified(|ui| {
            ui.label(theme::dim_text(&i18n.t(key), theme::font::H3));
        });
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .stick_to_bottom(state.auto_scroll)
        .show(ui, |ui| {
            for line in rows {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
                    ui.painter()
                        .circle_filled(rect.center(), 3.0, level_color(line.level));
                    ui.label(
                        RichText::new(&line.time)
                            .monospace()
                            .size(theme::font::XS)
                            .color(theme::c().text_dim),
                    );
                    ui.label(
                        RichText::new(format!("{:5}", line.level))
                            .monospace()
                            .size(theme::font::XS)
                            .color(level_color(line.level)),
                    );
                    // 消息体显式 Wrap:horizontal 布局的默认 wrap 模式是
                    // Extend(egui 0.36 Ui::wrap_mode 按布局方向判定),
                    // 长行会被截断到窗口外
                    ui.add(
                        egui::Label::new(
                            RichText::new(&line.message)
                                .size(theme::font::SM)
                                .color(theme::c().text),
                        )
                        .wrap_mode(egui::TextWrapMode::Wrap),
                    );
                });
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
