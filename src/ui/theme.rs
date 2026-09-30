//! 主题:深/浅两套调色板、Visuals 定制、字体加载。
//!
//! 颜色全部经 [`c()`] 取当前主题调色板,禁止散落硬编码色值(规范 3);
//! 切换主题时由设置页调用 [`set_theme`] 立即重刷 Visuals。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use eframe::egui;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, Margin, Shadow, Stroke, Vec2, Visuals,
};

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
const WINDOW_SHADOW_DARK: Shadow = Shadow {
    offset: [0, 8],
    blur: 24,
    spread: 0,
    color: Color32::from_black_alpha(130),
};
const POPUP_SHADOW_DARK: Shadow = Shadow {
    offset: [0, 4],
    blur: 12,
    spread: 0,
    color: Color32::from_black_alpha(100),
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

/// 调色板:界面与地图全部颜色,深浅主题各一套
pub struct Palette {
    // 强调色与语义色
    /// 唯一强调色 #5C9DFF(深色主题)
    pub accent: Color32,
    /// 强调色低透明底(选中态)
    pub accent_soft: Color32,
    /// 入站语义色
    pub inbound: Color32,
    /// 出站语义色
    pub outbound: Color32,
    /// 监控运行状态点
    pub status_ok: Color32,
    /// 中间警示态(介于正常与异常之间,如倒计时余量不足)
    pub status_warn: Color32,
    /// 阻断语义色(规则命中阻断/错误提示)
    pub danger: Color32,
    // 背景层级
    /// 主背景(中央区域)
    pub bg_base: Color32,
    /// 导航面板背景
    pub bg_panel: Color32,
    /// 地图画布背景(海洋)
    pub bg_map: Color32,
    /// 地图陆地填充
    pub map_land: Color32,
    /// 地图海岸线描边
    pub map_coast: Color32,
    /// 地图国界线描边
    pub map_border: Color32,
    /// 地图主要河流线
    pub map_river: Color32,
    /// 地图经纬网格
    pub map_grid: Color32,
    /// 地图节点默认填充
    pub map_node: Color32,
    /// 地图国家名标签
    pub map_label_country: Color32,
    /// 地图海洋名标签
    pub map_label_sea: Color32,
    /// 中国省级名称标签(弱化,不与国家名争层级)
    pub map_label_province: Color32,
    /// 南海断续国界线(十段线)
    pub map_south_sea_line: Color32,
    /// 卡片/信息浮层背景
    pub bg_card: Color32,
    /// 浮起层背景(弹窗/菜单/悬浮控件,比卡片更高一级)
    pub bg_elevated: Color32,
    /// 信息浮层(带透明度,悬浮于地图之上)
    pub bg_float: Color32,
    /// 条纹行/微弱填充
    pub faint: Color32,
    // 前景
    pub text: Color32,
    pub text_dim: Color32,
    /// 强调色上的文字(强调色填充按钮/徽章内)
    pub on_accent: Color32,
    /// 常规描边
    pub stroke: Color32,
    /// 强描边(输入框/卡片边界,弱化描边强调层级时使用)
    pub stroke_strong: Color32,
    // 交互态
    /// 控件悬停底色
    pub hover_bg: Color32,
    /// 控件展开底色(下拉菜单打开等)
    pub open_bg: Color32,
}

/// 深色主题:绿蓝地图(海洋深蓝、陆地深绿)
const DARK: Palette = Palette {
    accent: Color32::from_rgb(92, 157, 255),
    accent_soft: Color32::from_rgba_unmultiplied_const(92, 157, 255, 34),
    inbound: Color32::from_rgb(126, 224, 163),
    outbound: Color32::from_rgb(92, 207, 230),
    status_ok: Color32::from_rgb(88, 214, 141),
    status_warn: Color32::from_rgb(232, 192, 92),
    danger: Color32::from_rgb(236, 106, 106),
    bg_base: Color32::from_rgb(16, 18, 24),
    bg_panel: Color32::from_rgb(22, 25, 33),
    bg_map: Color32::from_rgb(12, 24, 34),
    map_land: Color32::from_rgb(30, 58, 48),
    map_coast: Color32::from_rgb(70, 120, 95),
    map_border: Color32::from_rgb(52, 92, 74),
    // 河流:偏亮水蓝,在深绿陆地上呈水系脉络,暗于海名标签避免抢视觉
    map_river: Color32::from_rgb(58, 104, 134),
    map_grid: Color32::from_rgb(18, 32, 42),
    map_node: Color32::from_rgb(118, 152, 138),
    map_label_country: Color32::from_rgb(150, 190, 168),
    map_label_sea: Color32::from_rgb(100, 155, 196),
    // 省名取中性蓝灰,明显弱于亮青的国家名
    map_label_province: Color32::from_rgb(128, 146, 162),
    // 十段线:比国界亮的强调青,突出断续主权界
    map_south_sea_line: Color32::from_rgb(214, 226, 138),
    bg_card: Color32::from_rgb(28, 32, 41),
    bg_elevated: Color32::from_rgb(36, 41, 53),
    bg_float: Color32::from_rgba_unmultiplied_const(28, 32, 41, 240),
    faint: Color32::from_rgb(26, 29, 38),
    text: Color32::from_rgb(222, 226, 235),
    text_dim: Color32::from_rgb(140, 147, 164),
    on_accent: Color32::from_rgb(255, 255, 255),
    stroke: Color32::from_rgb(48, 53, 66),
    stroke_strong: Color32::from_rgb(62, 70, 90),
    hover_bg: Color32::from_rgb(40, 45, 57),
    open_bg: Color32::from_rgb(36, 41, 53),
};

/// 浅色主题:LS 式绿蓝地图(海洋浅蓝、陆地浅绿、白色国界)
const LIGHT: Palette = Palette {
    accent: Color32::from_rgb(47, 127, 232),
    accent_soft: Color32::from_rgba_unmultiplied_const(47, 127, 232, 36),
    inbound: Color32::from_rgb(30, 148, 94),
    outbound: Color32::from_rgb(20, 134, 168),
    status_ok: Color32::from_rgb(34, 160, 100),
    status_warn: Color32::from_rgb(197, 144, 34),
    danger: Color32::from_rgb(198, 60, 60),
    bg_base: Color32::from_rgb(243, 245, 248),
    bg_panel: Color32::from_rgb(233, 237, 242),
    bg_map: Color32::from_rgb(168, 205, 230),
    map_land: Color32::from_rgb(215, 229, 196),
    map_coast: Color32::from_rgb(150, 178, 138),
    map_border: Color32::from_rgb(255, 255, 255),
    // 河流:浅色陆地上取偏深水蓝,与海洋同族但更饱和
    map_river: Color32::from_rgb(120, 158, 190),
    map_grid: Color32::from_rgb(140, 172, 200),
    map_node: Color32::from_rgb(96, 116, 150),
    map_label_country: Color32::from_rgb(96, 110, 88),
    map_label_sea: Color32::from_rgb(74, 122, 168),
    // 浅色主题省名用浅蓝灰
    map_label_province: Color32::from_rgb(120, 128, 140),
    // 十段线:浅色主题用深金棕,避免与绿色陆地国界混同
    map_south_sea_line: Color32::from_rgb(176, 122, 40),
    bg_card: Color32::from_rgb(255, 255, 255),
    bg_elevated: Color32::from_rgb(255, 255, 255),
    bg_float: Color32::from_rgba_unmultiplied_const(255, 255, 255, 240),
    faint: Color32::from_rgb(228, 232, 238),
    text: Color32::from_rgb(31, 36, 48),
    text_dim: Color32::from_rgb(90, 98, 114),
    on_accent: Color32::from_rgb(255, 255, 255),
    stroke: Color32::from_rgb(198, 204, 216),
    stroke_strong: Color32::from_rgb(172, 181, 198),
    hover_bg: Color32::from_rgb(222, 228, 236),
    open_bg: Color32::from_rgb(232, 236, 242),
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
    v.window_stroke = Stroke::new(1.0, p.stroke);
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
