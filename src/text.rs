//! UI 文本常量(AGENTS.md 规范 4):绘制代码禁止散落字符串字面量,
//! 引入 i18n 后由此统一迁移。

// 应用与导航
pub const APP_NAME: &str = "NetOwl";
pub const APP_SUBTITLE: &str = "Network Monitor";
pub const NAV_MAP: &str = "流量地图";
pub const NAV_CONNECTIONS: &str = "连接";
pub const NAV_RULES: &str = "规则";
pub const NAV_SETTINGS: &str = "设置";

// 状态区
pub const STATUS_MONITORING: &str = "监控中";
pub const STATUS_MOCK: &str = "模拟数据";

// 地图页
pub const MAP_TITLE: &str = "流量地图";
pub const MAP_SUBTITLE: &str = "实时网络连接可视化";
pub const MAP_LEGEND_OUT: &str = "出站";
pub const MAP_LEGEND_IN: &str = "入站";
pub const MAP_LOCAL_LABEL: &str = "本机";

// 连接页
pub const CONNS_TITLE: &str = "连接";
pub const CONNS_SUBTITLE: &str = "当前活跃的网络连接";
pub const COL_PROCESS: &str = "进程";
pub const COL_PROTO: &str = "协议";
pub const COL_REMOTE: &str = "远端地址";
pub const COL_LOCATION: &str = "位置";
pub const COL_DOWN: &str = "下载";
pub const COL_UP: &str = "上传";
pub const CONNS_EMPTY: &str = "暂无活跃连接";

// 占位页
pub const RULES_TITLE: &str = "规则";
pub const RULES_PLACEHOLDER: &str = "规则引擎将在后续里程碑接入:允许/拒绝策略、进程与网段匹配、持久化。";
pub const SETTINGS_TITLE: &str = "设置";
pub const SETTINGS_PLACEHOLDER: &str = "设置页将在后续里程碑接入:启动项、主题、通知与数据采集方式。";

// 地图信息卡
/// 信息卡底部"其余连接"提示(带插值,故以函数形式提供)
pub fn info_more(n: usize) -> String {
    format!("+{n} 个连接")
}
