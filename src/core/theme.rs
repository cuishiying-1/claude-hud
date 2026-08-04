use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 主题引用：字符串预设名或 [theme] 表（部分/完整/preset+overrides 统一走
/// Table 形态）。untagged 按声明顺序尝试，字符串与表天然互斥，无歧义。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ThemeRef {
    /// theme = "dracula"
    Preset(String),
    /// [theme] ...（部分表/完整表/preset+overrides 均为此形态）
    Table(ThemeTable),
}

/// [theme] 表：preset 引用 + overrides 微调 + flatten 捕获的显式主题键。
/// flatten 是叠加合并正确性的关键——「哪些键被显式写出」可检测。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ThemeTable {
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub overrides: Option<HashMap<String, toml::Value>>,
    #[serde(flatten)]
    pub colors: HashMap<String, toml::Value>,
}

/// Complete theme definition (20 tokens).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Theme {
    // Color tokens (11)
    #[serde(default = "default_bg")]
    pub bg: String,
    #[serde(default = "default_fg")]
    pub fg: String,
    #[serde(default = "default_accent")]
    pub accent: String,
    #[serde(default = "default_success")]
    pub success: String,
    #[serde(default = "default_warning")]
    pub warning: String,
    #[serde(default = "default_danger")]
    pub danger: String,
    #[serde(default = "default_muted")]
    pub muted: String,
    #[serde(default = "default_border")]
    pub border: String,
    #[serde(default = "default_skill_color")]
    pub skill_color: String,
    #[serde(default = "default_mcp_color")]
    pub mcp_color: String,
    #[serde(default = "default_model_color")]
    pub model_color: String,

    // Style tokens (9)
    #[serde(default = "default_bar_filled")]
    pub bar_filled: char,
    #[serde(default = "default_bar_empty")]
    pub bar_empty: char,
    #[serde(default = "default_separator")]
    pub separator: String,
    #[serde(default = "default_border_style")]
    pub border_style: BorderStyle,
    #[serde(default = "default_icon_set")]
    pub icon_set: IconSet,
    #[serde(default = "default_bar_width")]
    pub bar_width: u16,
    #[serde(default = "default_padding")]
    pub padding: u16,
    #[serde(default = "default_compact_lines")]
    pub compact_lines: u8,
    #[serde(default = "default_dashboard_grid")]
    pub dashboard_grid: u8,
}

fn default_bg() -> String { "#2e3440".into() }
fn default_fg() -> String { "#d8dee9".into() }
fn default_accent() -> String { "#88c0d0".into() }
fn default_success() -> String { "#a3be8c".into() }
fn default_warning() -> String { "#ebcb8b".into() }
fn default_danger() -> String { "#bf616a".into() }
fn default_muted() -> String { "#5e81ac".into() }
fn default_border() -> String { "#434c5e".into() }
fn default_skill_color() -> String { "#b48ead".into() }
fn default_mcp_color() -> String { "#d08770".into() }
fn default_model_color() -> String { "#88c0d0".into() }

