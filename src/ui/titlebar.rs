//! 自绘无边框标题栏:拖动、双击最大化、最小化/最大化/关闭按钮。
//!
//! 主窗口以 `with_decorations(false)` 创建(见 main.rs),标题栏由本模块
//! 自绘:空白区域按住拖动(StartDrag 走系统移动循环)、双击切换最大化;
//! 关闭按钮与系统关闭事件走同一路径(隐藏到托盘,由 app 层处理)。

use eframe::egui;
use egui::{Align2, FontId, Sense, ViewportCommand};

use super::{icons, theme};

/// 标题栏高度(面板 exact_size 用)
pub const HEIGHT: f32 = 40.0;
/// 窗控按钮宽度
const BUTTON_W: f32 = 44.0;

/// 标题栏按钮触发的窗口动作(app 层处理)
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TitleAction {
    None,
    Minimize,
    ToggleMaximize,
    Close,
}

/// 顶部标题栏(在 `egui::Panel::top` 内调用);返回本轮按钮动作
pub fn show(ui: &mut egui::Ui, app_name: &str) -> TitleAction {
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|i| i.viewport().maximized).unwrap_or(false);
    let mut action = TitleAction::None;

    // 栏底 1px 分隔线,划分标题栏与内容区
    let max = ui.max_rect();
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(max.left(), max.top() + HEIGHT - 1.0),
            egui::pos2(max.right(), max.top() + HEIGHT),
        ),
        0.0,
        theme::c().stroke,
    );

    ui.horizontal(|ui| {
        ui.add_space(theme::sp::LG);
        ui.set_min_height(HEIGHT);
        ui.label(
            egui::RichText::new(app_name)
                .size(theme::font::BODY)
                .strong()
                .color(theme::c().text_dim),
        );

        // 拖动区:占据品牌名与窗控按钮之间的全部空间
        let drag_w = (ui.available_width() - 3.0 * BUTTON_W).max(0.0);
        let (_, drag) = ui.allocate_exact_size(egui::vec2(drag_w, HEIGHT), Sense::click_and_drag());
        if drag.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            action = TitleAction::ToggleMaximize;
        }

        if sys_button(ui, icons::DASH_LG, false) {
            action = TitleAction::Minimize;
        }
        // 还原态用叠框 glyph 区分于最大化
        let maximize_glyph = if maximized {
            icons::COPY
        } else {
            icons::SQUARE
        };
        if sys_button(ui, maximize_glyph, false) {
            action = TitleAction::ToggleMaximize;
        }
        if sys_button(ui, icons::X_LG, true) {
            action = TitleAction::Close;
        }
    });

    action
}

/// 单个窗控按钮(满高方形,悬停显底色;danger 用于关闭钮)
fn sys_button(ui: &mut egui::Ui, glyph: &str, danger: bool) -> bool {
    let p = theme::c();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(BUTTON_W, HEIGHT), Sense::click());
    let hovered = resp.hovered();
    if hovered {
        let bg = if danger { p.danger } else { p.hover_bg };
        ui.painter().rect_filled(rect, 0.0, bg);
    }
    let color = if danger && hovered {
        p.on_accent
    } else {
        p.text_dim
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(theme::font::SM),
        color,
    );
    resp.clicked()
}
