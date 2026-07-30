use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use super::theme::Theme;
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
    pub theme: Option<Theme>,

    #[serde(default)]
    pub widgets: HashMap<String, toml::Value>,

    #[serde(default)]
    pub runtime_overrides: Option<RuntimeOverrides>,
}

fn default_active_mod() -> String {
    "glacier-workstation".into()
}

fn default_separator() -> String {
    " │ ".into()
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct DashboardConfig {
    #[serde(default = "default_refresh")]
    pub refresh_interval_ms: u64,
    #[serde(default = "default_dash_layout")]
    pub default_layout: String,
}

fn default_refresh() -> u64 { 500 }
fn default_dash_layout() -> String { "grid-2x2".into() }

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
                "alerts".into(),
            ],
            dashboard: DashboardConfig::default(),
            theme: None,
            widgets: HashMap::new(),
            runtime_overrides: None,
        }
    }
}
