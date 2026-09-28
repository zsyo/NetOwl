//! Bootstrap Icons 码点常量(assets/icons.ttf 编译期内嵌,字体版本 1.0,共 2078 字形)。
//!
//! 码点经 fontTools 对照字体 cmap 逐一提取验证,注释即真实 glyph 名
//! (注释即契约,AGENTS.md 规范 9);字体由 theme::install_fonts 挂到
//! Proportional / Monospace 的 fallback 链尾,私有区(PUA)码点在普通
//! 文本中直接可用,无需切换字体族。

/// glyph: globe(流量地图)
pub const GLOBE: &str = "\u{f3ee}";
/// glyph: list-ul(连接列表)
pub const LIST_UL: &str = "\u{f478}";
/// glyph: clock-history(连接历史)
pub const CLOCK_HISTORY: &str = "\u{f292}";
/// glyph: shield(拦截规则)
pub const SHIELD: &str = "\u{f53f}";
/// glyph: gear(设置)
pub const GEAR: &str = "\u{f3e5}";
/// glyph: caret-up-fill(表头正序三角)
pub const CARET_UP_FILL: &str = "\u{f235}";
/// glyph: caret-down-fill(表头倒序三角)
pub const CARET_DOWN_FILL: &str = "\u{f229}";
/// glyph: check-lg(允许)
pub const CHECK_LG: &str = "\u{f633}";
/// glyph: x-lg(拒绝)
pub const X_LG: &str = "\u{f659}";
