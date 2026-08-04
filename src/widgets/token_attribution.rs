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

pub struct TokenAttribution {
    summary: Mutex<Option<TranscriptSummary>>,
}

impl TokenAttribution {
    pub fn new() -> Self {
        Self { summary: Mutex::new(None) }
    }
}

impl Widget for TokenAttribution {
    fn id(&self) -> &str { "token_attribution" }

    fn display_name(&self) -> &str { "Token Attribution" }

    fn render_compact(&self, _data: &SessionData, theme: &Theme, config: &WidgetConfig) -> String {
        if let Ok(ref guard) = self.summary.lock() {
            if let Some(ref summary) = **guard {
                let attr = summary.token_attribution();
                if let Some((top_agent, pct)) = attr.first() {
                    return format!("{}",
                        ansi::ansi_fg(&format!("{}:{} {:.0}%", tr(config.lang, "runtime.top_pct"), top_agent.name, pct), &theme.accent));
                }
            }
        }
        String::new()
    }

    fn render_dashboard(&self, _data: &SessionData, area: Rect, frame: &mut Frame, theme: &Theme, config: &WidgetConfig) {
        let mut lines: Vec<Line> = vec![];
        lines.push(Line::from(Span::styled("Token Attribution",
            Style::default().fg(ansi::parse_ratatui_color(&theme.accent)))));

        let lock = self.summary.lock();
        if let Ok(ref guard) = lock {
            if let Some(ref summary) = **guard {
                let attr = summary.token_attribution();
                let colors = [&theme.danger, &theme.warning, &theme.success, &theme.muted];
                for (i, (agent, pct)) in attr.iter().take(8).enumerate() {
                    let color = colors[i % colors.len()];
                    let bar_width = (*pct / 100.0 * 20.0) as usize;
                    let bar = "█".repeat(bar_width);
                    lines.push(Line::from(vec![
                        Span::styled(bar, Style::default().fg(ansi::parse_ratatui_color(color))),
                        Span::raw(" "), Span::raw(&agent.name), Span::raw(" "),
                        Span::styled(format!("{:.0}%", pct),
                            Style::default().fg(ansi::parse_ratatui_color(&theme.muted))),
                    ]));
                }
                frame.render_widget(Paragraph::new(Text::from(lines)), area);
                return;
            }
        }
        lines.push(Line::from(tr(config.lang, "runtime.no_token_data")));
        frame.render_widget(Paragraph::new(Text::from(lines)), area);
    }

    fn update_transcript(&self, summary: &TranscriptSummary) {
        if let Ok(ref mut guard) = self.summary.lock() {
            **guard = Some(summary.clone());
        }
    }
}
