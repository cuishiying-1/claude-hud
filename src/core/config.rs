use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use super::theme::{ResolvedTheme, Theme, ThemeRef};
use super::theme::apply_theme_keys;
use super::widget::WidgetConfig;
use crate::core::i18n::Language;

/// Top-level configuration loaded from config.toml.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default = "default_active_mod")]
    pub active_mod: String,

    #[serde(default)]
    pub preset: Option<String>,

    #[serde(default = "default_separator")]
    pub separator: String,

    #[serde(default)]
    pub compact_layout: Vec<String>,

    #[serde(default)]
    pub dashboard: DashboardConfig,

    #[serde(default)]
    pub theme: Option<ThemeRef>,

    #[serde(default)]
    pub widgets: HashMap<String, toml::Value>,

    #[serde(default)]
    pub runtime_overrides: Option<RuntimeOverrides>,

    #[serde(default = "default_alerts")]
    pub alerts: AlertsConfig,

    #[serde(default)]
    pub budget: BudgetConfig,

    #[serde(default)]
    pub currency_symbol: Option<String>,

    #[serde(default = "default_language")]
    pub language: String,

    #[serde(default)]
    pub pricing: HashMap<String, crate::core::pricing::PriceEntry>,

    #[serde(default)]
    pub models: HashMap<String, crate::core::pricing::ModelEntry>,
}

fn default_active_mod() -> String {
    "glacier-workstation".into()
}

fn default_language() -> String {
    "en".into()
}

fn default_separator() -> String {
    " │ ".into()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DashboardConfig {
    #[serde(default = "default_refresh")]
    pub refresh_interval_ms: u64,
    #[serde(default = "default_dash_layout")]
    pub default_layout: String,
    #[serde(default = "default_scanlines")]
    pub scanlines: bool,
}

fn default_refresh() -> u64 { 500 }
fn default_dash_layout() -> String { "grid-2x2".into() }
fn default_scanlines() -> bool { true }

// 手动 Default：使 Rust 默认（setup/mod reset 写出的 config.toml）与 serde 默认一致，
// 并修正此前 Rust 默认 refresh=0 的潜在忙轮询（DEPLOY.md 文档值即 500）。
impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            refresh_interval_ms: 500,
            default_layout: "grid-2x2".into(),
            scanlines: true,
        }
    }
}

/// [alerts] section: thresholds (0 = disabled) and cooldown window.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AlertsConfig {
    #[serde(default = "default_ctx_critical")]
    pub context_critical_pct: f64,
    #[serde(default = "default_cost_threshold")]
    pub cost_threshold_usd: f64,
    #[serde(default = "default_rate_limit")]
    pub rate_limit_pct: f64,
    #[serde(default = "default_cooldown")]
    pub cooldown_minutes: u64,
    /// ④ 压缩临近通知阈值（分钟；0 = 关闭，默认 15）。
    #[serde(default = "default_compaction_eta")]
    pub compaction_eta_minutes: u64,
}

fn default_ctx_critical() -> f64 { 95.0 }
fn default_cost_threshold() -> f64 { 10.0 }
fn default_rate_limit() -> f64 { 90.0 }
fn default_cooldown() -> u64 { 10 }
fn default_compaction_eta() -> u64 { 15 }

impl Default for AlertsConfig {
    fn default() -> Self {
        Self {
            context_critical_pct: 95.0,
            cost_threshold_usd: 10.0,
            rate_limit_pct: 90.0,
            cooldown_minutes: 10,
            compaction_eta_minutes: 15,
        }
    }
}

/// [budget] 预算告警：cap_usd（0=关闭）+ warn_pcts 渐进档位（每档一次）。
/// 冷却复用 [alerts].cooldown_minutes；预算基于 ≈ 实时估算成本触发。
/// 口径（用户拍板 2026-08-05）：成本/上限/百分比统一语言选币种——
/// cap_usd 即显示币种数值（zh 用户写 10 就是 10 元，en 即 10 美元），
/// 百分比 = 语言币种成本 / 上限，无任何 USD 汇率换算。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BudgetConfig {
    /// 预算上限（显示币种数值：语言选币种，zh 即人民币、en 即美元）。
    /// 字段名保留历史兼容（usd 后缀不代表币种换算）。
    #[serde(default = "default_budget_cap")]
    pub cap_usd: f64,
    #[serde(default = "default_budget_warn_pcts")]
    pub warn_pcts: Vec<f64>,
}

