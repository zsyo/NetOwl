//! 配置加载与写盘:启动加载(损坏备份 .bak 后回退默认)、写盘走
//! 临时文件 + .bak 备份 + rename 的原子替换;变更只更新内存,由 App
//! 层防抖合并调用 save_to_file。

use super::Config;
use crate::platform::paths;

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
        let content_len = full.len();
        let write_result = std::fs::write(&tmp, full).and_then(|_| {
            if std::fs::metadata(paths::CONFIG_FILE).is_ok()
                && let Err(e) =
                    std::fs::copy(paths::CONFIG_FILE, format!("{}.bak", paths::CONFIG_FILE))
            {
                tracing::debug!("[Config] 写盘前备份 .bak 失败: {e}");
            }
            // 首次写盘时原文件不存在,remove 的 NotFound 须容忍,否则会中断
            // 后面的 rename(现象:tmp 残留、config.toml 永远写不出来)
            match std::fs::remove_file(paths::CONFIG_FILE) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            std::fs::rename(&tmp, paths::CONFIG_FILE)
        });
        if let Err(e) = write_result {
            tracing::warn!("[Config] 配置文件写入失败: {e}");
        } else {
            tracing::debug!("[Config] 配置已写盘({content_len} 字节)");
        }
    }
}
