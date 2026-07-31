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
fn default_icon_set() -> IconSet { IconSet::Auto }
fn default_bar_width() -> u16 { 16 }
fn default_padding() -> u16 { 1 }
fn default_compact_lines() -> u8 { 2 }
fn default_dashboard_grid() -> u8 { 2 }

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BorderStyle {
    Single,
    Double,
    Rounded,
    Thick,
    Hidden,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
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