fn default_budget_cap() -> f64 { 0.0 }
fn default_budget_warn_pcts() -> Vec<f64> { vec![50.0, 80.0, 100.0] }

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            cap_usd: 0.0,
            warn_pcts: vec![50.0, 80.0, 100.0],
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct RuntimeOverrides {
    pub compact_lines: Option<u8>,
    #[serde(default)]
    pub animation: Option<AnimationOverrides>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct AnimationOverrides {
    pub enabled: Option<bool>,
}

/// Mod package stored as a .toml file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModPackage {
    #[serde(default)]
    pub mod_info: ModInfo,
    pub layout: Option<ModLayout>,
    /// 保存时的 compact widget 数组快照（布局 ID 之外的完整保留）。
    #[serde(default)]
    pub compact_widgets: Option<Vec<String>>,
    pub theme: Option<ModTheme>,
    #[serde(default)]
    pub animation: Option<ModAnimation>,
    #[serde(default)]
    pub widgets: HashMap<String, toml::Value>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct ModInfo {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub scene: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModLayout {
    pub compact: String,
    pub dashboard: String,
    #[serde(default = "default_two")]
    pub compact_lines: u8,
}

fn default_two() -> u8 { 2 }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModTheme {
    pub preset: String,
    #[serde(default)]
    pub overrides: Option<HashMap<String, toml::Value>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModAnimation {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub effects: Vec<String>,
}

fn default_true() -> bool { true }

impl AppConfig {
    /// Load config from the standard location.
    pub fn load() -> Result<Self, String> {
        Self::load_from_path(&Self::config_path()?)
    }

    /// 从任意路径加载（测试注入 temp 路径；默认路径见 load()）。
    pub fn load_from_path(path: &std::path::Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content =
            fs::read_to_string(path).map_err(|e| format!("read config: {}", e))?;
        toml::from_str(&content).map_err(|e| format!("parse config: {}", e))
    }

    /// 重建式保存：全量校验 → 备份 config.toml.bak → 写 tmp → 原子替换。
    /// 丢失手写注释（用户已确认接受）；path 参数化便于测试。
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        crate::core::config_schema::validate_config(self)?;
        if path.exists() {
            let bak = path.with_extension("toml.bak");
            fs::copy(path, &bak).map_err(|e| format!("backup config: {}", e))?;
        }
        let content =
            toml::to_string(self).map_err(|e| format!("serialize config: {}", e))?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, content).map_err(|e| format!("write temp: {}", e))?;
        if path.exists() {
            fs::remove_file(path).map_err(|e| format!("remove old: {}", e))?;
        }
        fs::rename(&tmp, path).map_err(|e| format!("replace config: {}", e))?;
        Ok(())
    }

    /// Load a Mod package by name.
    pub fn load_mod(name: &str) -> Result<ModPackage, String> {
        // First check built-in presets
        if let Some(data) = Self::load_builtin_mod(name) {
            return Ok(data);
        }
        // Then check user mods directory
        let mods_dir = Self::mods_dir()?;
        let path = mods_dir.join(format!("{}.toml", name));
        if !path.exists() {
            return Err(format!("mod '{}' not found", name));
        }
        let content = fs::read_to_string(&path).map_err(|e| format!("read mod: {}", e))?;
        toml::from_str(&content).map_err(|e| format!("parse mod: {}", e))
    }

    fn load_builtin_mod(name: &str) -> Option<ModPackage> {
        let data = match name {
            "glacier-workstation" => include_str!("../presets/glacier-workstation.toml"),
            "obsidian-command" => include_str!("../presets/obsidian-command.toml"),
            "ember-night" => include_str!("../presets/ember-night.toml"),
            "matrix-surveillance" => include_str!("../presets/matrix-surveillance.toml"),
            "noir-precision" => include_str!("../presets/noir-precision.toml"),
            "noir-tabbed" => include_str!("../presets/noir-tabbed.toml"),
            _ => return None,
        };
        toml::from_str(data).ok()
    }

    pub fn config_path() -> Result<PathBuf, String> {
        // CLAUDE_HUD_CONFIG 优先：黑盒测试注入临时语言配置
        // （与 COLUMNS / CLAUDE_HUD_PHASE 的 env 注入先例一致）
        if let Ok(p) = std::env::var("CLAUDE_HUD_CONFIG") {
            return Ok(PathBuf::from(p));
        }
        let base = dirs::home_dir()
            .ok_or_else(|| "cannot find home directory".to_string())?;
        Ok(base.join(".claude").join("plugins").join("claude-hud").join("config.toml"))
    }

    pub fn mods_dir() -> Result<PathBuf, String> {
        let base = dirs::home_dir()
            .ok_or_else(|| "cannot find home directory".to_string())?;
        Ok(base.join(".claude").join("plugins").join("claude-hud").join("mods"))
    }

    pub fn state_path() -> Result<PathBuf, String> {
        let base = dirs::home_dir()
            .ok_or_else(|| "cannot find home directory".to_string())?;
        Ok(base.join(".claude").join("plugins").join("claude-hud").join("state.json"))
    }

    /// 多窗口实时快照目录:每窗口一个 <key>.json(key = transcript_path 哈希)。
    pub fn windows_dir() -> Result<PathBuf, String> {
        let base = dirs::home_dir()
            .ok_or_else(|| "cannot find home directory".to_string())?;
        Ok(base.join(".claude").join("plugins").join("claude-hud").join("windows"))
    }

    /// Build WidgetConfig for a given widget id from the config.
    pub fn widget_config(&self, id: &str) -> WidgetConfig {
        let mut values = HashMap::new();
        if let Some(toml::Value::Table(table)) = self.widgets.get(id) {
            for (k, v) in table {
                let s = match v {
                    toml::Value::String(s) => s.clone(),
                    toml::Value::Integer(i) => i.to_string(),
                    toml::Value::Float(f) => f.to_string(),
                    toml::Value::Boolean(b) => b.to_string(),
                    _ => continue,
                };
                values.insert(k.clone(), s);
            }
        }
        WidgetConfig {
            values,
            lang: self.language(),
            context_bar_present: self
                .compact_layout
                .iter()
                .any(|w| w == "context_bar"),
        }
    }

    /// 解析 language 键为 Language；非法值回退 En（警告在 main/doctor 入口各一次）。
    pub fn language(&self) -> crate::core::i18n::Language {
        crate::core::i18n::Language::from_str(&self.language)
            .unwrap_or(crate::core::i18n::Language::En)
    }

    /// 币种符号决议：显式配置 → zh 语言 ¥ → 其他 $。
    pub fn currency(&self) -> &str {
        if let Some(s) = self.currency_symbol.as_deref() {
            return s;
        }
        match self.language() {
            Language::Zh => "¥",
            _ => "$",
        }
    }

    /// 主题叠加链：基底(mod preset 或 config preset 或 default) →
    /// config.theme 显式键 → config.theme.overrides → mod.theme.overrides。
    pub fn resolve_theme(&self) -> ResolvedTheme {
        let mut preset_name: Option<String> = None;
        let mut base = Theme::default();
        if !self.active_mod.is_empty() {
            if let Ok(pkg) = Self::load_mod(&self.active_mod) {
                if let Some(mt) = &pkg.theme {
                    if let Some(t) = Theme::load_preset(&mt.preset) {
                        base = t;
                        preset_name = Some(mt.preset.clone());
                    }
                }
            }
        }
        if let Some(tr) = &self.theme {
            match tr {
                ThemeRef::Preset(p) => {
                    if preset_name.is_none() {
                        if let Some(t) = Theme::load_preset(p) {
                            base = t;
                            preset_name = Some(p.clone());
                        }
                    }
                }
                ThemeRef::Table(tbl) => {
                    if preset_name.is_none() {
                        if let Some(p) = &tbl.preset {
                            if let Some(t) = Theme::load_preset(p) {
                                base = t;
                                preset_name = Some(p.clone());
                            }
                        }
                    }
                    apply_theme_keys(&mut base, &tbl.colors);
                    if let Some(ov) = &tbl.overrides {
                        apply_theme_keys(&mut base, ov);
                    }
                }
            }
        }
        if !self.active_mod.is_empty() {
            if let Ok(pkg) = Self::load_mod(&self.active_mod) {
                if let Some(mt) = &pkg.theme {
                    if let Some(ov) = &mt.overrides {
                        apply_theme_keys(&mut base, ov);
                    }
                }
            }
        }
        ResolvedTheme { preset: preset_name, theme: base }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            active_mod: "glacier-workstation".into(),
            preset: Some("full".into()),
            separator: " │ ".into(),
            compact_layout: vec![
                "model_display".into(),
                "context_bar".into(),
                "agent_overview".into(),
                "cost_display".into(),
                "skills_mcp".into(),
                "token_rate".into(),
                "alerts".into(),
            ],
            dashboard: DashboardConfig::default(),
            theme: None,
            widgets: HashMap::new(),
            runtime_overrides: None,
            alerts: AlertsConfig::default(),
            budget: BudgetConfig::default(),
            currency_symbol: None,
            language: "en".into(),
            pricing: HashMap::new(),
            models: HashMap::new(),
        }
    }
}

