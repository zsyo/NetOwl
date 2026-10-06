//! 主题:深/浅两套调色板、Visuals 定制、字体加载。
//!
//! 颜色全部经 [`c()`] 取当前主题调色板,禁止散落硬编码色值(规范 3);
//! 切换主题时由设置页调用 [`set_theme`] 立即重刷 Visuals。
//! 调色板字段与颜色字面量见 palette。

mod palette;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use eframe::egui;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, Margin, Shadow, Stroke, Vec2, Visuals,
};

pub use palette::Palette;
use palette::{DARK, LIGHT};

// ---- 圆角刻度(规范 3,不出现圆角魔法数字)----
pub const RADIUS_SM: u8 = 4;
pub const RADIUS_MD: u8 = 8;
pub const RADIUS_LG: u8 = 12;
/// 胶囊形:圆角大于半高,渲染时被 clamp 成半圆(徽章/开关轨道)
pub const RADIUS_PILL: u8 = u8::MAX;

// ---- 字号刻度(全局文字尺寸统一入口,页面内不再散布字号魔法数字)----
pub mod font {
    /// 页面标题/品牌名
    pub const H1: f32 = 20.0;
    /// 卡片大数字/弹窗标题
    pub const H2: f32 = 17.0;
    /// 分组标题/导航项/加强按钮
    pub const H3: f32 = 14.0;
    /// 面板/浮层标题(介于 H3 与 H2,详情视图标题用)
    pub const PANEL_TITLE: f32 = 16.0;
    /// 正文
    pub const BODY: f32 = 13.0;
    /// 次要信息/表头
    pub const SM: f32 = 12.0;
    /// 辅助说明/路径
    pub const XS: f32 = 11.0;
    /// 徽章/极小提示
    pub const MICRO: f32 = 10.0;
}

// ---- 间距刻度(布局留白统一入口)----
pub mod sp {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
}

// ---- 阴影刻度(浮起层投影:窗口 > 弹层)----
// 深色阴影带冷色调(预乘 RGBA 的深蓝黑)呼应冷色 HUD 基调
const WINDOW_SHADOW_DARK: Shadow = Shadow {
    offset: [0, 8],
    blur: 24,
    spread: 0,
    color: Color32::from_rgba_premultiplied(3, 10, 18, 140),
};
const POPUP_SHADOW_DARK: Shadow = Shadow {
    offset: [0, 4],
    blur: 12,
    spread: 0,
    color: Color32::from_rgba_premultiplied(3, 10, 18, 105),
};
const WINDOW_SHADOW_LIGHT: Shadow = Shadow {
    offset: [0, 8],
    blur: 28,
    spread: 0,
    color: Color32::from_rgba_unmultiplied_const(31, 36, 48, 40),
};
const POPUP_SHADOW_LIGHT: Shadow = Shadow {
    offset: [0, 4],
    blur: 14,
    spread: 0,
    color: Color32::from_rgba_unmultiplied_const(31, 36, 48, 30),
};

static THEME: AtomicUsize = AtomicUsize::new(0);
const DARK_IDX: usize = 0;
const LIGHT_IDX: usize = 1;

/// 当前主题调色板(绘制代码统一入口)
pub fn c() -> &'static Palette {
    match THEME.load(Ordering::Relaxed) {
        LIGHT_IDX => &LIGHT,
        _ => &DARK,
    }
}

/// 是否深色主题
pub fn is_dark() -> bool {
    THEME.load(Ordering::Relaxed) == DARK_IDX
}

/// 当前主题字符串(持久化用:"dark" / "light")
pub fn theme_str() -> &'static str {
    if is_dark() { "dark" } else { "light" }
}

/// 切换主题并立即应用到 ctx(设置页调用)
pub fn set_theme(dark: bool, ctx: &egui::Context) {
    THEME.store(if dark { DARK_IDX } else { LIGHT_IDX }, Ordering::Relaxed);
    apply_visuals(ctx);
}

/// 按配置字符串应用主题("light" 为浅色,其余深色;启动时调用)
pub fn apply_theme_str(s: &str, ctx: &egui::Context) {
    set_theme(s != "light", ctx);
}

/// 安装字体与视觉样式(启动时按配置主题)
pub fn install(ctx: &egui::Context, theme: &str) {
    install_fonts(ctx);
    apply_theme_str(theme, ctx);
}

