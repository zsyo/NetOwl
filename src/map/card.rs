//! 悬停归属节点的信息卡:节点名、总流量与连接明细。
//! 卡宽固定,行内左右两段文本先测宽再按份额尾部省略,长域名与长进程名
//! 不再叠印或溢出卡片。

use std::collections::HashMap;

use eframe::egui;
use egui::{
    Align2, Color32, CornerRadius, FontId, Pos2, Rect, Stroke, StrokeKind, TextureHandle, Vec2,
};

use crate::i18n::I18n;
use crate::model::{Connection, Place, fmt_bytes};
use crate::net::geoip;
use crate::net::rdns;
use crate::ui::theme;

/// 单行文本测宽(与绘制同字体,保证测量与呈现口径一致)
fn text_width(painter: &egui::Painter, text: &str, font: &FontId) -> f32 {
    painter
        .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
        .rect
        .width()
}

/// 按像素尾部省略:不超宽原样返回,超宽则去尾补省略号直到可容纳
fn fit_text(painter: &egui::Painter, text: &str, font: &FontId, max_w: f32) -> String {
    if text_width(painter, text, font) <= max_w {
        return text.to_owned();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while chars.len() > 1 {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "…";
        if text_width(painter, &candidate, font) <= max_w {
            return candidate;
        }
    }
    "…".to_owned()
}

/// 悬停归属节点的信息卡:节点名、总流量、最多 6 条连接明细
#[allow(clippy::too_many_arguments)]
pub(super) fn info_card(
    painter: &egui::Painter,
    canvas: Rect,
    place: Place,
    conns: &[Connection],
    i18n: &I18n,
    rdns: &rdns::Rdns,
    icon_tex: &HashMap<String, Option<TextureHandle>>,
    default_icon_tex: Option<&TextureHandle>,
) {
    const WIDTH: f32 = 310.0;
    const LINE_H: f32 = 17.0;
    const HEAD_H: f32 = 42.0;
    const MAX_ROWS: usize = 6;
    const PAD: f32 = 14.0;
    /// 左右两段之间的最小间距
    const GAP: f32 = 10.0;
    /// 拥挤时左段(进程名)占可用宽的份额,其余归右段(远端)
    const LEFT_SHARE: f32 = 0.45;

    let rows: Vec<&Connection> = conns
        .iter()
        .filter(|conn| conn.city == Some(place))
        .collect();
    let shown = rows.len().min(MAX_ROWS);
    let extra = rows.len() - shown;
    let total: u64 = rows.iter().map(|conn| conn.total_bytes()).sum();
    let height = HEAD_H + shown as f32 * LINE_H + if extra > 0 { LINE_H } else { 0.0 } + 8.0;
    let card = Rect::from_min_size(
        Pos2::new(canvas.left() + PAD, canvas.top() + PAD),
        Vec2::new(WIDTH, height),
    );

    // 浮层投影 + 卡片底 + 1px 描边(悬浮于地图之上,投影强化浮起层次)
    painter.add(theme::popup_shadow().as_shape(card, CornerRadius::same(theme::RADIUS_LG)));
    painter.rect_filled(
        card,
        CornerRadius::same(theme::RADIUS_LG),
        theme::c().bg_float,
    );
    painter.rect_stroke(
        card,
        CornerRadius::same(theme::RADIUS_LG),
        Stroke::new(1.0, theme::c().stroke),
        StrokeKind::Inside,
    );

    // 头部:右上角字节量先测宽占位,地点名在剩余宽度内省略
    let bytes_text = fmt_bytes(total);
    let bytes_font = FontId::proportional(12.0);
    let title_font = FontId::proportional(16.0);
    let title_max =
        (WIDTH - PAD * 2.0 - GAP - text_width(painter, &bytes_text, &bytes_font)).max(0.0);
    let title = fit_text(
        painter,
        &geoip::place_label(place, i18n),
        &title_font,
        title_max,
    );
    painter.text(
        Pos2::new(card.left() + PAD, card.top() + 12.0),
        Align2::LEFT_TOP,
        title,
        title_font,
        theme::c().text,
    );
    painter.text(
        Pos2::new(card.right() - PAD, card.top() + 14.0),
        Align2::RIGHT_TOP,
        bytes_text,
        bytes_font,
        theme::c().text_dim,
    );

    let inner = WIDTH - PAD * 2.0 - GAP;
    let process_font = FontId::proportional(12.0);
    let remote_font = FontId::monospace(11.0);
    let mut y = card.top() + HEAD_H - 4.0;
    for conn in rows.iter().take(MAX_ROWS) {
        // 进程图标(14px);无图标时文本左缘保持一致,信息卡行不留空位
        let mut text_x = card.left() + PAD;
        let mut icon_w = 0.0;
        if let Some(tex) = conn
            .proc_path
            .as_deref()
            .and_then(|p| icon_tex.get(p))
            .and_then(|t| t.as_ref())
            .or(default_icon_tex)
        {
            let icon_rect = Rect::from_min_size(
                Pos2::new(text_x, y + LINE_H / 2.0 - 7.0),
                Vec2::new(14.0, 14.0),
            );
            painter.image(
                tex.id(),
                icon_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
            text_x += 18.0;
            icon_w = 18.0;
        }
        let process = format!("{} ({})", conn.process, conn.pid);
        // 域名优先、无 PTR 回退地址:端口;超长交给像素省略,不做字符预截
        let remote = match rdns.lookup(conn.remote_ip) {
            Some(host) => format!("{host}:{}", conn.remote_port),
            None => conn.remote_display(),
        };
        let left_need = icon_w + text_width(painter, &process, &process_font);
        let right_need = text_width(painter, &remote, &remote_font);
        let (left_max, right_max) = if left_need + right_need <= inner {
            (left_need, right_need)
        } else {
            (inner * LEFT_SHARE, inner * (1.0 - LEFT_SHARE))
        };
        let process_shown = fit_text(
            painter,
            &process,
            &process_font,
            (left_max - icon_w).max(0.0),
        );
        let remote_shown = fit_text(painter, &remote, &remote_font, right_max);
        painter.text(
            Pos2::new(text_x, y + 8.0),
            Align2::LEFT_CENTER,
            process_shown,
            process_font.clone(),
            theme::c().text,
        );
        painter.text(
            Pos2::new(card.right() - PAD, y + 8.0),
            Align2::RIGHT_CENTER,
            remote_shown,
            remote_font.clone(),
            theme::c().text_dim,
        );
        y += LINE_H;
    }
    if extra > 0 {
        painter.text(
            Pos2::new(card.right() - PAD, y + 8.0),
            Align2::RIGHT_CENTER,
            i18n.t_with_args("map-info-more", &[("n", extra.to_string())]),
            FontId::proportional(11.0),
            theme::c().text_dim,
        );
    }
}
