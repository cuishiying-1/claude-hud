use std::sync::Mutex;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Text;

use crate::core::ansi;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::widget::{Widget, WidgetConfig};
use crate::probe::git::GitStatus;

pub struct GitStatusWidget {
    cached: Mutex<Option<GitStatus>>,
}

impl GitStatusWidget {
    pub fn new() -> Self {
        Self { cached: Mutex::new(None) }
    }
}

impl Widget for GitStatusWidget {
    fn id(&self) -> &str { "git_status" }
    fn display_name(&self) -> &str { "Git Status" }

    fn render_compact(&self, _data: &SessionData, theme: &Theme, _config: &WidgetConfig) -> String {
        let status = crate::probe::git::probe_git();
        let mut parts = vec![];
        if let Some(ref s) = status {
            parts.push(ansi::ansi_fg(&s.branch, &theme.accent));
            if s.is_dirty { parts.push(ansi::ansi_fg("*", &theme.warning)); }
            if s.ahead > 0 { parts.push(ansi::ansi_fg(&format!("↑{}", s.ahead), &theme.muted)); }
            if s.behind > 0 { parts.push(ansi::ansi_fg(&format!("↓{}", s.behind), &theme.muted)); }
        } else {
            parts.push(ansi::ansi_fg("—", &theme.muted));
        }
        if let Ok(ref mut guard) = self.cached.lock() { **guard = status; }
        parts.join(" ")
    }

    fn render_dashboard(&self, _data: &SessionData, area: Rect, frame: &mut Frame, _theme: &Theme, _config: &WidgetConfig) {
        if let Ok(ref guard) = self.cached.lock() {
            let text = match guard.as_ref() {
                Some(s) => format!("Branch: {} | Dirty: {} | Ahead: {} | Behind: {}", s.branch, s.is_dirty, s.ahead, s.behind),
                None => "Git: not a repository".into(),
            };
            frame.render_widget(Text::from(text), area);
        }
    }
}
