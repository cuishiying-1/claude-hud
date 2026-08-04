use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use super::theme::{ResolvedTheme, Theme, ThemeRef};
use super::theme::apply_theme_keys;
use super::widget::WidgetConfig;

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

    #[serde(default = "default_currency_symbol")]
    pub currency_symbol: String,

    #[serde(default)]
    pub pricing: HashMap<String, crate::core::pricing::PriceEntry>,
}

fn default_active_mod() -> String {
    "glacier-workstation".into()
}

fn default_currency_symbol() -> String {
    "$".into()
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
}

fn default_ctx_critical() -> f64 { 95.0 }
fn default_cost_threshold() -> f64 { 10.0 }
fn default_rate_limit() -> f64 { 90.0 }
fn default_cooldown() -> u64 { 10 }

impl Default for AlertsConfig {
    fn default() -> Self {
        Self {
            context_critical_pct: 95.0,
            cost_threshold_usd: 10.0,
            rate_limit_pct: 90.0,
            cooldown_minutes: 10,
        }
    }
}

/// [budget] 预算告警：cap_usd（0=关闭）+ warn_pcts 渐进档位（每档一次）。
/// 冷却复用 [alerts].cooldown_minutes；预算基于 ≈ 实时估算成本触发。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BudgetConfig {
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
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content =
            fs::read_to_string(&path).map_err(|e| format!("read config: {}", e))?;
        toml::from_str(&content).map_err(|e| format!("parse config: {}", e))
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
        WidgetConfig { values }
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
            currency_symbol: "$".into(),
            pricing: HashMap::new(),
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
    }

    #[test]
    fn currency_symbol_and_pricing_defaults() {
        let c = AppConfig::default();
        assert_eq!(c.currency_symbol, "$");
        assert!(c.pricing.is_empty());
    }

    #[test]
    fn pricing_table_parses_with_field_defaults() {
        let toml_str = r#"
            currency_symbol = "¥"
            [pricing]
            "m1" = { input = 1e-6, output = 2e-6 }
        "#;
        let cfg: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.currency_symbol, "¥");
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
}
