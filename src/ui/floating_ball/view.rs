//! 悬浮窗绘制:长条速率条(贴边半隐信号格窄条 / 全显速率两行)
//! 与详情浮窗气泡。底色与描边走调色板。

use eframe::egui;
use egui::{Align, Label, Layout, RichText, Stroke};

use super::types::{BAR_H, BAR_W, BallData, ProcRate, RATE_STEPS, STRIP_W};
use crate::i18n::I18n;
use crate::model::fmt_bytes;
use crate::ui::Page;
use crate::ui::{icons, theme, widgets};

/// 控件绘制模式:贴边半隐(窄条信号格)或全显(长条速率两行)
pub(super) enum BallMode {
    /// 贴边半隐:控件大部分移出窗口被裁剪,只露贴边侧窄条;
    /// bars_left = 条在控件左端(即控件贴右屏缘)
    Docked { bars_left: bool },
    /// 全显:长条两行 上传/下载速率
    Full,
}

/// 速率紧凑格式:去单位空格,球面与榜单共用("1.5MB/s")
fn fmt_rate(n: u64) -> String {
    format!("{}/s", fmt_bytes(n).replace(' ', ""))
}

/// 长条控件:圆角矩形底 + 细描边 + 按模式的窄条信号格或速率两行
pub(super) fn ball(ui: &mut egui::Ui, cx: f32, cy: f32, data: &BallData, mode: BallMode) {
    let painter = ui.painter();
    let rect = egui::Rect::from_center_size(egui::pos2(cx, cy), egui::vec2(BAR_W, BAR_H));
    let radius = egui::CornerRadius::same(theme::RADIUS_SM);
    painter.rect_filled(rect, radius, theme::c().bg_float);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, theme::c().stroke),
        egui::StrokeKind::Inside,
    );

    match mode {
        BallMode::Docked { bars_left } => signal_grid(painter, cx, data, bars_left),
        BallMode::Full => rate_stack(painter, cx, cy, data),
    }
}

/// 贴边半隐窄条信号格:上下两组各 4 格、组内从下往上堆叠——上组 =
/// 上传(贴条顶)、下组 = 下载(贴条底),组间留明显间隔;速率达到
/// 档位阈值时从各自组的最低格向上点亮
fn signal_grid(painter: &egui::Painter, cx: f32, data: &BallData, bars_left: bool) {
    let (bar_w, bar_h, gap) = (8.0_f32, 3.0_f32, 2.0_f32);
    // 窄条位于控件贴屏侧端部:由控件中心与端部方向推出条中心 x
    let strip_cx = if bars_left {
        cx - BAR_W * 0.5 + STRIP_W * 0.5
    } else {
        cx + BAR_W * 0.5 - STRIP_W * 0.5
    };
    // (速率, 点亮色, 组底 y):下载组贴条底、上传组贴中线,组间间隔 =
    // 中线 - 条底 - 组高,随 BAR_H 自适应
    let groups = [
        (data.rates.0, theme::c().inbound, BAR_H - 4.0),
        (data.rates.1, theme::c().outbound, BAR_H * 0.5 - 4.0),
    ];
    for (rate, color, group_bottom) in groups {
        for (g, step) in RATE_STEPS.iter().enumerate() {
            let lit = rate >= *step;
            let color = if lit { color } else { theme::c().faint };
            let y = group_bottom - bar_h - g as f32 * (bar_h + gap);
            let rect = egui::Rect::from_min_size(
                egui::pos2(strip_cx - bar_w * 0.5, y),
                egui::vec2(bar_w, bar_h),
            );
            // 点亮格辉光垫底(扩大 1.5px 的低透明层)
            if lit {
                painter.rect_filled(
                    rect.expand2(egui::vec2(1.5, 1.5)),
                    egui::CornerRadius::same(2),
                    color.gamma_multiply(0.28),
                );
            }
            painter.rect_filled(rect, egui::CornerRadius::same(1), color);
        }
    }
}

/// 全显长条:两行 上传速率 / 下载速率(各占半高,水平居中)
fn rate_stack(painter: &egui::Painter, cx: f32, cy: f32, data: &BallData) {
    draw_centered(
        painter,
        cx,
        cy - BAR_H * 0.5 + 6.0,
        format!("{}{}", icons::ARROW_UP, fmt_rate(data.rates.1)),
        theme::c().outbound,
    );
    draw_centered(
        painter,
        cx,
        cy + 4.0,
        format!("{}{}", icons::ARROW_DOWN, fmt_rate(data.rates.0)),
        theme::c().inbound,
    );
}

/// 长条内居中单行文字(galley 测宽后水平居中,top 为行顶)
fn draw_centered(painter: &egui::Painter, cx: f32, top: f32, text: String, color: egui::Color32) {
    let galley = painter.layout_no_wrap(text, egui::FontId::proportional(theme::font::SM), color);
    let w = galley.rect.width();
    painter.galley(egui::pos2(cx - w * 0.5, top), galley, color);
}

/// 圆角浮层面板底(半透明 bg_float + 细描边;菜单窗口共用)
pub(super) fn panel_bg(ui: &mut egui::Ui, rect: egui::Rect) {
    let radius = egui::CornerRadius::same(theme::RADIUS_LG);
    ui.painter().rect_filled(rect, radius, theme::c().bg_float);
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, theme::c().stroke),
        egui::StrokeKind::Inside,
    );
}

