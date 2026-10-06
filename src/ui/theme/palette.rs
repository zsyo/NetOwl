//! 调色板:Palette 字段定义与深/浅两套颜色字面量(纯数据)。
//! 取色入口在 super::theme::c()。

use eframe::egui::Color32;

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

/// 深色主题:冷色 HUD(深冷蓝黑底、电光青 accent、暗色科技底图:
/// 深蓝海洋 + 亮蓝灰海岸)
pub(super) const DARK: Palette = Palette {
    accent: Color32::from_rgb(34, 211, 238),
    accent_soft: Color32::from_rgba_unmultiplied_const(34, 211, 238, 34),
    inbound: Color32::from_rgb(126, 224, 163),
    outbound: Color32::from_rgb(92, 207, 230),
    status_ok: Color32::from_rgb(88, 214, 141),
    status_warn: Color32::from_rgb(232, 192, 92),
    danger: Color32::from_rgb(236, 106, 106),
    bg_base: Color32::from_rgb(11, 15, 23),
    bg_panel: Color32::from_rgb(15, 20, 32),
    bg_map: Color32::from_rgb(8, 17, 32),
    map_land: Color32::from_rgb(21, 34, 50),
    map_coast: Color32::from_rgb(62, 110, 158),
    map_border: Color32::from_rgb(44, 74, 110),
    // 河流:偏亮水蓝,在暗青陆地上呈水系脉络,暗于海名标签避免抢视觉
    map_river: Color32::from_rgb(39, 86, 122),
    map_grid: Color32::from_rgb(14, 27, 44),
    map_node: Color32::from_rgb(111, 214, 236),
    map_label_country: Color32::from_rgb(155, 196, 224),
    map_label_sea: Color32::from_rgb(93, 138, 184),
    // 省名取中性蓝灰,明显弱于亮青的国家名
    map_label_province: Color32::from_rgb(126, 144, 168),
    // 十段线:比国界亮的强调色,突出断续主权界
    map_south_sea_line: Color32::from_rgb(214, 226, 138),
    bg_card: Color32::from_rgb(19, 26, 41),
    bg_elevated: Color32::from_rgb(26, 35, 52),
    bg_float: Color32::from_rgba_unmultiplied_const(19, 26, 41, 240),
    faint: Color32::from_rgb(17, 23, 36),
    text: Color32::from_rgb(222, 229, 240),
    text_dim: Color32::from_rgb(135, 146, 166),
    on_accent: Color32::from_rgb(255, 255, 255),
    stroke: Color32::from_rgb(42, 51, 69),
    stroke_strong: Color32::from_rgb(56, 67, 89),
    hover_bg: Color32::from_rgb(27, 35, 51),
    open_bg: Color32::from_rgb(26, 35, 52),
};

/// 浅色主题:LS 式地图(海洋浅蓝、陆地浅绿、白色国界),accent 换
/// 冷青与新设计语言呼应
pub(super) const LIGHT: Palette = Palette {
    accent: Color32::from_rgb(8, 145, 178),
    accent_soft: Color32::from_rgba_unmultiplied_const(8, 145, 178, 36),
    inbound: Color32::from_rgb(30, 148, 94),
    outbound: Color32::from_rgb(20, 134, 168),
    status_ok: Color32::from_rgb(34, 160, 100),
    status_warn: Color32::from_rgb(197, 144, 34),
    danger: Color32::from_rgb(198, 60, 60),
    bg_base: Color32::from_rgb(243, 245, 248),
    bg_panel: Color32::from_rgb(233, 237, 242),
    bg_map: Color32::from_rgb(168, 205, 230),
    map_land: Color32::from_rgb(211, 226, 216),
    map_coast: Color32::from_rgb(127, 163, 147),
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
