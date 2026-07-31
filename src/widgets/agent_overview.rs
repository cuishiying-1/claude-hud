use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Text;

use crate::core::ansi;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::widget::{Widget, WidgetConfig};

pub struct AgentOverview;

impl Widget for AgentOverview {
    fn id(&self) -> &str { "agent_overview" }
    fn display_name(&self) -> &str { "Agent Overview" }

    fn render_compact(&self, data: &SessionData, theme: &Theme, config: &WidgetConfig) -> String {
        let empty_agents = Vec::new();
        let agents = data.subagent_status_line.as_ref().map(|s| &s.agents).unwrap_or(&empty_agents);
        if agents.is_empty() { return String::new(); }
        let total = agents.len();
        let active = agents.iter().filter(|a| a.is_active).count();
        let stalled = agents.iter().filter(|a| a.is_active && a.elapsed_secs > config.get_u64("stall_threshold_sec", 30)).count();
        let mut parts = vec![];
        let icon = if stalled > 0 { ansi::ansi_fg("⬤", &theme.danger) }
            else if active > 0 { ansi::ansi_fg("⚡", &theme.success) }
            else { ansi::ansi_fg("✓", &theme.muted) };
        parts.push(icon);
        parts.push(ansi::ansi_fg(&format!("{}/{} agents", active, total),
            if stalled > 0 { &theme.warning } else { &theme.success }));
        if stalled > 0 {
            parts.push(ansi::ansi_fg(&format!(" · {} stalled", stalled), &theme.danger));
        }
        parts.join("")
    }

    fn render_dashboard(&self, data: &SessionData, area: Rect, frame: &mut Frame, _theme: &Theme, _config: &WidgetConfig) {
        let empty_agents = Vec::new();
        let agents = data.subagent_status_line.as_ref().map(|s| &s.agents).unwrap_or(&empty_agents);
        let total = agents.len();
        let active = agents.iter().filter(|a| a.is_active).count();
        let done = total - active;
        let pct = if total > 0 { (done as f64 / total as f64) * 100.0 } else { 0.0 };
        frame.render_widget(Text::from(format!("Agents — Total: {} | Active: {} | Done: {} | {:.0}%", total, active, done, pct)), area);
    }
}
