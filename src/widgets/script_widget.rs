use std::path::PathBuf;
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
    state_path: PathBuf,
}

impl ScriptWidget {
    pub fn new_rhai(script_path: String, state_path: PathBuf) -> Self {
        Self { widget_type: ScriptWidgetType::Rhai { script_path, engine: ScriptEngine::new() }, cached_output: Mutex::new(String::new()), last_refresh: Mutex::new(None), state_path }
    }
    pub fn new_shell(command: String, refresh_secs: u64, state_path: PathBuf) -> Self {
        Self { widget_type: ScriptWidgetType::Shell { command, refresh_secs }, cached_output: Mutex::new(String::new()), last_refresh: Mutex::new(None), state_path }
    }
    pub fn new_http(url: String, refresh_secs: u64, state_path: PathBuf) -> Self {
        Self { widget_type: ScriptWidgetType::Http { url, refresh_secs }, cached_output: Mutex::new(String::new()), last_refresh: Mutex::new(None), state_path }
    }

    /// Throttle key: the command/url/script path (unique per instance).
    fn throttle_key(&self) -> String {
        self.display_name().to_string()
    }

    fn should_refresh(&self) -> bool {
        let secs = match &self.widget_type {
            ScriptWidgetType::Shell { refresh_secs, .. } | ScriptWidgetType::Http { refresh_secs, .. } => *refresh_secs,
            ScriptWidgetType::Rhai { .. } => 5,
        };
        let in_process_fresh = self
            .last_refresh
            .lock()
            .ok()
            .map_or(false, |t| t.map_or(false, |t| t.elapsed().as_secs() < secs));
        if in_process_fresh {
            return false;
        }
        // 跨进程：last_run 持久化在 state.cache.script_throttle
        let now = crate::core::state::now_secs();
        let last_run = crate::core::state::StateFile::read(&self.state_path)
            .cache
            .script_throttle
            .get(&self.throttle_key())
            .copied()
            .unwrap_or(0);
        now.saturating_sub(last_run) >= secs
    }

    fn refresh_output(&self, data: &SessionData, theme: &Theme) {
        let output = match &self.widget_type {
            ScriptWidgetType::Rhai { script_path, engine } => engine.run_widget_script(script_path, data, theme).unwrap_or_else(|e| format!("rhai: {}", e)),
            ScriptWidgetType::Shell { command, .. } => run_shell_command(command).unwrap_or_else(|e| format!("shell: {}", e)),
            ScriptWidgetType::Http { url, .. } => http_poll(url).unwrap_or_else(|e| format!("http: {}", e)),
        };
        if let Ok(ref mut guard) = self.cached_output.lock() { **guard = output; }
        if let Ok(ref mut guard) = self.last_refresh.lock() { **guard = Some(Instant::now()); }
        // 回写跨进程节流时间戳（窄键 read-modify-write，失败静默）
        let now = crate::core::state::now_secs();
        let key = self.throttle_key();
        let _ = crate::core::state::StateFile::update(&self.state_path, |st| {
            st.cache.script_throttle.insert(key, now);
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_state() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("hud-throttle-{}.json", std::process::id()));
        p
    }

    #[test]
    fn cross_process_throttle_uses_state() {
        let p = tmp_state();
        let _ = std::fs::remove_file(&p);
        let theme = crate::core::theme::Theme::default();
        let data = crate::core::session::SessionData::default();
        let cfg = WidgetConfig::default();
        let key = "echo hi".to_string();

        // state 节流新鲜 → 不刷新（cached 为空 → 渲染空串）
        let widget = ScriptWidget::new_shell("echo hi".into(), 30, p.clone());
        crate::core::state::StateFile::update(&p, |st| {
            st.cache.script_throttle.insert(key.clone(), crate::core::state::now_secs());
        })
        .unwrap();
        assert_eq!(widget.render_compact(&data, &theme, &cfg), "");

        // state 节流过期 → 刷新并回写 last_run
        let widget2 = ScriptWidget::new_shell("echo hi".into(), 30, p.clone());
        crate::core::state::StateFile::update(&p, |st| {
            st.cache.script_throttle.insert(key.clone(), 0);
        })
        .unwrap();
        assert_eq!(widget2.render_compact(&data, &theme, &cfg), "hi");
        let st = crate::core::state::StateFile::read(&p);
        assert!(st.cache.script_throttle.get(&key).copied().unwrap_or(0) > 0);
        let _ = std::fs::remove_file(&p);
    }
}
