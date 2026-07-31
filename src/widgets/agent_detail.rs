use std::sync::Mutex;

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::core::ansi;
use crate::core::animation::AnimationState;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::transcript::TranscriptSummary;
use crate::core::widget::{Widget, WidgetConfig};

pub struct AgentDetail {
    summary: Mutex<Option<TranscriptSummary>>,
    anim: Mutex<AnimationState>,
}

impl AgentDetail {
    pub fn new() -> Self {
        Self {
            summary: Mutex::new(None),
            anim: Mutex::new(AnimationState::new(true)),
        }
    }
}

impl Widget for AgentDetail {
    fn id(&self) -> &str { "agent_detail" }

    fn display_name(&self) -> &str { "Agent Detail" }

    fn render_compact(
        &self,
        _data: &SessionData,
        theme: &Theme,
        config: &WidgetConfig,
    ) -> String {
        let stall_secs = config.get_u64("stall_threshold_sec", 30);
        let mut parts = vec![];

        if let Ok(ref guard) = self.summary.lock() {
            if let Some(ref summary) = **guard {
                for agent in &summary.agents {
                    if !agent.is_active {
                        continue;
                    }
                    let is_stalled = agent
                        .last_tool_call_secs
                        .map_or(false, |t| {
                            agent.start_time_secs.saturating_sub(t) > stall_secs
                        });
                    let status = if is_stalled {
                        ansi::ansi_fg("◐", &theme.danger)
                    } else {
                        ansi::ansi_fg("◐", &theme.success)
                    };
                    let name = ansi::ansi_fg(&agent.name, &theme.accent);
                    let task =
                        ansi::ansi_fg(&ansi::truncate(&agent.task_description, 40), &theme.muted);
                    let elapsed = agent.start_time_secs;
                    let elapsed_str = if elapsed >= 60 {
                        format!("{}m{}s", elapsed / 60, elapsed % 60)
                    } else {
                        format!("{}s", elapsed)
                    };
                    let time = ansi::ansi_fg(&elapsed_str, &theme.muted);
                    parts.push(format!("{} {} {} {}", status, name, task, time));
                }
            }
        }
        parts.join(" │ ")
    }

    fn render_dashboard(
        &self,
        _data: &SessionData,
        area: Rect,
        frame: &mut Frame,
        theme: &Theme,
        _config: &WidgetConfig,
    ) {
        if let Ok(ref mut guard) = self.anim.lock() {
            guard.tick();
        }
        let anim = self.anim.lock().ok();
        let is_stalled_anim = anim
            .as_ref()
            .and_then(|a| a.neon_breathing(&theme.danger))
            .unwrap_or_else(|| Theme::parse_hex(&theme.danger).unwrap_or((255, 0, 0)));

        let mut lines: Vec<Line> = vec![];
        lines.push(Line::from(Span::styled(
            "Agent Detail",
            Style::default().fg(ansi::parse_ratatui_color(&theme.accent)),
        )));

        let lock = self.summary.lock();
        if let Ok(ref guard) = lock {
            if let Some(ref summary) = **guard {
                for agent in &summary.agents {
                    let is_stalled = agent
                        .last_tool_call_secs
                        .map_or(false, |t| {
                            agent.start_time_secs.saturating_sub(t) > 30
                        });
                    let status_color = if is_stalled {
                        Color::Rgb(is_stalled_anim.0, is_stalled_anim.1, is_stalled_anim.2)
                    } else if agent.is_active {
                        ansi::parse_ratatui_color(&theme.success)
                    } else {
                        ansi::parse_ratatui_color(&theme.muted)
                    };
                    let icon = if is_stalled {
                        "⬤"
                    } else if agent.is_active {
                        "●"
                    } else {
                        "✓"
                    };
                    let line = Line::from(vec![
                        Span::styled(icon, Style::default().fg(status_color)),
                        Span::raw(" "),
                        Span::raw(&agent.name),
                        Span::raw(" "),
                        Span::styled(
                            ansi::truncate(&agent.task_description, 50),
                            Style::default().fg(ansi::parse_ratatui_color(&theme.muted)),
                        ),
                    ]);
                    lines.push(line);
                }
            } else {
                lines.push(Line::from("No agent data (transcript not parsed)"));
            }
        } else {
            lines.push(Line::from("No agent data (transcript not parsed)"));
        }

        frame.render_widget(Paragraph::new(Text::from(lines)), area);
    }

    fn needs_tick(&self) -> bool { true }

    fn dashboard_size(&self) -> (u16, u16) { (30, 6) }

    fn update_transcript(&self, summary: &TranscriptSummary) {
        if let Ok(ref mut guard) = self.summary.lock() {
            **guard = Some(summary.clone());
        }
    }
}