fn default_alerts() -> AlertsConfig {
    AlertsConfig::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alerts_defaults() {
        let a = AlertsConfig::default();
        assert_eq!(a.context_critical_pct, 95.0);
        assert_eq!(a.cost_threshold_usd, 10.0);
        assert_eq!(a.rate_limit_pct, 90.0);
        assert_eq!(a.cooldown_minutes, 10);
        assert_eq!(a.compaction_eta_minutes, 15);
    }

    #[test]
    fn currency_symbol_and_pricing_defaults() {
        let c = AppConfig::default();
        assert!(c.currency_symbol.is_none(), "default currency_symbol is None");
        assert_eq!(c.currency(), "$");
        assert!(c.pricing.is_empty());
    }

    #[test]
    fn models_section_parsed_with_dual_currency() {
        let toml_str = r#"
            [models."deepseek-v4-flash"]
            context_window = 1000000
            synced_at = "2026-08-05T12:00:00Z"
            [models."deepseek-v4-flash".price_usd]
            input = 0.14e-6
            output = 0.28e-6
            cache_read = 0.0028e-6
            cache_creation = 0.175e-6
            [models."deepseek-v4-flash".price_cny]
            input = 1.0e-6
            output = 2.0e-6
            cache_read = 0.02e-6
            cache_creation = 1.25e-6
            [pricing."my-model"]
            input = 1.0e-6
            output = 2.0e-6
        "#;
        let cfg: AppConfig = toml::from_str(toml_str).unwrap();
        let entry = cfg.models.get("deepseek-v4-flash").expect("model entry parsed");
        assert_eq!(entry.context_window, Some(1_000_000));
        assert_eq!(entry.synced_at.as_deref(), Some("2026-08-05T12:00:00Z"));
        let usd = entry.price_usd.as_ref().expect("usd price");
        assert!((usd.input - 0.14e-6).abs() < 1e-15);
        let cny = entry.price_cny.as_ref().expect("cny price");
        assert!((cny.output - 2.0e-6).abs() < 1e-15);
        // [pricing] 与 [models] 并存
        assert!(cfg.pricing.contains_key("my-model"));
    }

    #[test]
    fn currency_resolution_explicit_zh_default_en() {
        let mut zh = AppConfig::default();
        zh.language = "zh".into();
        assert_eq!(zh.currency(), "¥", "zh without explicit symbol");
        let mut explicit = AppConfig::default();
        explicit.currency_symbol = Some("€".into());
        assert_eq!(explicit.currency(), "€", "explicit wins");
        let mut zh_explicit = AppConfig::default();
        zh_explicit.language = "zh".into();
        zh_explicit.currency_symbol = Some("$".into());
        assert_eq!(zh_explicit.currency(), "$", "explicit beats zh default");
        assert_eq!(AppConfig::default().currency(), "$", "en default");
    }

    #[test]
    fn language_field_defaults_and_parses() {
        let c = AppConfig::default();
        assert_eq!(c.language, "en");
        assert_eq!(c.language(), crate::core::i18n::Language::En);

        let zh: AppConfig = toml::from_str("language = \"zh\"\n").unwrap();
        assert_eq!(zh.language(), crate::core::i18n::Language::Zh);

        let bad: AppConfig = toml::from_str("language = \"xx\"\n").unwrap();
        assert_eq!(bad.language(), crate::core::i18n::Language::En); // 静默回退（警告在 main 启动处）
    }

    #[test]
    fn widget_config_injects_language() {
        let cfg: AppConfig = toml::from_str("language = \"zh\"\n").unwrap();
        let wc = cfg.widget_config("model_display");
        assert_eq!(wc.lang, crate::core::i18n::Language::Zh);
        let wc2 = AppConfig::default().widget_config("model_display");
        assert_eq!(wc2.lang, crate::core::i18n::Language::En);
    }

    #[test]
    fn widget_config_injects_context_bar_presence() {
        let cfg: AppConfig = toml::from_str(
            "compact_layout = [\"model_display\", \"context_bar\", \"cost_display\"]\n",
        )
        .unwrap();
        assert!(cfg.widget_config("cost_display").context_bar_present);
        let minimal: AppConfig =
            toml::from_str("compact_layout = [\"model_display\", \"cost_display\"]\n").unwrap();
        assert!(!minimal.widget_config("cost_display").context_bar_present);
        // 默认布局含 context_bar
        assert!(AppConfig::default().widget_config("cost_display").context_bar_present);
    }

    #[test]
    fn pricing_table_parses_with_field_defaults() {
        let toml_str = r#"
            currency_symbol = "¥"
            [pricing]
            "m1" = { input = 1e-6, output = 2e-6 }
        "#;
        let cfg: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.currency_symbol.as_deref(), Some("¥"));
        let p = cfg.pricing.get("m1").expect("model price parsed");
        assert_eq!(p.input, 1e-6);
        assert_eq!(p.output, 2e-6);
        assert_eq!(p.cache_read, 0.0); // 缺省按 0
        assert_eq!(p.cache_creation, 0.0);
    }

    #[test]
    fn merge_theme_layers_order() {
        // 基底 dracula → config 键层 accent → config overrides accent →
        // mod overrides accent：后者胜出
        let base = Theme::load_preset("dracula").unwrap();
        let config_keys: HashMap<String, toml::Value> =
            toml::from_str("accent = \"#111111\"\n").unwrap();
        let config_ov: HashMap<String, toml::Value> =
            toml::from_str("accent = \"#222222\"\n").unwrap();
        let mod_ov: HashMap<String, toml::Value> =
            toml::from_str("accent = \"#333333\"\n").unwrap();
        let mut merged = base;
        apply_theme_keys(&mut merged, &config_keys);
        apply_theme_keys(&mut merged, &config_ov);
        apply_theme_keys(&mut merged, &mod_ov);
        assert_eq!(merged.accent, "#333333");
        assert_eq!(merged.bg, "#282a36"); // dracula 底色保留
    }

    #[test]
    fn resolve_theme_string_preset_without_mod() {
        let cfg: AppConfig = toml::from_str(
            "active_mod = \"\"\ntheme = \"dracula\"\n",
        ).unwrap();
        let r = cfg.resolve_theme();
        assert_eq!(r.preset.as_deref(), Some("dracula"));
        assert_eq!(r.theme.bg, "#282a36");
    }

    #[test]
    fn resolve_theme_partial_table_overrides_default() {
        let cfg: AppConfig = toml::from_str(
            "active_mod = \"\"\n[theme]\naccent = \"#ff0000\"\n",
        ).unwrap();
        let r = cfg.resolve_theme();
        assert_eq!(r.theme.accent, "#ff0000");
        assert_eq!(r.theme.bg, "#2e3440"); // 其余为 default nord
    }

    #[test]
    fn resolve_theme_preset_and_overrides_table() {
        let cfg: AppConfig = toml::from_str(
            "active_mod = \"\"\n[theme]\npreset = \"dracula\"\n[theme.overrides]\naccent = \"#ff0000\"\n",
        ).unwrap();
        let r = cfg.resolve_theme();
        assert_eq!(r.preset.as_deref(), Some("dracula"));
        assert_eq!(r.theme.bg, "#282a36");
        assert_eq!(r.theme.accent, "#ff0000");
    }

    #[test]
    fn config_path_env_override() {
        let tmp = std::env::temp_dir().join("hud-i18n-test-config.toml");
        std::env::set_var("CLAUDE_HUD_CONFIG", &tmp);
        let p = AppConfig::config_path().expect("config path resolves");
        assert_eq!(p, tmp);
        std::env::remove_var("CLAUDE_HUD_CONFIG");
        let p2 = AppConfig::config_path().expect("config path resolves");
        assert_ne!(p2, tmp);
    }

    #[test]
    fn windows_dir_under_hud_dir() {
        let dir = AppConfig::windows_dir().unwrap();
        let s = dir.to_string_lossy().to_string();
        assert!(
            s.ends_with("claude-hud\\windows") || s.ends_with("claude-hud/windows"),
            "{}",
            s
        );
    }

    #[test]
    fn save_round_trips_to_disk() {
        let dir = std::env::temp_dir().join(format!("hud-cfg-{}-rt", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let mut c = AppConfig::default();
        c.language = "zh".into();
        c.save(&path).unwrap();
        let loaded = AppConfig::load_from_path(&path).unwrap();
        assert_eq!(loaded.language, "zh");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_creates_backup_of_original() {
        let dir = std::env::temp_dir().join(format!("hud-cfg-{}-bak", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "language = \"en\"\n").unwrap();
        let mut c = AppConfig::default();
        c.language = "zh".into();
        c.save(&path).unwrap();
        let bak = std::fs::read_to_string(dir.join("config.toml.bak")).unwrap();
        assert!(bak.contains("language = \"en\""), "bak = {bak}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_rejects_invalid_config_without_touching_file() {
        let dir = std::env::temp_dir().join(format!("hud-cfg-{}-inv", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "language = \"en\"\n").unwrap();
        let mut c = AppConfig::default();
        c.language = "xx".into();
        assert!(c.save(&path).is_err());
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("language = \"en\""));
        assert!(!dir.join("config.toml.bak").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
