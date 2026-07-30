use std::sync::Mutex;
use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Text};
use ratatui::widgets::Paragraph;

use crate::core::ansi;
use crate::core::scripting::{ScriptEngine, http_poll, run_shell_command};
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::widget::{Widget, WidgetConfig};

pub enum ScriptWidgetType {
    Rhai { script_path: String, engine: ScriptEngine },
    Shell { command: String, refresh_secs: u64 },
    Http { url: String, refresh_secs: u64 },
}

pub struct ScriptWidget {
    widget_type: ScriptWidgetType,
    cached_output: Mutex<String>,
    last_refresh: Mutex<Option<Instant>>,
}

impl ScriptWidget {
    pub fn new_rhai(script_path: String) -> Self {
        Self { widget_type: ScriptWidgetType::Rhai { script_path, engine: ScriptEngine::new() }, cached_output: Mutex::new(String::new()), last_refresh: Mutex::new(None) }
    }
    pub fn new_shell(command: String, refresh_secs: u64) -> Self {
        Self { widget_type: ScriptWidgetType::Shell { command, refresh_secs }, cached_output: Mutex::new(String::new()), last_refresh: Mutex::new(None) }
    }
    pub fn new_http(url: String, refresh_secs: u64) -> Self {
        Self { widget_type: ScriptWidgetType::Http { url, refresh_secs }, cached_output: Mutex::new(String::new()), last_refresh: Mutex::new(None) }
    }

    fn should_refresh(&self) -> bool {
        let secs = match &self.widget_type {
            ScriptWidgetType::Shell { refresh_secs, .. } | ScriptWidgetType::Http { refresh_secs, .. } => *refresh_secs,
            ScriptWidgetType::Rhai { .. } => 5,
        };
        self.last_refresh.lock().ok().map_or(true, |t| t.map_or(true, |t| t.elapsed().as_secs() >= secs))
    }

    fn refresh_output(&self, data: &SessionData, theme: &Theme) {
        let output = match &self.widget_type {
            ScriptWidgetType::Rhai { script_path, engine } => engine.run_widget_script(script_path, data, theme).unwrap_or_else(|e| format!("rhai: {}", e)),
            ScriptWidgetType::Shell { command, .. } => run_shell_command(command).unwrap_or_else(|e| format!("shell: {}", e)),
            ScriptWidgetType::Http { url, .. } => http_poll(url).unwrap_or_else(|e| format!("http: {}", e)),
        };
        if let Ok(ref mut guard) = self.cached_output.lock() { **guard = output; }
        if let Ok(ref mut guard) = self.last_refresh.lock() { **guard = Some(Instant::now()); }
    }
}

impl Widget for ScriptWidget {
    fn id(&self) -> &str {
        match &self.widget_type {
            ScriptWidgetType::Rhai { .. } => "script_rhai",
            ScriptWidgetType::Shell { .. } => "script_shell",
            ScriptWidgetType::Http { .. } => "script_http",
        }
    }
    fn display_name(&self) -> &str {
        match &self.widget_type {
            ScriptWidgetType::Rhai { script_path, .. } => script_path.as_str(),
            ScriptWidgetType::Shell { command, .. } => command.as_str(),
            ScriptWidgetType::Http { url, .. } => url.as_str(),
        }
    }

    fn render_compact(&self, data: &SessionData, theme: &Theme, _config: &WidgetConfig) -> String {
        if self.should_refresh() { self.refresh_output(data, theme); }
        self.cached_output.lock().ok().map_or(String::new(), |g| g.lines().next().unwrap_or("").to_string())
    }

    fn render_dashboard(&self, data: &SessionData, area: Rect, frame: &mut Frame, theme: &Theme, _config: &WidgetConfig) {
        if self.should_refresh() { self.refresh_output(data, theme); }
        if let Ok(ref guard) = self.cached_output.lock() {
            let lines: Vec<Line> = guard.lines().map(|l| Line::from(l.to_string())).collect();
            frame.render_widget(Paragraph::new(Text::from(lines)), area);
        }
    }
}
