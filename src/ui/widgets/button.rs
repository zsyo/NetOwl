//! 按钮公共件:图标操作钮与主按钮。图标钮 24px 方形、悬停显底色、
//! danger 语义(删除等,悬停警示色)、disabled 置灰;规则表/档管理
//! 弹窗等行内操作钮统一入口。

use eframe::egui;
use egui::{Align2, CornerRadius, FontId, RichText};

use super::super::theme;

/// 主按钮:强调色填充 + 反色文字(页内首要动作);`enabled=false`
/// 置灰禁用(规则页保存/检查更新等在途防重复触发)
pub fn primary_btn(ui: &mut egui::Ui, text: String, enabled: bool) -> egui::Response {
    let p = theme::c();
    let (fill, text_color, cursor) = if enabled {
        (p.accent, p.on_accent, egui::CursorIcon::PointingHand)
    } else {
        (p.faint, p.text_dim, egui::CursorIcon::NotAllowed)
    };
    ui.add(
        egui::Button::new(
            RichText::new(text)
                .size(theme::font::BODY)
                .strong()
                .color(text_color),
        )
        .fill(fill)
        .corner_radius(CornerRadius::same(theme::RADIUS_MD)),
    )
    .on_hover_cursor(cursor)
}

/// 图标操作钮;`tip` 为悬停提示(None 不挂 tooltip),`danger` 悬停
/// 显示警示底色与文字色
pub fn icon_btn(
    ui: &mut egui::Ui,
    glyph: &str,
    tip: Option<String>,
    danger: bool,
    enabled: bool,
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 22.0), sense);
    let hovered = enabled && resp.hovered();
    if hovered {
        let bg = if danger {
            theme::c().danger_soft
        } else {
            theme::c().hover_bg
        };
        ui.painter()
            .rect_filled(rect, CornerRadius::same(theme::RADIUS_SM), bg);
    }
    let color = if !enabled {
        theme::c().stroke_strong
    } else if hovered {
        if danger {
            theme::c().danger
        } else {
            theme::c().text
        }
    } else {
        theme::c().text_dim
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(theme::font::SM),
        color,
    );
    let resp = if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp.on_hover_cursor(egui::CursorIcon::NotAllowed)
    };
    if let Some(tip) = tip {
        resp.on_hover_text(tip)
    } else {
        resp
    }
}
