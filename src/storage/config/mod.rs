//! 应用配置模型(config.toml,serde TOML):结构定义与内存更新;
//! 加载与写盘在 io。窗口位置保存物理像素(跨多屏不同 DPI 无歧义),
//! i32::MIN 表示未设置(居中打开)。

mod io;

use serde::{Deserialize, Serialize};

/// 窗口位置未设置标记:使用系统默认位置(居中)
pub const WINDOW_POS_UNSET: i32 = i32::MIN;
/// 视为无效坐标的最小值:Windows 最小化时窗口会被移到 -32000,不得持久化
const INVALID_COORD: i32 = -8000;

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub floating_ball: FloatingBallConfig,
    #[serde(default)]
    pub window: WindowConfig,
}

/// 悬浮球配置;位置为窗口外框左上角逻辑点,坐标哨兵 = 未设置(默认右缘居中)
#[derive(Serialize, Deserialize, Debug)]
pub struct FloatingBallConfig {
    /// 悬浮球开关(默认关)
    #[serde(default)]
    pub enabled: bool,
    /// 贴边位置 x(逻辑点;i32::MIN = 未设置,右缘居中)
    #[serde(default = "default_pos")]
    pub x: i32,
    /// 贴边位置 y(逻辑点)
    #[serde(default = "default_pos")]
    pub y: i32,
    /// 窗口置顶显示(悬浮窗不被其他窗口遮挡)
    #[serde(default = "default_true")]
    pub always_on_top: bool,
    /// 贴边自动隐藏(鼠标离开后收缩为半隐窄条;关闭时常显)
    #[serde(default = "default_true")]
    pub auto_hide_edge: bool,
}

impl Default for FloatingBallConfig {
    fn default() -> Self {
        FloatingBallConfig {
            enabled: false,
            x: WINDOW_POS_UNSET,
            y: WINDOW_POS_UNSET,
            always_on_top: true,
            auto_hide_edge: true,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GeneralConfig {
    /// 界面语言代码(空表示未设置,跟随首次启动时的系统语言)
    #[serde(default)]
    pub language: String,
    /// 界面主题:"dark" / "light"
    #[serde(default)]
    pub theme: String,
    /// 连接数据源:"real"(真实采集)/ "mock"(模拟演示)
    #[serde(default = "default_collector")]
    pub collector: String,
    /// 历史数据自动清理天数(0 = 不自动清理)
    #[serde(default)]
    pub history_days: u32,
    /// 历史库超容提醒(false = 已勾选不再提醒;手动清空时还原为 true)
    #[serde(default = "default_true")]
    pub history_remind: bool,
    /// 连接页与历史页隐藏回环远端(127.0.0.0/8)
    #[serde(default = "default_true")]
    pub hide_local: bool,
    /// 连接页与历史页隐藏私网远端(RFC1918)
    #[serde(default = "default_true")]
    pub hide_lan: bool,
    /// 新连接询问弹窗(默认关闭):开启后未命中规则的公网新连接弹窗询问;
    /// 仅静默模式为 "off" 时生效
    #[serde(default)]
    pub ask_connections: bool,
    /// 全局静默模式(LS Silent Mode):"off" 询问模式(按 ask_connections)/
    /// "allow" 静默放行未命中连接 / "deny" 静默拒绝未命中连接
    #[serde(default)]
    pub silent_mode: String,
    /// 当前规则配置档 id(profiles 表,默认 1)
    #[serde(default = "default_profile")]
    pub profile_id: i64,
    /// 托盘图标常驻任务栏(NotifyIconSettings IsPromoted,免折叠进隐藏区);
    /// 写入失败(项未注册等)保持系统默认行为
    #[serde(default)]
    pub tray_pinned: bool,
    /// 开机自启动(HKCU Run 键):登录后静默启动到托盘,主窗口不弹出
    #[serde(default)]
    pub autostart: bool,
    /// 新设备接入提醒(局域网出现新 ARP 设备时右下角 toast 通知)
    #[serde(default = "default_true")]
    pub lan_notify: bool,
    /// 月度流量配额(GB,本地时区月界;0 = 不启用告警)
    #[serde(default)]
    pub usage_quota_gb: u32,
    /// 日志级别:off/error/warn/info/debug/trace(设置页可调,立即生效)
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// 文件日志(exe 同级 logs/latest.log,旧文件按时间戳轮转)
    #[serde(default)]
    pub log_to_file: bool,
    /// 检查更新渠道:"stable"(正式版,默认)/ "preview"(预览版,含
    /// pre-release;最新为正式版时同样覆盖)
    #[serde(default)]
    pub update_channel: String,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        GeneralConfig {
            language: String::new(),
            theme: String::new(),
            collector: default_collector(),
            history_days: 0,
            history_remind: true,
            hide_local: true,
            hide_lan: true,
            ask_connections: false,
            silent_mode: String::new(),
            profile_id: default_profile(),
            tray_pinned: false,
            autostart: false,
            lan_notify: true,
            usage_quota_gb: 0,
            log_level: default_log_level(),
            log_to_file: false,
            update_channel: String::new(),
        }
    }
}

fn default_collector() -> String {
    "real".to_owned()
}

fn default_profile() -> i64 {
    1
}

fn default_log_level() -> String {
    "info".to_owned()
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WindowConfig {
    /// 窗口外框左上角 x(物理像素)
    #[serde(default = "default_pos")]
    pub x: i32,
    /// 窗口外框左上角 y(物理像素)
    #[serde(default = "default_pos")]
    pub y: i32,
    /// 窗口内容宽度(物理像素)
    #[serde(default)]
    pub width: i32,
    /// 窗口内容高度(物理像素)
    #[serde(default)]
    pub height: i32,
    #[serde(default)]
    pub maximized: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        WindowConfig {
            x: WINDOW_POS_UNSET,
            y: WINDOW_POS_UNSET,
            width: 0,
            height: 0,
            maximized: false,
        }
    }
}

fn default_pos() -> i32 {
    WINDOW_POS_UNSET
}

impl Config {
    /// 窗口位置是否有效(未设置或被最小化污染时返回 None)
    pub fn window_position(&self) -> Option<(i32, i32, i32, i32)> {
        let w = &self.window;
        if w.x == WINDOW_POS_UNSET || w.y == WINDOW_POS_UNSET || w.width <= 0 || w.height <= 0 {
            return None;
        }
        Some((w.x, w.y, w.width, w.height))
    }

    /// 更新窗口几何;仅在内容变化时返回 true(避免无谓的写盘)
    pub fn set_window(&mut self, x: i32, y: i32, width: i32, height: i32, maximized: bool) -> bool {
        // 双双远超屏幕左侧 = 最小化污染(Windows 移窗到 -32000),不记录
        if x < INVALID_COORD && y < INVALID_COORD {
            return false;
        }
        let w = &mut self.window;
        if (w.x, w.y, w.width, w.height, w.maximized) != (x, y, width, height, maximized) {
            *w = WindowConfig {
                x,
                y,
                width,
                height,
                maximized,
            };
            true
        } else {
            false
        }
    }

    pub fn set_language(&mut self, lang: String) {
        self.general.language = lang;
    }

    /// 更新界面主题;仅在变化时返回 true(避免无谓的写盘)
    pub fn set_theme(&mut self, theme: String) -> bool {
        if self.general.theme != theme {
            self.general.theme = theme;
            true
        } else {
            false
        }
    }
}
