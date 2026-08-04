use std::sync::Mutex;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::core::ansi;
use crate::core::i18n::tr;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::transcript::TranscriptSummary;
use crate::core::widget::{Widget, WidgetConfig};

pub struct SessionStats {
    summary: Mutex<Option<TranscriptSummary>>,
}

impl SessionStats {
    pub fn new() -> Self {
        Self { summary: Mutex::new(None) }
    }
}

impl Widget for SessionStats {
    fn id(&self) -> &str { "session_stats" }

    fn display_name(&self) -> &str { "Session Stats" }

    fn render_compact(&self, data: &SessionData, theme: &Theme, config: &WidgetConfig) -> String {
        let dur_secs = data.cost.total_duration_ms / 1000;
        let mins = dur_secs / 60;
        let secs = dur_secs % 60;
        let dur_str = if mins > 0 { format!("{}m{}s", mins, secs) } else { format!("{}s", secs) };
        let tok_per_sec = if dur_secs > 0 {
            (data.context_window.total_input_tokens + data.context_window.total_output_tokens) / dur_secs
        } else { 0 };
        let total_tool_calls = self.summary.lock().ok().as_ref()
            .and_then(|g| g.as_ref())
            .map(|s| s.tool_counts.values().sum::<usize>())
            .unwrap_or(0);
        format!("{} {} {}",
            ansi::ansi_fg(&format!("⏱{}", dur_str), &theme.fg),
            ansi::ansi_fg(&format!("{}tok/s", tok_per_sec), &theme.accent),
            ansi::ansi_fg(&format!("{}{}", total_tool_calls, tr(config.lang, "runtime.calls")), &theme.muted))
    }

    fn render_dashboard(&self, data: &SessionData, area: Rect, frame: &mut Frame, theme: &Theme, config: &WidgetConfig) {
        let dur_secs = data.cost.total_duration_ms / 1000;
        let tool_calls = self.summary.lock().ok().as_ref()
            .and_then(|g| g.as_ref())
            .map(|s| s.tool_counts.values().sum::<usize>())
            .unwrap_or(0);
        let mut lines = vec![
            Line::from(Span::styled("Session Stats", Style::default().fg(ansi::parse_ratatui_color(&theme.accent)))),
            Line::from(format!("{}: {}m {}s | {}: +{}/-{} | {}: {}",
                tr(config.lang, "runtime.duration"),
                dur_secs / 60,
                dur_secs % 60,
                tr(config.lang, "runtime.lines"),
                data.cost.total_lines_added,
                data.cost.total_lines_removed,
                tr(config.lang, "runtime.tools"),
                tool_calls)),
        ];
        if let Ok(ref guard) = self.summary.lock() {
            if let Some(ref s) = **guard {
                let mut tools: Vec<(&String, &usize)> = s.tool_counts.iter().collect();
                tools.sort_by(|a, b| b.1.cmp(a.1));
                lines.push(Line::from(format!("{}:", tr(config.lang, "runtime.top_tools"))));
                for (name, count) in tools.iter().take(5) {
                    lines.push(Line::from(
                        tr(config.lang, "runtime.tool_calls")
                            .replace("{name}", name)
                            .replace("{count}", &count.to_string()),
                    ));
                }
            }
        }
        frame.render_widget(Paragraph::new(Text::from(lines)), area);
    }

    fn update_transcript(&self, summary: &TranscriptSummary) {
        if let Ok(ref mut guard) = self.summary.lock() {
            **guard = Some(summary.clone());
        }
    }
}
