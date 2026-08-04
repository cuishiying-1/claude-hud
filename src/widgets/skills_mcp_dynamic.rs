use std::collections::HashSet;
use std::sync::Mutex;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::core::ansi;
use crate::core::i18n::tr;
use crate::core::session::SessionData;
use crate::core::theme::{IconSet, Theme};
use crate::core::transcript::TranscriptSummary;
use crate::core::widget::{Widget, WidgetConfig};

pub struct SkillsMcpDynamic {
    summary: Mutex<Option<TranscriptSummary>>,
}

impl SkillsMcpDynamic {
    pub fn new() -> Self {
        Self { summary: Mutex::new(None) }
    }
}

impl Widget for SkillsMcpDynamic {
    fn id(&self) -> &str { "skills_mcp_dynamic" }

    fn display_name(&self) -> &str { "Skills & MCP Dynamic" }

    fn render_compact(&self, _data: &SessionData, theme: &Theme, _config: &WidgetConfig) -> String {
        let mut parts = vec![];
        if let Ok(ref guard) = self.summary.lock() {
            if let Some(ref summary) = **guard {
                let active_skills: Vec<&str> = summary.skill_calls.iter()
                    .filter(|s| s.is_active).map(|s| s.name.as_str()).collect();
                if !active_skills.is_empty() {
                    parts.push(ansi::ansi_fg(
                        &format!("{} {}", skill_icon(theme), active_skills.join(" ")),
                        &theme.skill_color));
                }
                let unique_mcps: Vec<String> = {
                    let mut seen = HashSet::new();
                    summary.mcp_calls.iter()
                        .map(|m| m.server.clone())
                        .filter(|s| seen.insert(s.clone()))
                        .collect()
                };
                if !unique_mcps.is_empty() {
                    parts.push(ansi::ansi_fg(
                        &format!("{} {}", mcp_icon(theme), unique_mcps.join(" ")),
                        &theme.mcp_color));
                }
            }
        }
        parts.join(" │ ")
    }

    fn render_dashboard(&self, _data: &SessionData, area: Rect, frame: &mut Frame, theme: &Theme, config: &WidgetConfig) {
        let mut lines = vec![
            Line::from(Span::styled(tr(config.lang, "runtime.skills_mcp_dynamic_title"),
                Style::default().fg(ansi::parse_ratatui_color(&theme.accent)))),
        ];
        if let Ok(ref guard) = self.summary.lock() {
            if let Some(ref summary) = **guard {
                if !summary.skill_calls.is_empty() {
                    lines.push(Line::from(Span::styled(tr(config.lang, "runtime.skills_colon"),
                        Style::default().fg(ansi::parse_ratatui_color(&theme.skill_color)))));
                    for skill in &summary.skill_calls {
                        let icon = if skill.is_active { "●" } else { "○" };
                        lines.push(Line::from(
                            tr(config.lang, "runtime.skill_calls")
                                .replace("{icon}", icon)
                                .replace("{name}", &skill.name)
                                .replace("{count}", &skill.call_count.to_string()),
                        ));
                    }
                }
                if !summary.mcp_calls.is_empty() {
                    lines.push(Line::from(Span::styled(tr(config.lang, "runtime.mcp_colon"),
                        Style::default().fg(ansi::parse_ratatui_color(&theme.mcp_color)))));
                    let mut server_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
                    for call in &summary.mcp_calls {
                        *server_counts.entry(call.server.clone()).or_default() += call.call_count;
                    }
                    for (server, count) in &server_counts {
                        lines.push(Line::from(
                            tr(config.lang, "runtime.mcp_calls")
                                .replace("{name}", server)
                                .replace("{count}", &count.to_string()),
                        ));
                    }
                }
                frame.render_widget(Paragraph::new(Text::from(lines)), area);
                return;
            }
        }
        lines.push(Line::from(tr(config.lang, "runtime.no_dynamic")));
        frame.render_widget(Paragraph::new(Text::from(lines)), area);
    }

    fn update_transcript(&self, summary: &TranscriptSummary) {
        if let Ok(ref mut guard) = self.summary.lock() {
            **guard = Some(summary.clone());
        }
    }
}

fn skill_icon(theme: &Theme) -> &str {
    match theme.icon_set {
        IconSet::Auto => "◇",             // 防御分支：所有渲染入口均已先决议 icon_set，此处正常不可达
        IconSet::Nerd => "🧩", IconSet::Ascii => "[SK]", IconSet::Minimal => "◇",
    }
}

fn mcp_icon(theme: &Theme) -> &str {
    match theme.icon_set {
        IconSet::Auto => "◆",             // 防御分支：所有渲染入口均已先决议 icon_set，此处正常不可达
        IconSet::Nerd => "🔌", IconSet::Ascii => "[MC]", IconSet::Minimal => "◆",
    }
}
