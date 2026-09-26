//! 主题:颜色/圆角常量、字体加载与 Visuals 定制。

use std::sync::Arc;

use eframe::egui;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, Margin, Stroke, Vec2, Visuals,
};

// ---- 强调色与语义色(规范 3:唯一强调色,新增色前先查重)----
/// 唯一强调色 #5C9DFF(深色主题)
pub const ACCENT: Color32 = Color32::from_rgb(92, 157, 255);
/// 强调色低透明底(选中态)
pub const ACCENT_SOFT: Color32 = Color32::from_rgba_unmultiplied_const(92, 157, 255, 34);
/// 入站语义色
pub const INBOUND: Color32 = Color32::from_rgb(126, 224, 163);
/// 出站语义色
pub const OUTBOUND: Color32 = Color32::from_rgb(92, 207, 230);
/// 监控运行状态点
pub const STATUS_OK: Color32 = Color32::from_rgb(88, 214, 141);

// ---- 背景层级 ----
/// 主背景(中央区域)
pub const BG_BASE: Color32 = Color32::from_rgb(16, 18, 24);
/// 导航面板背景
pub const BG_PANEL: Color32 = Color32::from_rgb(22, 25, 33);
/// 地图画布背景
pub const BG_MAP: Color32 = Color32::from_rgb(13, 15, 21);
/// 地图陆地轮廓线框
pub const MAP_COAST: Color32 = Color32::from_rgb(58, 66, 86);
/// 地图经纬网格
pub const MAP_GRID: Color32 = Color32::from_rgb(30, 34, 46);
/// 地图节点默认填充
pub const MAP_NODE: Color32 = Color32::from_rgb(120, 140, 178);
/// 卡片/信息浮层背景
pub const BG_CARD: Color32 = Color32::from_rgb(28, 32, 41);
/// 信息浮层(带透明度,悬浮于地图之上)
pub const BG_FLOAT: Color32 = Color32::from_rgba_unmultiplied_const(28, 32, 41, 240);
/// 条纹行/微弱填充
pub const FAINT: Color32 = Color32::from_rgb(26, 29, 38);

// ---- 前景 ----
pub const TEXT: Color32 = Color32::from_rgb(222, 226, 235);
pub const TEXT_DIM: Color32 = Color32::from_rgb(140, 147, 164);
/// 常规描边
pub const STROKE: Color32 = Color32::from_rgb(48, 53, 66);

// ---- 圆角刻度(规范 3,不出现圆角魔法数字)----
pub const RADIUS_SM: u8 = 4;
pub const RADIUS_MD: u8 = 8;
pub const RADIUS_LG: u8 = 12;

/// 安装字体与视觉样式
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    apply_visuals(ctx);
}

/// 加载 Windows 自带微软雅黑(中文 fallback)与 Consolas(monospace)。
/// 字体缺失时按规范显式报错,不做静默降级。
fn install_fonts(ctx: &egui::Context) {
    let msyh = std::fs::read("C:/Windows/Fonts/msyh.ttc")
        .expect("NetOwl 需要 Windows 自带字体 C:/Windows/Fonts/msyh.ttc(微软雅黑)来渲染界面");
    let consola = std::fs::read("C:/Windows/Fonts/consola.ttf")
        .expect("NetOwl 需要 Windows 自带字体 C:/Windows/Fonts/consola.ttf(Consolas)");

    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("msyh".to_owned(), Arc::new(FontData::from_owned(msyh)));
    // 追加到 proportional 末尾:拉丁字形仍由内置 Inter 渲染,中文回退到雅黑
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .expect("default proportional family")
        .push("msyh".to_owned());
    fonts.font_data.insert("consolas".to_owned(), Arc::new(FontData::from_owned(consola)));
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .expect("default monospace family")
        .insert(0, "consolas".to_owned());
    ctx.set_fonts(fonts);
}

/// 深色主题定制:背景层级、控件交互态、圆角与间距
fn apply_visuals(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.panel_fill = BG_PANEL;
    v.window_fill = BG_BASE;
    v.extreme_bg_color = BG_BASE;
    v.faint_bg_color = FAINT;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT_SOFT;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.window_corner_radius = CornerRadius::same(RADIUS_LG);
    v.menu_corner_radius = CornerRadius::same(RADIUS_MD);

    let widget = |mut w: egui::style::WidgetVisuals, bg: Color32, fg: Color32, r: u8| {
        w.bg_fill = bg;
        w.weak_bg_fill = bg;
        w.fg_stroke = Stroke::new(1.0, fg);
        w.corner_radius = CornerRadius::same(r);
        w
    };
    v.widgets.noninteractive = widget(
        v.widgets.noninteractive,
        BG_CARD,
        TEXT,
        RADIUS_SM,
    );
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, STROKE);
    v.widgets.inactive = widget(v.widgets.inactive, Color32::TRANSPARENT, TEXT_DIM, RADIUS_MD);
    v.widgets.hovered = widget(v.widgets.hovered, Color32::from_rgb(40, 45, 57), TEXT, RADIUS_MD);
    v.widgets.active = widget(v.widgets.active, ACCENT_SOFT, ACCENT, RADIUS_MD);
    v.widgets.open = widget(v.widgets.open, Color32::from_rgb(36, 41, 53), TEXT, RADIUS_MD);

    ctx.all_styles_mut(|style| {
        style.visuals = v.clone();
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
        style.spacing.menu_margin = Margin::same(6);
    });
}

/// 带字号的强调色文本
pub fn accent_text(text: &str, size: f32) -> egui::RichText {
    egui::RichText::new(text).size(size).color(ACCENT)
}

/// 弱化文本
pub fn dim_text(text: &str, size: f32) -> egui::RichText {
    egui::RichText::new(text).size(size).color(TEXT_DIM)
}
