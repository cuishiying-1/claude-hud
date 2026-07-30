use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Text;

use crate::core::ansi;
use crate::core::session::SessionData;
use crate::core::theme::{IconSet, Theme};
use crate::core::widget::{Widget, WidgetConfig};

pub struct SkillsMcp;

impl Widget for SkillsMcp {
    fn id(&self) -> &str { "skills_mcp" }
    fn display_name(&self) -> &str { "Skills & MCP" }

    fn render_compact(&self, _data: &SessionData, theme: &Theme, _config: &WidgetConfig) -> String {
        let sc = std::env::var("CLAUDE_HUD_SKILL_COUNT").ok().and_then(|v| v.parse().ok()).unwrap_or(0usize);
        let mc = std::env::var("CLAUDE_HUD_MCP_COUNT").ok().and_then(|v| v.parse().ok()).unwrap_or(0usize);
        let si = match theme.icon_set {
            IconSet::Nerd => "🧩 ", IconSet::Ascii => "[SK] ", IconSet::Minimal => "◇ ",
        };
        let mi = match theme.icon_set {
            IconSet::Nerd => "🔌 ", IconSet::Ascii => "[MC] ", IconSet::Minimal => "◆ ",
        };
        format!("{}{} {}{}",
            ansi::ansi_fg(si, &theme.skill_color),
            ansi::ansi_fg(&sc.to_string(), &theme.fg),
            ansi::ansi_fg(mi, &theme.mcp_color),
            ansi::ansi_fg(&mc.to_string(), &theme.fg))
    }

    fn render_dashboard(&self, _data: &SessionData, area: Rect, frame: &mut Frame, _theme: &Theme, _config: &WidgetConfig) {
        frame.render_widget(Text::from("Skills & MCP — Phase 1 static (Phase 2: dynamic tracking)"), area);
    }
}
