//! I18n 模块:基于 fluent-bundle 的多语言支持。
//! - mod.rs 负责 locales 目录扫描、语言包加载与语言列表管理
//! - translate.rs 负责词条查找、参数插值、缺失回退与告警
//!
//! 词条目录缺省时使用编译期内嵌的 zh-cn/en 兜底,发布产物无需携带 locales;
//! 放入外部 .ftl 可新增语言(设置页进入时重扫)。

mod translate;

use fluent_bundle::{FluentBundle, FluentResource};
use std::cell::RefCell;
use std::collections::HashSet;
use std::{collections::HashMap, fs, path::PathBuf};
use unic_langid::LanguageIdentifier;

/// 默认语言(词条缺失时的回退目标)
pub const DEFAULT_LANG_CODE: &str = "zh-cn";
const LOCALES_DIR_NAME: &str = "locales";
const FTL_EXTENSION: &str = "ftl";
const LANG_NAME_KEY: &str = "lang-name";

/// 内嵌兜底词条:locales 目录不可用或为空时保证界面可用
const EMBEDDED_ZH_CN: &str = include_str!("../../locales/zh-cn.ftl");
const EMBEDDED_EN: &str = include_str!("../../locales/en.ftl");

#[derive(Clone, Debug)]
pub struct LangInfo {
    pub code: String,
    pub name: String,
}

pub struct I18n {
    pub(crate) bundles: HashMap<String, FluentBundle<FluentResource>>,
    pub available_langs: Vec<LangInfo>,
    pub current_lang: String,
    /// 已告警过的缺失键,避免每帧渲染重复刷日志
    pub(crate) warned_keys: RefCell<HashSet<String>>,
}

impl Default for I18n {
    fn default() -> Self {
        Self::new()
    }
}