/// 加载 Windows 自带微软雅黑(中文 fallback)与 Consolas(monospace),
/// 并把内嵌 Bootstrap Icons 挂到两族 fallback 链尾(码点见 ui::icons)。
/// 系统字体缺失时按规范显式报错,不做静默降级。
fn install_fonts(ctx: &egui::Context) {
    let msyh = std::fs::read("C:/Windows/Fonts/msyh.ttc")
        .expect("NetOwl 需要 Windows 自带字体 C:/Windows/Fonts/msyh.ttc(微软雅黑)来渲染界面");
    let consola = std::fs::read("C:/Windows/Fonts/consola.ttf")
        .expect("NetOwl 需要 Windows 自带字体 C:/Windows/Fonts/consola.ttf(Consolas)");

    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("msyh".to_owned(), Arc::new(FontData::from_owned(msyh)));
    // 追加到 proportional 末尾:拉丁字形仍由内置 Inter 渲染,中文回退到雅黑
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .expect("default proportional family")
        .push("msyh".to_owned());
    fonts.font_data.insert(
        "consolas".to_owned(),
        Arc::new(FontData::from_owned(consola)),
    );
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .expect("default monospace family")
        .insert(0, "consolas".to_owned());
    // 图标字体:全私用区码点,正常文本不涉及;挂链尾兜底,由
    // ui::icons 的码点常量按名引用(码点对照 cmap 验证,注释即契约)
    fonts.font_data.insert(
        "bootstrap-icons".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/icons.ttf"
        ))),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .get_mut(&family)
            .expect("default family")
            .push("bootstrap-icons".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// 深浅主题定制:背景层级、阴影、控件交互态、圆角与间距
fn apply_visuals(ctx: &egui::Context) {
    let p = c();
    let mut v = if is_dark() {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = p.bg_panel;
    v.window_fill = p.bg_elevated;
    v.extreme_bg_color = p.bg_base;
    v.faint_bg_color = p.faint;
    v.override_text_color = Some(p.text);
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.window_corner_radius = CornerRadius::same(RADIUS_LG);
    // 窗口描边带 accent 辉光(冷色 HUD 门面);浅色主题保持中性描边
    v.window_stroke = if is_dark() {
        Stroke::new(1.0, p.accent_dim)
    } else {
        Stroke::new(1.0, p.stroke)
    };
    v.window_shadow = window_shadow();
    v.menu_corner_radius = CornerRadius::same(RADIUS_MD);
    v.popup_shadow = popup_shadow();

    let widget = |mut w: egui::style::WidgetVisuals, bg: Color32, fg: Color32, r: u8| {
        w.bg_fill = bg;
        w.weak_bg_fill = bg;
        w.fg_stroke = Stroke::new(1.0, fg);
        w.corner_radius = CornerRadius::same(r);
        w
    };
    v.widgets.noninteractive = widget(v.widgets.noninteractive, p.bg_card, p.text, RADIUS_SM);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.stroke);
    v.widgets.inactive = widget(
        v.widgets.inactive,
        Color32::TRANSPARENT,
        p.text_dim,
        RADIUS_MD,
    );
    v.widgets.hovered = widget(v.widgets.hovered, p.hover_bg, p.text, RADIUS_MD);
    v.widgets.active = widget(v.widgets.active, p.accent_soft, p.accent, RADIUS_MD);
    v.widgets.open = widget(v.widgets.open, p.open_bg, p.text, RADIUS_MD);
    // egui 0.36 的 button_style 按状态用 bg_stroke 宽度回缩按钮内边距
    // (inner_margin = button_padding - bg_stroke.width):inactive 宽 0 而
    // hovered/active/open 默认宽 1,按钮悬停瞬间会缩小 2px 造成抖动。
    // 与 egui menu_style 同思路统一清空,各状态尺寸恒定,
    // 悬停反馈由 hover_bg 背景承担,不再依赖描边
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.active.bg_stroke = Stroke::NONE;
    v.widgets.open.bg_stroke = Stroke::NONE;

    ctx.all_styles_mut(|style| {
        style.visuals = v.clone();
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
        style.spacing.menu_margin = Margin::same(6);
    });
}

/// 当前主题的窗口投影(egui::Window 等大浮层用)
pub fn window_shadow() -> Shadow {
    if is_dark() {
        WINDOW_SHADOW_DARK
    } else {
        WINDOW_SHADOW_LIGHT
    }
}

/// 当前主题的弹层投影(悬浮卡片/菜单等小浮层用)
pub fn popup_shadow() -> Shadow {
    if is_dark() {
        POPUP_SHADOW_DARK
    } else {
        POPUP_SHADOW_LIGHT
    }
}

/// 带字号的强调色文本
pub fn accent_text(text: &str, size: f32) -> egui::RichText {
    egui::RichText::new(text).size(size).color(c().accent)
}

/// 弱化文本
pub fn dim_text(text: &str, size: f32) -> egui::RichText {
    egui::RichText::new(text).size(size).color(c().text_dim)
}
