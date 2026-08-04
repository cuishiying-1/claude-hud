use std::sync::Mutex;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::core::ansi;
use crate::core::animation;
use crate::core::i18n::tr;
use crate::core::session::SessionData;
use crate::core::state;
use crate::core::theme::Theme;
use crate::core::transcript::TranscriptSummary;
use crate::core::widget::{Widget, WidgetConfig};

pub struct Alerts {
    summary: Mutex<Option<TranscriptSummary>>,
}

impl Alerts {
    pub fn new() -> Self {
        Self { summary: Mutex::new(None) }
    }
}

impl Widget for Alerts {
    fn id(&self) -> &str { "alerts" }

    fn display_name(&self) -> &str { "Alerts" }

    fn render_compact(&self, data: &SessionData, theme: &Theme, config: &WidgetConfig) -> String {
        let mut alerts = vec![];
        let pct = data.context_window.used_percentage;
        let critical = config.get_f64("context_critical", 95.0);
        let warn = config.get_f64("context_warn", 80.0);
        let cost_warn = config.get_f64("cost_warn_usd", 10.0);

        if pct >= critical {
            let (r, g, b) = animation::breathe(&theme.danger, animation::now_phase(4.0));
            let hex = format!("#{:02x}{:02x}{:02x}", r, g, b);
            alerts.push(ansi::ansi_fg(&format!("⚠ ctx {:.0}%", pct), &hex));
        } else if pct >= warn {
            alerts.push(ansi::ansi_fg(&format!("ctx {:.0}%", pct), &theme.warning));
        }
        let cost = config.get_f64("effective_cost", data.cost.total_cost_usd);
        let symbol = config.get_str("currency_symbol", "$");
        let estimated = config.get_bool("cost_estimated", false);
        if cost >= cost_warn {
            let prefix = if estimated { "≈" } else { "" };
            alerts.push(ansi::ansi_fg(&format!("{}{}{:.2}", prefix, symbol, cost), &theme.warning));
        }
        if data.rate_limits.five_hour.used_percentage >= 90.0 {
            alerts.push(ansi::ansi_fg(tr(config.lang, "runtime.rate_limited"), &theme.danger));
        }
        if let Ok(ref guard) = self.summary.lock() {
            if let Some(ref summary) = **guard {
                // 卡顿检测只在时间轴可靠时真实触发；不可靠会话不猜测
                if summary.timestamps_reliable {
                    let stalled = summary.stalled_agents(30, state::now_secs());
                    if !stalled.is_empty() {
                        alerts.push(ansi::ansi_fg(&format!("⚠ {} {}", stalled.len(), tr(config.lang, "runtime.stalled")), &theme.danger));
                    }
                }
                if let Some(minutes) = summary
                    .compaction_prediction(pct, data.context_window.context_window_size)
                {
                    if minutes < 10 {
                        alerts.push(ansi::ansi_fg(&format!("{} ~{}m", tr(config.lang, "runtime.compact_eta"), minutes), &theme.warning));
                    }
                }
            }
        }
        if alerts.is_empty() { String::new() } else { alerts.join(" · ") }
    }

    fn render_dashboard(&self, data: &SessionData, area: Rect, frame: &mut Frame, theme: &Theme, config: &WidgetConfig) {
        let mut lines = vec![
            Line::from(Span::styled("Alerts", Style::default().fg(ansi::parse_ratatui_color(&theme.accent)))),
        ];
        let pct = data.context_window.used_percentage;
        if pct >= config.get_f64("context_critical", 95.0) {
            lines.push(Line::from(Span::styled(format!("⚠ {}: {:.0}% — {}", tr(config.lang, "runtime.critical_label"), pct, tr(config.lang, "runtime.compaction_imminent")),
                Style::default().fg(ansi::parse_ratatui_color(&theme.danger)))));
        } else if pct >= config.get_f64("context_warn", 80.0) {
            lines.push(Line::from(Span::styled(format!("⚠ {}: {:.0}%", tr(config.lang, "runtime.warning_label"), pct),
                Style::default().fg(ansi::parse_ratatui_color(&theme.warning)))));
        } else {
            lines.push(Line::from(format!("✓ {}", tr(config.lang, "runtime.no_critical"))));
        }
        if let Ok(ref guard) = self.summary.lock() {
            if let Some(ref summary) = **guard {
                if summary.timestamps_reliable {
                    let now = state::now_secs();
                    for agent in summary.stalled_agents(30, now) {
                        // ⑮ 卡顿归因：有最后工具名 → 归因文本；无 → 维持原格式
                        let line_text = match &agent.last_tool_name {
                            Some(tool) => {
                                let idle = agent.last_tool_call_secs.map_or(0, |t| now.saturating_sub(t));
                                format!(
                                    "⚠ {} '{}' {}",
                                    tr(config.lang, "runtime.agent_word"),
                                    agent.name,
                                    crate::widgets::agent_detail::stalled_attr(idle, tool, config.lang)
                                )
                            }
                            None => format!(
                                "⚠ {} '{}' {} >30s",
                                tr(config.lang, "runtime.agent_word"),
                                agent.name,
                                tr(config.lang, "runtime.stalled")
                            ),
                        };
                        lines.push(Line::from(Span::styled(
                            line_text,
                            Style::default().fg(ansi::parse_ratatui_color(&theme.danger)))));
                    }
                }
            }
        }
        frame.render_widget(Paragraph::new(Text::from(lines)), area);
    }

    fn update_transcript(&self, summary: &TranscriptSummary) {
        if let Ok(ref mut guard) = self.summary.lock() { **guard = Some(summary.clone()); }
    }
}