impl I18n {
    pub fn new() -> Self {
        let mut bundles = HashMap::new();
        let mut available_langs = Vec::new();

        if let Ok(entries) = fs::read_dir(Self::resolve_locales_dir()) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some(FTL_EXTENSION)
                    && let Some(lang_code) = path.file_stem().and_then(|s| s.to_str())
                {
                    let lang_code = lang_code.to_lowercase();
                    if let Ok(content) = fs::read_to_string(&path) {
                        Self::add_bundle(&mut bundles, &mut available_langs, &lang_code, &content);
                    }
                }
            }
        }

        // 目录不存在或没有任何词条:加载内嵌兜底
        if available_langs.is_empty() {
            Self::add_bundle(&mut bundles, &mut available_langs, "zh-cn", EMBEDDED_ZH_CN);
            Self::add_bundle(&mut bundles, &mut available_langs, "en", EMBEDDED_EN);
        }

        // 初始语言:环境变量覆盖(测试/开发用) -> 系统语言(取前两段) -> 默认语言 -> 列表首个
        let sys_lang = sys_locale::get_locale().unwrap_or_default().to_lowercase();
        let short_sys_lang = sys_lang.split('-').take(2).collect::<Vec<_>>().join("-");
        let mut current_lang = if Self::lang_code_exists(&available_langs, &short_sys_lang) {
            short_sys_lang
        } else if Self::lang_code_exists(&available_langs, DEFAULT_LANG_CODE) {
            DEFAULT_LANG_CODE.to_string()
        } else {
            available_langs
                .first()
                .map(|info| info.code.clone())
                .unwrap_or_else(|| DEFAULT_LANG_CODE.to_string())
        };
        if let Ok(env_lang) = std::env::var("NETOWL_LANG") {
            let env_lang = env_lang.to_lowercase();
            if Self::lang_code_exists(&available_langs, &env_lang) {
                current_lang = env_lang;
            }
        };

        I18n {
            bundles,
            available_langs,
            current_lang,
            warned_keys: RefCell::new(HashSet::new()),
        }
    }

    /// 解析 locales 目录:按候选顺序取第一个存在的目录。
    /// 候选覆盖 exe 同级(Windows 便携)与后续跨平台安装布局;开发模式回退项目根。
    fn resolve_locales_dir() -> PathBuf {
        Self::locales_dir_candidates()
            .into_iter()
            .find(|dir| dir.is_dir())
            .unwrap_or_else(|| PathBuf::from(LOCALES_DIR_NAME))
    }

    fn locales_dir_candidates() -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            candidates.push(dir.join(LOCALES_DIR_NAME));
            // 预留跨平台安装布局:Linux 系统安装(/usr/bin/../lib/netowl/locales)
            candidates.push(
                dir.join("..")
                    .join("lib")
                    .join("netowl")
                    .join(LOCALES_DIR_NAME),
            );
            // macOS .app Resources
            candidates.push(dir.join("..").join("Resources").join(LOCALES_DIR_NAME));
            candidates.push(dir.join("..").join(LOCALES_DIR_NAME));
            candidates.push(dir.join("..").join("..").join(LOCALES_DIR_NAME));
        }
        candidates.push(PathBuf::from(LOCALES_DIR_NAME));
        candidates
    }

    /// 重扫 locales 目录,加载运行期间新增的语言文件。
    /// 仅增量添加新语言,已加载语言不受文件删除影响。
    pub fn refresh_languages(&mut self) {
        let existing_count = self.available_langs.len();
        let Ok(entries) = fs::read_dir(Self::resolve_locales_dir()) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Some(lang_code) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let lang_code = lang_code.to_lowercase();
            if path.extension().and_then(|s| s.to_str()) != Some(FTL_EXTENSION)
                || self.bundles.contains_key(&lang_code)
            {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            Self::add_bundle(
                &mut self.bundles,
                &mut self.available_langs,
                &lang_code,
                &content,
            );
        }

        let added = self.available_langs.len() - existing_count;
        if added > 0 {
            tracing::info!("[I18n] [locales] 重扫完成,新增 {added} 个语言");
        }
    }

    pub(crate) fn lang_code_exists(langs: &[LangInfo], code: &str) -> bool {
        langs.iter().any(|info| info.code == code)
    }

    /// (语言代码, 语言显示名) 列表,供设置页下拉框
    pub fn lang_codes_and_names(&self) -> Vec<(String, String)> {
        self.available_langs
            .iter()
            .map(|info| (info.code.clone(), info.name.clone()))
            .collect()
    }

    pub fn set_language(&mut self, lang: String) {
        if self.bundles.contains_key(&lang) {
            self.current_lang = lang;
        }
    }

    fn add_bundle(
        bundles: &mut HashMap<String, FluentBundle<FluentResource>>,
        langs: &mut Vec<LangInfo>,
        code: &str,
        content: &str,
    ) {
        if let Ok(res) = FluentResource::try_new(content.to_string()) {
            let lang_id: LanguageIdentifier = code.parse().unwrap_or_default();
            let mut bundle = FluentBundle::new(vec![lang_id]);
            // 界面为拉丁/CJK 文本,无双向排版需求;关闭隔离避免插值两侧出现
            // FSI/PDI 控制字符(egui 字体无字形,显示为问号)
            bundle.set_use_isolating(false);
            if bundle.add_resource(res).is_ok() {
                let lang_name = Self::extract_lang_name(&bundle, code);
                bundles.insert(code.to_string(), bundle);
                if !Self::lang_code_exists(langs, code) {
                    langs.push(LangInfo {
                        code: code.to_string(),
                        name: lang_name,
                    });
                }
            }
        }
    }

    fn extract_lang_name(bundle: &FluentBundle<FluentResource>, code: &str) -> String {
        let mut errors = vec![];
        if let Some(msg) = bundle.get_message(LANG_NAME_KEY)
            && let Some(pattern) = msg.value()
        {
            return bundle
                .format_pattern(pattern, None, &mut errors)
                .to_string();
        }
        code.to_string()
    }
}
