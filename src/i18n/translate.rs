//! 词条翻译:查找、参数插值、默认语言回退与缺失告警

use super::{DEFAULT_LANG_CODE, I18n};
use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};

impl I18n {
    /// 翻译指定键(无参数)。UI 每帧数百次取词,结果缓存于 t_cache,
    /// 命中时免 fluent 解析与插值(即时模式渲染最大的每帧分配源);
    /// 带参数词条不缓存(插值结果随参数变化)
    pub fn t(&self, key: &str) -> String {
        if let Some(hit) = self.t_cache.borrow().get(key) {
            return hit.clone();
        }
        let text = self.translate(key, None);
        self.t_cache
            .borrow_mut()
            .insert(key.to_owned(), text.clone());
        text
    }

    /// 翻译指定键,并用 `args` 替换 FTL 文件中的 `{$name}` 占位符
    ///
    /// 注意:FTL 中变量必须带 `$` 前缀(`{$name}`);
    /// `{name}` 是消息引用而非变量,不会被插值
    pub fn t_with_args(&self, key: &str, args: &[(&str, String)]) -> String {
        let mut fluent_args = FluentArgs::new();
        for (name, value) in args {
            fluent_args.set(*name, value.as_str());
        }
        self.translate(key, Some(&fluent_args))
    }

    /// 查找词条:优先当前语言,缺失时回退默认语言,仍未命中则告警并返回键名
    fn translate(&self, key: &str, args: Option<&FluentArgs>) -> String {
        let mut parts = key.splitn(2, '.');
        let id_name = parts.next().unwrap_or_default();
        let attr_name = parts.next();

        let bundle = self.bundles.get(&self.current_lang);
        let default_bundle = self.bundles.get(DEFAULT_LANG_CODE);

        if let Some(text) = bundle.and_then(|b| Self::format_message(b, id_name, attr_name, args)) {
            return text;
        }
        if let Some(text) =
            default_bundle.and_then(|b| Self::format_message(b, id_name, attr_name, args))
        {
            return text;
        }

        self.warn_missing_key(key);
        key.to_string()
    }

    /// 在指定 bundle 中格式化词条,键采用 `消息ID.属性名` 形式,属性名可省略
    fn format_message(
        bundle: &FluentBundle<FluentResource>,
        id_name: &str,
        attr_name: Option<&str>,
        args: Option<&FluentArgs>,
    ) -> Option<String> {
        let msg = bundle.get_message(id_name)?;
        let pattern = match attr_name {
            Some(name) => msg.get_attribute(name).map(|attr| attr.value()),
            None => msg.value(),
        }?;
        let mut errors = vec![];
        Some(
            bundle
                .format_pattern(pattern, args, &mut errors)
                .to_string(),
        )
    }

    /// 记录缺失键告警(相同键仅告警一次)
    fn warn_missing_key(&self, key: &str) {
        if self.warned_keys.borrow_mut().insert(key.to_string()) {
            tracing::warn!(
                "[I18n] [key:{key}] 当前语言({})与默认语言({DEFAULT_LANG_CODE})均无此词条,回退显示键名",
                self.current_lang
            );
        }
    }
}
