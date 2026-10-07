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
/// glyph: layout-sidebar(地图页左面板开关)
pub const LAYOUT_SIDEBAR: &str = "\u{f45f}";
/// glyph: layout-text-sidebar-reverse(地图页右面板开关)
pub const LAYOUT_TEXT_SIDEBAR_REVERSE: &str = "\u{f461}";
/// glyph: chevron-right(进程组收起态)
pub const CHEVRON_RIGHT: &str = "\u{f285}";
/// glyph: chevron-down(进程组展开态)
pub const CHEVRON_DOWN: &str = "\u{f282}";
/// glyph: ban(进程级阻断生效中)
pub const BAN: &str = "\u{f6b6}";

// ---- 通用操作 ----
/// glyph: search(搜索框)
pub const SEARCH: &str = "\u{f52a}";
/// glyph: plus(新增)
pub const PLUS: &str = "\u{f4fe}";
/// glyph: plus-lg(新增,大号)
pub const PLUS_LG: &str = "\u{f64d}";
/// glyph: pencil(编辑)
pub const PENCIL: &str = "\u{f4cb}";
/// glyph: trash(删除)
pub const TRASH: &str = "\u{f5de}";
/// glyph: arrow-up(上移/上传)
pub const ARROW_UP: &str = "\u{f148}";
/// glyph: arrow-down(下移/下载)
pub const ARROW_DOWN: &str = "\u{f128}";
/// glyph: arrow-down-up(收发双向)
pub const ARROW_DOWN_UP: &str = "\u{f127}";
/// glyph: arrow-repeat(刷新)
pub const ARROW_REPEAT: &str = "\u{f130}";
/// glyph: funnel(筛选)
pub const FUNNEL: &str = "\u{f3e1}";
/// glyph: download(历史导出)
pub const DOWNLOAD: &str = "\u{f30a}";
/// glyph: copy(窗口还原态)
pub const COPY: &str = "\u{f759}";

// ---- 状态与提示 ----
/// glyph: check-circle(启用/成功)
pub const CHECK_CIRCLE: &str = "\u{f26b}";
/// glyph: x-circle(停用/失败)
pub const X_CIRCLE: &str = "\u{f623}";
/// glyph: exclamation-circle(警示)
pub const EXCLAMATION_CIRCLE: &str = "\u{f333}";
/// glyph: info-circle(信息)
pub const INFO_CIRCLE: &str = "\u{f431}";
/// glyph: question-circle(未知/疑问)
pub const QUESTION_CIRCLE: &str = "\u{f505}";
/// glyph: shield-check(已验证签名/拦截引擎正常)
pub const SHIELD_CHECK: &str = "\u{f52f}";
/// glyph: shield-fill-x(拦截引擎失效)
pub const SHIELD_FILL_X: &str = "\u{f535}";
/// glyph: shield-exclamation(新连接询问)
pub const SHIELD_EXCLAMATION: &str = "\u{f530}";

// ---- 窗口控制(自绘标题栏) ----
/// glyph: dash-lg(最小化)
pub const DASH_LG: &str = "\u{f63b}";
/// glyph: square(最大化)
pub const SQUARE: &str = "\u{f584}";

// ---- 分组/功能标识 ----
/// glyph: translate(语言)
pub const TRANSLATE: &str = "\u{f658}";
/// glyph: palette(主题外观)
pub const PALETTE: &str = "\u{f4b1}";
/// glyph: moon(深色主题)
pub const MOON: &str = "\u{f497}";
/// glyph: brightness-high(浅色主题)
pub const BRIGHTNESS_HIGH: &str = "\u{f5a2}";
/// glyph: hdd(数据存储)
pub const HDD: &str = "\u{f412}";
/// glyph: database(历史数据库)
pub const DATABASE: &str = "\u{f8c4}";
/// glyph: bell(新连接询问)
pub const BELL: &str = "\u{f18a}";
/// glyph: terminal(日志)
pub const TERMINAL: &str = "\u{f5c3}";
/// glyph: pin(托盘)
pub const PIN: &str = "\u{f4ed}";
/// glyph: sliders(通用设置)
pub const SLIDERS: &str = "\u{f56b}";
/// glyph: clock(保留期/时间)
pub const CLOCK: &str = "\u{f293}";
/// glyph: ethernet(网络连接)
pub const ETHERNET: &str = "\u{f6d5}";
/// glyph: pc-display(本机)
pub const PC_DISPLAY: &str = "\u{f6a6}";
/// glyph: flask(模拟数据源)
pub const FLASK: &str = "\u{f90a}";
/// glyph: view-list(明细视图)
pub const VIEW_LIST: &str = "\u{f605}";
/// glyph: view-stacked(聚合视图)
pub const VIEW_STACKED: &str = "\u{f606}";
/// glyph: bar-chart-line(进程汇总视图)
pub const BAR_CHART_LINE: &str = "\u{f17c}";
/// glyph: graph-up(用量视图/局域网页)
pub const GRAPH_UP: &str = "\u{f3f2}";
/// glyph: calendar-week(用量按天)
pub const CALENDAR_WEEK: &str = "\u{f1f3}";
/// glyph: router(局域网设备)
pub const ROUTER: &str = "\u{f6ec}";
/// glyph: github(GitHub 仓库跳转)
pub const GITHUB: &str = "\u{f3ed}";
/// glyph: chat-dots(问题反馈跳转)
pub const CHAT_DOTS: &str = "\u{f24a}";