/// 详情浮窗(气泡):品牌头 + 总速率 + 实时上传/下载 Top-N 榜单 + 查看详情
pub(super) fn hover_panel(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    data: &BallData,
    i18n: &I18n,
    show_main: &mut Option<Page>,
) {
    panel_bg(ui, rect);
    // 布局约束在气泡 rect 内:弹性贴底与按钮都以气泡底边为界,
    // 不再溢出到窗口底部与球重叠
    let mut panel_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: theme::sp::LG as i8,
            right: theme::sp::LG as i8,
            top: theme::sp::MD as i8,
            bottom: theme::sp::MD as i8,
        })
        .show(&mut panel_ui, |ui| {
            // 品牌头一行:logo + 程序名 + 右侧总上传/下载速率
            ui.horizontal(|ui| {
                if let Some(logo) = &data.logo {
                    ui.add(egui::Image::new(logo).fit_to_exact_size(egui::vec2(18.0, 18.0)));
                }
                ui.label(
                    RichText::new(crate::APP_NAME)
                        .size(theme::font::H3)
                        .strong()
                        .color(theme::c().text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{}{}", icons::ARROW_DOWN, fmt_rate(data.rates.0)))
                            .size(theme::font::XS)
                            .color(theme::c().inbound),
                    );
                    ui.add_space(theme::sp::SM);
                    ui.label(
                        RichText::new(format!("{}{}", icons::ARROW_UP, fmt_rate(data.rates.1)))
                            .size(theme::font::XS)
                            .color(theme::c().outbound),
                    );
                });
            });
            ui.add_space(theme::sp::SM);
            ui.separator();
            ui.add_space(theme::sp::XS);

            if data.elevated {
                section(
                    ui,
                    &i18n.t("ball-realtime-up"),
                    &data.up_top,
                    icons::ARROW_UP,
                    theme::c().outbound,
                    data.default_icon.as_ref(),
                    i18n,
                );
                ui.add_space(theme::sp::SM);
                section(
                    ui,
                    &i18n.t("ball-realtime-down"),
                    &data.down_top,
                    icons::ARROW_DOWN,
                    theme::c().inbound,
                    data.default_icon.as_ref(),
                    i18n,
                );
            } else {
                hint_line(ui, &i18n.t("ball-no-etw"));
            }

            // 按钮区:分隔线 + 按钮,紧跟榜单流式布局(不弹性贴底,
            // 气泡高度以内容校准,多余留白只落在气泡底部)
            ui.add_space(theme::sp::XS);
            ui.separator();
            ui.add_space(theme::sp::SM);
            if ui
                .add_sized(
                    [ui.available_width(), 26.0],
                    egui::Button::new(
                        RichText::new(i18n.t("ball-detail"))
                            .size(theme::font::BODY)
                            .strong()
                            .color(theme::c().on_accent),
                    )
                    .fill(theme::c().accent)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_MD)),
                )
                .clicked()
            {
                *show_main = Some(Page::Connections);
            }
        });
}

/// 榜单分组:标题 + 行(空榜显示占位提示)
fn section(
    ui: &mut egui::Ui,
    title: &str,
    rows: &[ProcRate],
    glyph: &str,
    color: egui::Color32,
    default_icon: Option<&egui::TextureHandle>,
    i18n: &I18n,
) {
    ui.label(
        RichText::new(title)
            .size(theme::font::XS)
            .color(theme::c().text_dim),
    );
    ui.add_space(theme::sp::XS);
    if rows.is_empty() {
        hint_line(ui, &i18n.t("ball-empty"));
        return;
    }
    for r in rows {
        proc_row(ui, r, default_icon, glyph, color);
    }
}

/// 提示行(极小字号,弱化色)
fn hint_line(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(theme::font::XS)
            .color(theme::c().text_dim),
    );
}

/// 榜单行:图标 + 名称(截断)+ 方向速率(右对齐);上传占优行名称
/// 用警示色(与地图页 Inspector 同规则)
fn proc_row(
    ui: &mut egui::Ui,
    r: &ProcRate,
    default_icon: Option<&egui::TextureHandle>,
    glyph: &str,
    color: egui::Color32,
) {
    // 速率列预留宽(下行 "↓123MB/s" 也不截断)
    const RATE_COL_W: f32 = 84.0;
    // 本行展示的速率:上传榜取 up,下载榜取 down(由方向字形色区分)
    let rate = if color == theme::c().outbound {
        r.up
    } else {
        r.down
    };
    ui.horizontal(|ui| {
        let name_w = ui.available_width() - RATE_COL_W;
        ui.allocate_ui(egui::vec2(name_w, 20.0), |ui| {
            ui.set_min_width(name_w);
            ui.horizontal(|ui| {
                widgets::process::proc_icon(ui, r.icon.as_ref(), default_icon, 16.0);
                // 上传占优视为可疑(可能是未经预期的外发流量)
                let warn = r.up > r.down && r.up > 0;
                ui.add(
                    Label::new(RichText::new(&r.name).size(theme::font::SM).color(if warn {
                        theme::c().status_warn
                    } else {
                        theme::c().text
                    }))
                    .wrap_mode(egui::TextWrapMode::Truncate),
                );
            });
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.set_min_width(RATE_COL_W);
            ui.label(
                RichText::new(format!("{glyph}{}", fmt_rate(rate)))
                    .size(theme::font::XS)
                    .color(color),
            );
        });
    });
}
