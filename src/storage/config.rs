//! 应用配置(config.toml,serde TOML)。
//!
//! 参照 wallwarp 模式:启动加载(损坏备份 .bak 后回退默认)、变更只更新内存,
//! 由 App 层防抖合并写盘;写盘走 临时文件 + .bak 备份 + rename 的原子替换。
//! 窗口位置保存物理像素(跨多屏不同 DPI 无歧义),i32::MIN 表示未设置(居中打开)。

use serde::{Deserialize, Serialize};

use crate::platform::paths;

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
    /// 日志级别:off/error/warn/info/debug/trace(设置页可调,立即生效)
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// 文件日志(exe 同级 logs/latest.log,旧文件按时间戳轮转)
    #[serde(default)]
    pub log_to_file: bool,
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
            log_level: default_log_level(),
            log_to_file: false,
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
    /// 加载配置;文件缺失或损坏时以默认配置创建(损坏文件备份为 .bak)。
    /// 语言不在可用列表时重置为 `fallback_lang`。
    pub fn load(fallback_lang: &str, available_langs: &[String]) -> Self {
        if let Ok(content) = std::fs::read_to_string(paths::CONFIG_FILE) {
            match toml::from_str::<Config>(&content) {
                Ok(mut cfg) => {
                    if !available_langs.contains(&cfg.general.language) {
                        cfg.general.language = fallback_lang.to_owned();
                    }
                    if cfg.general.theme != "light" && cfg.general.theme != "dark" {
                        cfg.general.theme = "dark".to_owned();
                    }
                    if cfg.general.collector != "real" && cfg.general.collector != "mock" {
                        cfg.general.collector = "real".to_owned();
                    }
                    if !matches!(cfg.general.silent_mode.as_str(), "off" | "allow" | "deny") {
                        cfg.general.silent_mode = "off".to_owned();
                    }
                    if crate::logging::LogLevel::parse(&cfg.general.log_level).as_str()
                        != cfg.general.log_level
                    {
                        cfg.general.log_level = "info".to_owned();
                    }
                    return cfg;
                }
                Err(e) => {
                    tracing::warn!(
                        "[Config] 配置文件解析失败: {e},将备份为 {}.bak 并使用默认配置",
                        paths::CONFIG_FILE
                    );
                    if let Err(e) =
                        std::fs::rename(paths::CONFIG_FILE, format!("{}.bak", paths::CONFIG_FILE))
                    {
                        tracing::debug!("[Config] 备份损坏配置失败: {e}");
                    }
                }
            }
        }
        let mut cfg = Config::default();
        cfg.general.language = fallback_lang.to_owned();
        cfg.save_to_file();
        cfg
    }

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

    /// 写盘:toml + 警告头,经 临时文件 -> .bak 备份 -> rename 原子替换
    pub fn save_to_file(&self) {
        let content = match toml::to_string_pretty(self) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("[Config] TOML 序列化失败: {e}");
                return;
            }
        };
        let full = format!(
            "# ====================================================\n\
             # 警告:手动修改此配置文件时请务必谨慎!\n\
             # 如果格式填写错误,该项可能会被重置为默认值,甚至导致程序无法启动。\n\
             # 建议在修改前备份此文件。\n\
             # ====================================================\n\n{content}"
        );
        let tmp = format!("{}.tmp", paths::CONFIG_FILE);
        let write_result = std::fs::write(&tmp, full).and_then(|_| {
            if std::fs::metadata(paths::CONFIG_FILE).is_ok()
                && let Err(e) =
                    std::fs::copy(paths::CONFIG_FILE, format!("{}.bak", paths::CONFIG_FILE))
            {
                tracing::debug!("[Config] 写盘前备份 .bak 失败: {e}");
            }
            std::fs::remove_file(paths::CONFIG_FILE)?;
            std::fs::rename(&tmp, paths::CONFIG_FILE)
        });
        if let Err(e) = write_result {
            tracing::warn!("[Config] 配置文件写入失败: {e}");
        }
    }
}
