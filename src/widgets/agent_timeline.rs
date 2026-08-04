use std::sync::Mutex;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Sparkline};

use crate::core::ansi;
use crate::core::i18n::tr;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::transcript::TranscriptSummary;
use crate::core::widget::{Widget, WidgetConfig};

pub struct AgentTimeline {
    summary: Mutex<Option<TranscriptSummary>>,
}

impl AgentTimeline {
    pub fn new() -> Self {
        Self { summary: Mutex::new(None) }
    }
}

impl Widget for AgentTimeline {
    fn id(&self) -> &str { "agent_timeline" }

    fn display_name(&self) -> &str { "Agent Timeline" }

    fn render_compact(&self, _data: &SessionData, _theme: &Theme, _config: &WidgetConfig) -> String {
        String::new()
    }

    fn render_dashboard(&self, _data: &SessionData, area: Rect, frame: &mut Frame, theme: &Theme, config: &WidgetConfig) {
        let mut lines: Vec<Line> = vec![];
        lines.push(Line::from(Span::styled("Agent Timeline",
            Style::default().fg(ansi::parse_ratatui_color(&theme.accent)))));

        if let Ok(ref guard) = self.summary.lock() {
            if let Some(ref summary) = **guard {
                if !summary.token_timeline.is_empty() {
                    let data: Vec<u64> = summary.token_timeline.iter().map(|s| s.total_tokens).collect();
                    let sparkline = Sparkline::default()
                        .data(&data)
                        .style(Style::default().fg(ansi::parse_ratatui_color(&theme.success)));
                    let spark_area = Rect { y: area.y + 1, height: 3.min(area.height.saturating_sub(2)), ..area };
                    frame.render_widget(sparkline, spark_area);

                    let agent_list: Vec<String> = summary.agents.iter()
                        .map(|a| format!("{}{}", if a.is_active { "●" } else { "✓" }, a.name))
                        .collect();
                    let text = format!("{}: {}", tr(config.lang, "runtime.timeline_agents"), agent_list.join(" · "));
                    let text_area = Rect { y: spark_area.y + spark_area.height + 1, height: 1, ..area };
                    frame.render_widget(Paragraph::new(Text::from(text)), text_area);
                    frame.render_widget(Paragraph::new(Text::from(lines)), area);
                    return;
                }
            }
        }
        lines.push(Line::from(tr(config.lang, "runtime.no_timeline")));
        frame.render_widget(Paragraph::new(Text::from(lines)), area);
    }

    fn update_transcript(&self, summary: &TranscriptSummary) {
        if let Ok(ref mut guard) = self.summary.lock() {
            **guard = Some(summary.clone());
        }
    }
}
