//! 应用配置(config.toml,serde TOML)。
//!
//! 参照 wallwarp 模式:启动加载(损坏备份 .bak 后回退默认)、变更只更新内存,
//! 由 App 层防抖合并写盘;写盘走 临时文件 + .bak 备份 + rename 的原子替换。
//! 窗口位置保存物理像素(跨多屏不同 DPI 无歧义),i32::MIN 表示未设置(居中打开)。

use serde::{Deserialize, Serialize};

use crate::paths;

/// 窗口位置未设置标记:使用系统默认位置(居中)
pub const WINDOW_POS_UNSET: i32 = i32::MIN;
/// 视为无效坐标的最小值:Windows 最小化时窗口会被移到 -32000,不得持久化
const INVALID_COORD: i32 = -8000;

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub window: WindowConfig,
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
}

impl Default for GeneralConfig {
    fn default() -> Self {
        GeneralConfig {
            language: String::new(),
            theme: String::new(),
            collector: default_collector(),
        }
    }
}

fn default_collector() -> String {
    "real".to_owned()
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
                    return cfg;
                }
                Err(e) => {
                    eprintln!(
                        "[Config] 配置文件解析失败: {e},将备份为 {}.bak 并使用默认配置",
                        paths::CONFIG_FILE
                    );
                    let _ = std::fs::rename(
                        paths::CONFIG_FILE,
                        format!("{}.bak", paths::CONFIG_FILE),
                    );
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
        if (w.x, w.y, w.width, w.height, w.maximized)
            != (x, y, width, height, maximized)
        {
            *w = WindowConfig { x, y, width, height, maximized };
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
                eprintln!("[Config] TOML 序列化失败: {e}");
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
            if std::fs::metadata(paths::CONFIG_FILE).is_ok() {
                let _ = std::fs::copy(paths::CONFIG_FILE, format!("{}.bak", paths::CONFIG_FILE));
                std::fs::remove_file(paths::CONFIG_FILE)?;
            }
            std::fs::rename(&tmp, paths::CONFIG_FILE)
        });
        if let Err(e) = write_result {
            eprintln!("[Config] 配置文件写入失败: {e}");
        }
    }
}
