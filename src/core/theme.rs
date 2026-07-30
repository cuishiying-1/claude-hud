use serde::{Deserialize, Serialize};

/// Complete theme definition (20 tokens).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Theme {
    // Color tokens (11)
    pub bg: String,
    pub fg: String,
    pub accent: String,
    pub success: String,
    pub warning: String,
    pub danger: String,
    pub muted: String,
    pub border: String,
    pub skill_color: String,
    pub mcp_color: String,
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

fn default_bar_filled() -> char { '█' }
fn default_bar_empty() -> char { '░' }
fn default_separator() -> String { " │ ".into() }
fn default_border_style() -> BorderStyle { BorderStyle::Rounded }
fn default_icon_set() -> IconSet { IconSet::Nerd }
fn default_bar_width() -> u16 { 16 }
fn default_padding() -> u16 { 1 }
fn default_compact_lines() -> u8 { 2 }
fn default_dashboard_grid() -> u8 { 2 }

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BorderStyle {
    Single,
    Double,
    Rounded,
    Thick,
    Hidden,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IconSet {
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

    /// Interpolate between start and end colors by t (0.0 to 1.0).
    pub fn interpolate_hex(start: &str, end: &str, t: f64) -> Option<(u8, u8, u8)> {
        let (sr, sg, sb) = Self::parse_hex(start)?;
        let (er, eg, eb) = Self::parse_hex(end)?;
        let t = t.clamp(0.0, 1.0);
        Some((
            (sr as f64 + (er as f64 - sr as f64) * t) as u8,
            (sg as f64 + (eg as f64 - sg as f64) * t) as u8,
            (sb as f64 + (eb as f64 - sb as f64) * t) as u8,
        ))
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::nord()
    }
}
