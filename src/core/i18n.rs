use std::collections::HashMap;
use std::sync::OnceLock;

/// 支持的语言。解析失败 → None（调用方警告 + 回退 En）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language { En, Zh }

impl Language {
    pub fn from_str(s: &str) -> Option<Language> {
        match s.trim().to_ascii_lowercase().as_str() {
            "en" => Some(Language::En),
            "zh" => Some(Language::Zh),
            _ => None,
        }
    }

    pub fn code(&self) -> &'static str {
        match self { Language::En => "en", Language::Zh => "zh" }
    }
}

impl Default for Language {
    fn default() -> Self { Language::En }
}

/// 把 TOML 表结构展平为点分 key（`[runtime]` 段 → `runtime.history_weekly`）。
fn flatten(value: &toml::Value, prefix: &str, out: &mut HashMap<String, String>) {
    match value {
        toml::Value::Table(t) => {
            for (k, v) in t {
                let key = if prefix.is_empty() { k.clone() } else { format!("{}.{}", prefix, k) };
                flatten(v, &key, out);
            }
        }
        toml::Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        other => panic!("locale value for '{}' must be a string, got {:?}", prefix, other.type_str()),
    }
}

fn load_table(embedded: &'static str) -> HashMap<String, String> {
    let root: toml::Value = toml::from_str(embedded).expect("locale table must parse");
    let mut out = HashMap::new();
    flatten(&root, "", &mut out);
    out
}

fn tables() -> (&'static HashMap<String, String>, &'static HashMap<String, String>) {
    static TABLES: OnceLock<(HashMap<String, String>, HashMap<String, String>)> = OnceLock::new();
    let (en, zh) = TABLES.get_or_init(|| {
        let en: HashMap<String, String> = load_table(include_str!("../../locales/en.toml"));
        let zh: HashMap<String, String> = load_table(include_str!("../../locales/zh.toml"));
        (en, zh)
    });
    (en, zh)
}

/// 回退链核心（纯函数，可测）：当前语言 → en → key 本身。
pub fn lookup<'a>(
    en: &'a HashMap<String, String>,
    zh: &'a HashMap<String, String>,
    lang: Language,
    key: &'a str,
) -> &'a str {
    let table = match lang {
        Language::En => en,
        Language::Zh => zh,
    };
    table
        .get(key)
        .map(String::as_str)
        .or_else(|| en.get(key).map(String::as_str))
        .unwrap_or(key)
}

/// 查表返回模板；调用处 format! 拼接。key 必须为字面量（en 缺失时回退 key 本身）。
pub fn tr(lang: Language, key: &'static str) -> &'static str {
    let (en, zh) = tables();
    lookup(en, zh, lang, key)
}

/// 动态 key（`widget.<id>` 显示名）专用：id 恒为英文稳定标识，en 缺失时回退 id 原文。
pub fn tr_dyn(lang: Language, id: &str) -> std::borrow::Cow<'static, str> {
    let (en, zh) = tables();
    let key = format!("widget.{}", id);
    let primary = match lang {
        Language::En => en,
        Language::Zh => zh,
    };
    primary
        .get(&key)
        .map(String::as_str)
        .or_else(|| en.get(&key).map(String::as_str))
        .map(std::borrow::Cow::Borrowed)
        .unwrap_or_else(|| std::borrow::Cow::Owned(id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn from_str_parses_valid_and_rejects_invalid() {
        assert_eq!(Language::from_str("en"), Some(Language::En));
        assert_eq!(Language::from_str("zh"), Some(Language::Zh));
        assert_eq!(Language::from_str("ZH"), Some(Language::Zh)); // 大小写不敏感
        assert_eq!(Language::from_str("xx"), None);
        assert_eq!(Language::from_str(""), None);
    }

    #[test]
    fn lookup_falls_back_zh_to_en_then_key() {
        let en = table(&[("a", "Alpha"), ("b", "Beta")]);
        let zh = table(&[("a", "阿尔法")]);
        // zh 有 → 用 zh
        assert_eq!(lookup(&en, &zh, Language::Zh, "a"), "阿尔法");
        // zh 缺 → 回退 en
        assert_eq!(lookup(&en, &zh, Language::Zh, "b"), "Beta");
        // en 也缺 → key 本身
        assert_eq!(lookup(&en, &zh, Language::Zh, "c"), "c");
        assert_eq!(lookup(&en, &zh, Language::En, "c"), "c");
    }

    #[test]
    fn zh_keys_are_subset_of_en_keys() {
        let (en, zh) = tables();
        let extra: Vec<&String> = zh.keys().filter(|k| !en.contains_key(*k)).collect();
        assert!(extra.is_empty(), "zh 表含 en 没有的 key: {:?}", extra);
    }

    #[test]
    fn shared_keys_have_matching_placeholder_counts() {
        let (en, zh) = tables();
        let count = |s: &str| s.matches("{}").count();
        for (k, ev) in en.iter() {
            if let Some(zv) = zh.get(k) {
                assert_eq!(
                    count(ev), count(zv),
                    "key '{}' 的 {{}} 数量不一致: en={} zh={}", k, ev, zv
                );
            }
        }
    }

    #[test]
    fn tr_returns_en_value_for_default_language() {
        assert_eq!(tr(Language::En, "runtime.doctor_all_passed"), "All checks passed.");
        assert_eq!(tr(Language::Zh, "runtime.history_weekly"), "每周报告（最近 7 天）：");
    }

    #[test]
    fn tr_dyn_falls_back_to_id() {
        // 无 widget.* key 的 id（如脚本 widget）：回退 id 原文（英文稳定标识）
        assert_eq!(tr_dyn(Language::En, "script_rhai"), "script_rhai");
        assert_eq!(tr_dyn(Language::Zh, "script_rhai"), "script_rhai");
    }

    #[test]
    fn tr_dyn_resolves_widget_keys() {
        assert_eq!(tr_dyn(Language::En, "model_display"), "Model Display");
        assert_eq!(tr_dyn(Language::Zh, "model_display"), "模型显示");
    }
}