fn default_bar_filled() -> char { '█' }
fn default_bar_empty() -> char { '░' }
fn default_separator() -> String { " │ ".into() }
fn default_border_style() -> BorderStyle { BorderStyle::Rounded }
fn default_icon_set() -> IconSet { IconSet::Auto }
fn default_bar_width() -> u16 { 16 }
fn default_padding() -> u16 { 1 }
fn default_compact_lines() -> u8 { 2 }
fn default_dashboard_grid() -> u8 { 2 }

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BorderStyle {
    Single,
    Double,
    Rounded,
    Thick,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IconSet {
    Auto,
    Nerd,
    Ascii,
    Minimal,
}

impl Theme {
    /// Return the 6 built-in preset names.
    pub fn preset_names() -> &'static [&'static str] {
        &[
            "dracula",
            "nord",
            "tokyo-night",
            "catppuccin",
            "monochrome",
            "solarized-dark",
        ]
    }

    /// Load a built-in preset by name.
    pub fn load_preset(name: &str) -> Option<Self> {
        match name {
            "dracula" => Some(Self::dracula()),
            "nord" => Some(Self::nord()),
            "tokyo-night" => Some(Self::tokyo_night()),
            "catppuccin" => Some(Self::catppuccin()),
            "monochrome" => Some(Self::monochrome()),
            "solarized-dark" => Some(Self::solarized_dark()),
            _ => None,
        }
    }

    fn dracula() -> Self {
        Self {
            bg: "#282a36".into(), fg: "#f8f8f2".into(),
            accent: "#bd93f9".into(), success: "#50fa7b".into(),
            warning: "#f1fa8c".into(), danger: "#ff79c6".into(),
            muted: "#6272a4".into(), border: "#44475a".into(),
            skill_color: "#ff79c6".into(), mcp_color: "#f1fa8c".into(),
            model_color: "#bd93f9".into(),
            ..Default::default()
        }
    }

    fn nord() -> Self {
        Self {
            bg: "#2e3440".into(), fg: "#d8dee9".into(),
            accent: "#88c0d0".into(), success: "#a3be8c".into(),
            warning: "#ebcb8b".into(), danger: "#bf616a".into(),
            muted: "#5e81ac".into(), border: "#434c5e".into(),
            skill_color: "#b48ead".into(), mcp_color: "#d08770".into(),
            model_color: "#88c0d0".into(),
            ..Default::default()
        }
    }

    fn tokyo_night() -> Self {
        Self {
            bg: "#1a1b26".into(), fg: "#c0caf5".into(),
            accent: "#7aa2f7".into(), success: "#9ece6a".into(),
            warning: "#e0af68".into(), danger: "#f7768e".into(),
            muted: "#565f89".into(), border: "#292e42".into(),
            skill_color: "#bb9af7".into(), mcp_color: "#e0af68".into(),
            model_color: "#7aa2f7".into(),
            ..Default::default()
        }
    }

    fn catppuccin() -> Self {
        Self {
            bg: "#1e1e2e".into(), fg: "#cdd6f4".into(),
            accent: "#cba6f7".into(), success: "#a6e3a1".into(),
            warning: "#f9e2af".into(), danger: "#f38ba8".into(),
            muted: "#6c7086".into(), border: "#313244".into(),
            skill_color: "#f5c2e7".into(), mcp_color: "#fab387".into(),
            model_color: "#cba6f7".into(),
            ..Default::default()
        }
    }

    fn monochrome() -> Self {
        Self {
            bg: "#1a1a1a".into(), fg: "#cccccc".into(),
            accent: "#ffffff".into(), success: "#aaaaaa".into(),
            warning: "#999999".into(), danger: "#ffffff".into(),
            muted: "#555555".into(), border: "#333333".into(),
            skill_color: "#888888".into(), mcp_color: "#888888".into(),
            model_color: "#ffffff".into(),
            ..Default::default()
        }
    }

    fn solarized_dark() -> Self {
        Self {
            bg: "#002b36".into(), fg: "#839496".into(),
            accent: "#2aa198".into(), success: "#859900".into(),
            warning: "#b58900".into(), danger: "#dc322f".into(),
            muted: "#586e75".into(), border: "#073642".into(),
            skill_color: "#d33682".into(), mcp_color: "#cb4b16".into(),
            model_color: "#268bd2".into(),
            ..Default::default()
        }
    }

    /// Parse a hex color string like "#ff6b6b" into (r, g, b).
    pub fn parse_hex(hex: &str) -> Option<(u8, u8, u8)> {
        let hex = hex.trim_start_matches('#');
        if hex.len() != 6 { return None; }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some((r, g, b))
    }

    /// Resolve Auto to a concrete set using the real font probe.
    pub fn resolve_icon_set(&self) -> IconSet {
        self.resolve_icon_set_with(detect_nerd_font())
    }

    /// Pure resolution: Auto -> Nerd iff a Nerd Font is installed,
    /// otherwise Minimal. Explicit choices are never downgraded.
    pub fn resolve_icon_set_with(&self, has_nerd_font: bool) -> IconSet {
        match self.icon_set {
            IconSet::Auto => {
                if has_nerd_font {
                    IconSet::Nerd
                } else {
                    IconSet::Minimal
                }
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_icon_set_is_auto() {
        assert!(matches!(Theme::default().icon_set, IconSet::Auto));
    }

    #[test]
    fn auto_with_nerd_font_resolves_nerd() {
        let theme = Theme::default();
        assert!(matches!(theme.resolve_icon_set_with(true), IconSet::Nerd));
    }

    #[test]
    fn auto_without_nerd_font_resolves_minimal() {
        let theme = Theme::default();
        assert!(matches!(theme.resolve_icon_set_with(false), IconSet::Minimal));
    }

    #[test]
    fn explicit_nerd_is_never_downgraded() {
        let mut theme = Theme::default();
        theme.icon_set = IconSet::Nerd;
        assert!(matches!(theme.resolve_icon_set_with(false), IconSet::Nerd));
    }

    #[test]
    fn theme_ref_string_preset_parses() {
        // untagged 反序列化：TOML 字符串值 → Preset 变体
        let doc: toml::Value = toml::from_str("theme = \"dracula\"").unwrap();
        let tr: ThemeRef = doc["theme"].clone().try_into().unwrap();
        assert!(matches!(tr, ThemeRef::Preset(s) if s == "dracula"));
    }

    #[test]
    fn theme_ref_table_parses_partial() {
        let tbl: ThemeTable = toml::from_str("accent = \"#ff0000\"\n").unwrap();
        assert_eq!(tbl.preset, None);
        assert_eq!(tbl.overrides, None);
        assert!(tbl.colors.contains_key("accent"));
    }

    #[test]
    fn theme_ref_table_parses_preset_and_overrides() {
        let tbl: ThemeTable = toml::from_str(
            "preset = \"dracula\"\n[overrides]\naccent = \"#123456\"\n",
        ).unwrap();
        assert_eq!(tbl.preset.as_deref(), Some("dracula"));
        assert!(tbl.overrides.is_some());
        // flatten 契约：具名字段不被 colors 吞掉
        assert!(!tbl.colors.contains_key("preset"));
        assert!(!tbl.colors.contains_key("overrides"));
    }

    #[test]
    fn theme_ref_empty_table_parses() {
        let tbl: ThemeTable = toml::from_str("").unwrap();
        assert_eq!(tbl.preset, None);
        assert!(tbl.colors.is_empty());
    }

    #[test]
    fn theme_partial_table_uses_serde_defaults() {
        // 部分表（仅 1 个颜色键）→ 其余 19 键走 per-field default
        let theme: Theme = toml::from_str("accent = \"#ff0000\"\n").unwrap();
        assert_eq!(theme.accent, "#ff0000");
        assert_eq!(theme.bg, "#2e3440");
        assert_eq!(theme.bar_filled, '█');
    }

    #[test]
    fn apply_theme_keys_color_numeric_and_enum() {
        let mut base = Theme::default();
        let keys: HashMap<String, toml::Value> = toml::from_str(
            "accent = \"#123456\"\nbar_width = 20\nicon_set = \"nerd\"\n",
        ).unwrap();
        apply_theme_keys(&mut base, &keys);
        assert_eq!(base.accent, "#123456");
        assert_eq!(base.bar_width, 20);
        assert!(matches!(base.icon_set, IconSet::Nerd));
        assert_eq!(base.bg, "#2e3440"); // 未提供的键不变
    }

    #[test]
    fn apply_theme_keys_unknown_ignored() {
        let mut base = Theme::default();
        let keys: HashMap<String, toml::Value> =
            toml::from_str("future_key = \"x\"\n").unwrap();
        apply_theme_keys(&mut base, &keys);
        assert_eq!(base.accent, "#88c0d0");
    }

    #[test]
    fn apply_theme_keys_enum_bad_value_keeps_base() {
        let mut base = Theme::default();
        let keys: HashMap<String, toml::Value> =
            toml::from_str("icon_set = \"bogus\"\n").unwrap();
        apply_theme_keys(&mut base, &keys);
        assert!(matches!(base.icon_set, IconSet::Auto));
    }

    #[test]
    fn apply_theme_keys_char_tokens() {
        let mut base = Theme::default();
        let keys: HashMap<String, toml::Value> =
            toml::from_str("bar_filled = \"■\"\n").unwrap();
        apply_theme_keys(&mut base, &keys);
        assert_eq!(base.bar_filled, '■');
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            bg: "#2e3440".into(),
            fg: "#d8dee9".into(),
            accent: "#88c0d0".into(),
            success: "#a3be8c".into(),
            warning: "#ebcb8b".into(),
            danger: "#bf616a".into(),
            muted: "#5e81ac".into(),
            border: "#434c5e".into(),
            skill_color: "#b48ead".into(),
            mcp_color: "#d08770".into(),
            model_color: "#88c0d0".into(),
            bar_filled: default_bar_filled(),
            bar_empty: default_bar_empty(),
            separator: default_separator(),
            border_style: default_border_style(),
            icon_set: default_icon_set(),
            bar_width: default_bar_width(),
            padding: default_padding(),
            compact_lines: default_compact_lines(),
            dashboard_grid: default_dashboard_grid(),
        }
    }
}

/// 合并结果：基底 preset 名 + 完整主题。mod save 快照需要知道基底名。
#[derive(Debug, Clone)]
pub struct ResolvedTheme {
    pub preset: Option<String>,
    pub theme: Theme,
}

/// 将键表中与 Theme 20 字段同名的键类型化覆盖到 base；未知键忽略。
/// colors 与 overrides 共用（唯一的差别是调用层级）。
pub fn apply_theme_keys(base: &mut Theme, keys: &HashMap<String, toml::Value>) {
    for (k, v) in keys {
        match k.as_str() {
            "bg" => base.bg = v.as_str().unwrap_or(&base.bg).to_string(),
            "fg" => base.fg = v.as_str().unwrap_or(&base.fg).to_string(),
            "accent" => base.accent = v.as_str().unwrap_or(&base.accent).to_string(),
            "success" => base.success = v.as_str().unwrap_or(&base.success).to_string(),
            "warning" => base.warning = v.as_str().unwrap_or(&base.warning).to_string(),
            "danger" => base.danger = v.as_str().unwrap_or(&base.danger).to_string(),
            "muted" => base.muted = v.as_str().unwrap_or(&base.muted).to_string(),
            "border" => base.border = v.as_str().unwrap_or(&base.border).to_string(),
            "skill_color" => base.skill_color = v.as_str().unwrap_or(&base.skill_color).to_string(),
            "mcp_color" => base.mcp_color = v.as_str().unwrap_or(&base.mcp_color).to_string(),
            "model_color" => base.model_color = v.as_str().unwrap_or(&base.model_color).to_string(),
            "separator" => base.separator = v.as_str().unwrap_or(&base.separator).to_string(),
            "bar_filled" => {
                if let Some(c) = v.as_str().and_then(|s| s.chars().next()) {
                    base.bar_filled = c;
                }
            }
            "bar_empty" => {
                if let Some(c) = v.as_str().and_then(|s| s.chars().next()) {
                    base.bar_empty = c;
                }
            }
            "bar_width" => {
                if let Some(i) = v.as_integer() {
                    base.bar_width = i as u16;
                }
            }
            "padding" => {
                if let Some(i) = v.as_integer() {
                    base.padding = i as u16;
                }
            }
            "compact_lines" => {
                if let Some(i) = v.as_integer() {
                    base.compact_lines = i as u8;
                }
            }
            "dashboard_grid" => {
                if let Some(i) = v.as_integer() {
                    base.dashboard_grid = i as u8;
                }
            }
            "icon_set" => {
                if let Some(s) = v.as_str() {
                    base.icon_set = match s {
                        "auto" => IconSet::Auto,
                        "nerd" => IconSet::Nerd,
                        "ascii" => IconSet::Ascii,
                        "minimal" => IconSet::Minimal,
                        _ => base.icon_set,
                    };
                }
            }
            "border_style" => {
                if let Some(s) = v.as_str() {
                    base.border_style = match s {
                        "single" => BorderStyle::Single,
                        "double" => BorderStyle::Double,
                        "rounded" => BorderStyle::Rounded,
                        "thick" => BorderStyle::Thick,
                        "hidden" => BorderStyle::Hidden,
                        _ => base.border_style.clone(),
                    };
                }
            }
            _ => {}
        }
    }
}

/// Probe whether a Nerd Font is installed. Never panics: any probe
/// failure means "no Nerd Font" so callers fall back to Minimal.
pub fn detect_nerd_font() -> bool {
    detect_nerd_font_platform()
}

#[cfg(target_os = "windows")]
fn detect_nerd_font_platform() -> bool {
    let output = std::process::Command::new("reg")
        .args(["query", r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts"])
        .output();
    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .to_lowercase()
            .contains("nerd"),
        Err(_) => false,
    }
}

#[cfg(target_os = "linux")]
fn detect_nerd_font_platform() -> bool {
    let output = std::process::Command::new("fc-list").output();
    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .to_lowercase()
            .contains("nerd"),
        Err(_) => false,
    }
}

#[cfg(target_os = "macos")]
fn detect_nerd_font_platform() -> bool {
    let mut dirs: Vec<std::path::PathBuf> = vec![
        "/System/Library/Fonts".into(),
        "/Library/Fonts".into(),
    ];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("Library").join("Fonts"));
    }
    dirs.iter().any(|dir| {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .any(|e| e.file_name().to_string_lossy().to_lowercase().contains("nerd"))
            })
            .unwrap_or(false)
    })
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn detect_nerd_font_platform() -> bool { false }
