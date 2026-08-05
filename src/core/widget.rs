use ratatui::layout::Rect;
use ratatui::Frame;

use super::session::SessionData;
use super::theme::Theme;

/// User-level configuration for a widget instance.
#[derive(Debug, Clone)]
pub struct WidgetConfig {
    pub values: std::collections::HashMap<String, String>,
    /// 全局语言注入（config.widget_config 构建时写入；默认 En）。
    pub lang: crate::core::i18n::Language,
    /// 当前紧凑布局是否含 context_bar（widget_config 构建时写入）。
    /// cost_display 用它做 token 去重：显式 show_tokens 键优先，否则同屏自动隐藏。
    pub context_bar_present: bool,
}

impl WidgetConfig {
    pub fn get_str(&self, key: &str, default: &str) -> String {
        self.values.get(key).cloned().unwrap_or_else(|| default.to_string())
    }

    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        self.values
            .get(key)
            .map(|v| v == "true")
            .unwrap_or(default)
    }

    pub fn get_f64(&self, key: &str, default: f64) -> f64 {
        self.values
            .get(key)
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }

    pub fn get_u64(&self, key: &str, default: u64) -> u64 {
        self.values
            .get(key)
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }
}

impl Default for WidgetConfig {
    fn default() -> Self {
        Self {
            values: std::collections::HashMap::new(),
            lang: crate::core::i18n::Language::En,
            context_bar_present: false,
        }
    }
}

/// The core trait every widget implements.
pub trait Widget {
    /// Unique identifier, e.g. "context_bar".
    fn id(&self) -> &str;

    /// Human-readable name shown in config UIs.
    fn display_name(&self) -> &str;

    /// Render a single line of ANSI for the compact status bar.
    fn render_compact(
        &self,
        data: &SessionData,
        theme: &Theme,
        config: &WidgetConfig,
    ) -> String;

    /// Render inside a ratatui frame region for the full dashboard.
    fn render_dashboard(
        &self,
        data: &SessionData,
        area: Rect,
        frame: &mut Frame,
        theme: &Theme,
        config: &WidgetConfig,
    );

    /// Optional: receive transcript summary update (Phase 2).
    fn update_transcript(&self, _summary: &super::transcript::TranscriptSummary) {}
}

/// Registry of all available widgets.
pub struct WidgetRegistry {
    pub widgets: Vec<Box<dyn Widget>>,
}

impl WidgetRegistry {
    pub fn new() -> Self {
        Self { widgets: Vec::new() }
    }

    pub fn register(&mut self, widget: Box<dyn Widget>) {
        self.widgets.push(widget);
    }

    pub fn get(&self, id: &str) -> Option<&dyn Widget> {
        self.widgets.iter().find(|w| w.id() == id).map(|w| w.as_ref())
    }

    pub fn list(&self) -> Vec<&dyn Widget> {
        self.widgets.iter().map(|w| w.as_ref()).collect()
    }
}

impl Default for WidgetRegistry {
    fn default() -> Self {
        Self::new()
    }
}
