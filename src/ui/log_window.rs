//! 日志浏览窗口:独立 viewport 展示内存层日志(参照 wallwarp 同名功能)。
//! 展示档位独立于控制台与文件层;窗口关闭即停止收集并清空本地缓冲。
//! 工具栏:展示级别下拉、自动滚动、关键字过滤、清空,置顶靠右端;日志区按等级着色。

use eframe::egui;
use egui::RichText;
use egui::viewport::WindowLevel;
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
    /// 置顶:本窗 always-on-top(会话态,不持久化;经 builder 的
    /// window_level 差量下发,切换即生效且不重建窗口)
    pin_on_top: bool,
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
            pin_on_top: false,
        }
    }
}

/// 窗口 viewport id(首帧预建常驻,运行中不再新建)
fn log_viewport_id() -> egui::ViewportId {
    egui::ViewportId(egui::Id::unique("netowl-log-window"))
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
/// 首帧创建,显示与否靠 ViewportCommand::Visible 切换;builder 属性
/// 的差量(如置顶的 window_level)运行中仍会正常下发
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
        .with_window_level(if state.pin_on_top {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        })
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

/// 工具栏标签:占位 + painter 直绘,文字垂直居中于工具栏行。直接
/// ui.label 时其分配 rect 比同行交互控件(interact_size 26)高约
/// 4px 且文字贴在分配区顶部,与控件文字错位——水平行 placer 对
/// 非交互矮件与交互控件的 frame 填充口径不一致(实测标签 rect
/// 中心 19 vs 控件 23,行中心 23),按表单标签既定定案直绘对齐
fn toolbar_label(ui: &mut egui::Ui, text: &str) {
    let w = crate::ui::text_width(ui, text, theme::font::BODY);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, TOOLBAR_ROW_H), egui::Sense::hover());
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(theme::font::BODY),
        theme::c().text_dim,
    );
}

/// 置顶按钮整宽(文字 + 按钮左右内边距),供顶栏右端预留:搜索框
/// 只填到预留位之前,按钮紧随其后即落在行尾,与同行控件顶对齐
/// (interact_size.y 已统一行高)。文案随语言变,取实测而非定值
fn pin_button_width(ui: &egui::Ui, text: &str) -> f32 {
    crate::ui::text_width(ui, text, theme::font::BODY) + 2.0 * ui.spacing().button_padding.x
}

/// 顶栏:展示级别下拉、自动滚动、过滤输入、清空按钮;置顶靠右端
fn toolbar(ui: &mut egui::Ui, state: &mut PageState, i18n: &I18n) {
    let pin_text = i18n.t("log-window-pin");
    ui.horizontal(|ui| {
        // 行高抬升只作用于本工具栏行(style_mut 泄漏到整页会改变
        // 后续所有布局的最小交互高度)
        ui.style_mut().spacing.interact_size.y = TOOLBAR_ROW_H;
        toolbar_label(ui, &i18n.t("log-window-level"));
        let before = state.shown_level;
        egui::ComboBox::from_id_salt("log-window-level")
            .width(110.0)
            .selected_text(
                RichText::new(i18n.t(level_key(state.shown_level))).size(theme::font::BODY),
            )
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
        ui.checkbox(
            &mut state.auto_scroll,
            RichText::new(i18n.t("log-window-autoscroll")).size(theme::font::BODY),
        );
        ui.add_space(8.0);
        let clear = ui.button(RichText::new(i18n.t("log-window-clear")).size(theme::font::BODY));
        ui.add_space(8.0);
        toolbar_label(ui, &i18n.t("log-window-filter"));
        let pin_w = pin_button_width(ui, &pin_text);
        crate::ui::widgets::search_box::search_box(
            ui,
            &mut state.filter,
            i18n.t("log-window-filter-placeholder"),
            ui.available_width() - pin_w - theme::sp::SM,
        );
        if clear.clicked() {
            state.lines.clear();
            logging::window_layer::clear();
        }
        ui.add_space(theme::sp::SM);
        // 置顶:本窗 always-on-top,经 builder 的 window_level 差量下发
        // (仅变更帧产生命令,不重建窗口);会话态,与自动滚动/过滤同口径。
        // 置于行尾:搜索框只填到预留位,本按钮紧随其后落在右端,不挤
        // 左组空间
        if ui
            .add(
                egui::Button::new(RichText::new(pin_text).size(theme::font::BODY))
                    .selected(state.pin_on_top),
            )
            .clicked()
        {
            state.pin_on_top = !state.pin_on_top;
        }
    });
}

/// 大小写不敏感的包含判断(needle 已小写化)。ASCII needle 走字节窗
/// 逐字节比较,零分配:1000 行上限下每行一次 to_lowercase 在事件驱动
/// 重绘时是纯浪费;含非 ASCII 时回退分配版(偶发,可接受)
fn contains_ci(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_ascii() {
        let n = needle_lower.as_bytes();
        // 合法 UTF-8 中 ASCII 字节只属于 ASCII 字符,跨字符边界不会误匹配
        return haystack
            .as_bytes()
            .windows(n.len())
            .any(|w| w.eq_ignore_ascii_case(n));
    }
    haystack.to_lowercase().contains(needle_lower)
}

/// 日志滚动区:关键字过滤(大小写不敏感)+ 等级着色;消息体自动换行
/// (Label 默认 wrap,长行不再截断),时间与级别列等宽字体固定宽度
fn log_list(ui: &mut egui::Ui, state: &mut PageState, i18n: &I18n) {
    let filter = state.filter.trim().to_lowercase();
    let rows: Vec<&logging::LogLine> = state
        .lines
        .iter()
        .filter(|line| filter.is_empty() || contains_ci(&line.message, &filter))
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
