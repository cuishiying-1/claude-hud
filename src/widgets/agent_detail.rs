use std::sync::Mutex;

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::core::ansi;
use crate::core::animation;
use crate::core::i18n::tr;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::transcript::TranscriptSummary;
use crate::core::widget::{Widget, WidgetConfig};

/// 卡顿判定：真实触发需 now − last_tool_call > threshold；不可靠
/// 时间轴不猜测（返回 false，避免行号代秒触发假告警）。
fn is_stalled(
    agent: &crate::core::transcript::AgentRecord,
    summary: &TranscriptSummary,
    now_secs: u64,
    stall_secs: u64,
) -> bool {
    summary.timestamps_reliable
        && agent
            .last_tool_call_secs
            .map(|t| now_secs.saturating_sub(t) > stall_secs)
            .unwrap_or(false)
}

fn format_dur(secs: u64) -> String {
    if secs >= 60 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}s", secs)
    }
}

/// ⑮ 卡顿归因文本：`stalled {dur} · {tool}`（en）/ `卡顿 {dur} · {tool}`（zh）。
/// idle_secs 是闲置秒数（now − last_tool_call）；时长口径与 format_dur 一致。
pub fn stalled_attr(idle_secs: u64, tool: &str, lang: crate::core::i18n::Language) -> String {
    tr(lang, "runtime.stalled_attr")
        .replace("{dur}", &format_dur(idle_secs))
        .replace("{tool}", tool)
}

pub struct AgentDetail {
    summary: Mutex<Option<TranscriptSummary>>,
}

impl AgentDetail {
    pub fn new() -> Self {
        Self { summary: Mutex::new(None) }
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
                    let now = crate::core::state::now_secs();
                    let is_stalled = is_stalled(agent, summary, now, stall_secs);
                    let status = if is_stalled {
                        let (r, g, b) = animation::breathe(&theme.danger, animation::now_phase(4.0));
                        ansi::ansi_fg("◐", &format!("#{:02x}{:02x}{:02x}", r, g, b))
                    } else {
                        ansi::ansi_fg("◐", &theme.success)
                    };
                    let name = ansi::ansi_fg(&ansi::truncate(&agent.name, 24), &theme.accent);
                    let task =
                        ansi::ansi_fg(&ansi::truncate(&agent.task_description, 40), &theme.muted);
                    let elapsed = summary
                        .last_event_secs
                        .map_or(0, |e| e.saturating_sub(agent.start_time_secs));
                    let elapsed_str = if summary.timestamps_reliable {
                        format_dur(elapsed)
                    } else {
                        format!("≈{}", format_dur(elapsed))
                    };
                    // ⑮ 卡顿归因：有最后工具名 → time 段替换为归因（danger 色）；
                    // 无工具记录 → 维持现状（elapsed）。
                    let time = if is_stalled {
                        match &agent.last_tool_name {
                            Some(tool) => {
                                let idle = agent
                                    .last_tool_call_secs
                                    .map_or(0, |t| now.saturating_sub(t));
                                ansi::ansi_fg(&stalled_attr(idle, tool, config.lang), &theme.danger)
                            }
                            None => ansi::ansi_fg(&elapsed_str, &theme.muted),
                        }
                    } else {
                        ansi::ansi_fg(&elapsed_str, &theme.muted)
                    };
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
        config: &WidgetConfig,
    ) {
        let is_stalled_anim = {
            let (r, g, b) = animation::breathe(&theme.danger, animation::now_phase(4.0));
            Color::Rgb(r, g, b)
        };

        let mut lines: Vec<Line> = vec![];
        lines.push(Line::from(Span::styled(
            "Agent Detail",
            Style::default().fg(ansi::parse_ratatui_color(&theme.accent)),
        )));

        let lock = self.summary.lock();
        if let Ok(ref guard) = lock {
            if let Some(ref summary) = **guard {
                for agent in &summary.agents {
                    let now = crate::core::state::now_secs();
                    let is_stalled =
                        is_stalled(agent, summary, now, config.get_u64("stall_threshold_sec", 30));
                    let status_color = if is_stalled {
                        is_stalled_anim
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
                lines.push(Line::from(tr(config.lang, "runtime.no_agent_data")));
            }
        } else {
            lines.push(Line::from(tr(config.lang, "runtime.no_agent_data")));
        }

        frame.render_widget(Paragraph::new(Text::from(lines)), area);
    }

    fn update_transcript(&self, summary: &TranscriptSummary) {
        if let Ok(ref mut guard) = self.summary.lock() {
            **guard = Some(summary.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::transcript::AgentRecord;

    fn summary(agents: Vec<AgentRecord>) -> TranscriptSummary {
        let mut s = TranscriptSummary::default();
        s.agents = agents;
        s
    }

    #[test]
    fn unreliable_session_elapsed_shows_approx_marker() {
        let mut s = summary(vec![AgentRecord {
            name: "a".into(),
            is_active: true,
            start_time_secs: 3,
            ..Default::default()
        }]);
        s.timestamps_reliable = false;
        let w = AgentDetail::new();
        w.update_transcript(&s);
        let out = w.render_compact(
            &SessionData::default(),
            &Theme::default(),
            &WidgetConfig::default(),
        );
        assert!(out.contains("≈"), "unreliable elapsed must be marked: {}", out);
    }

    #[test]
    fn reliable_session_elapsed_is_real_diff() {
        let mut s = summary(vec![AgentRecord {
            name: "a".into(),
            is_active: true,
            start_time_secs: 100,
            ..Default::default()
        }]);
        s.timestamps_reliable = true;
        s.last_event_secs = Some(160);
        let w = AgentDetail::new();
        w.update_transcript(&s);
        let out = w.render_compact(
            &SessionData::default(),
            &Theme::default(),
            &WidgetConfig::default(),
        );
        assert!(out.contains("1m0s"), "elapsed must be the real diff: {}", out);
        assert!(!out.contains("≈"), "reliable session must not be marked: {}", out);
    }

    #[test]
    fn is_stalled_requires_reliable_timeline_and_now() {
        let mut s = TranscriptSummary::default();
        s.timestamps_reliable = true;
        let agent = AgentRecord {
            name: "a".into(),
            is_active: true,
            last_tool_call_secs: Some(100),
            ..Default::default()
        };
        assert!(is_stalled(&agent, &s, 200, 30));
        assert!(!is_stalled(&agent, &s, 120, 30));
        s.timestamps_reliable = false;
        assert!(!is_stalled(&agent, &s, 200, 30));
    }

    #[test]
    fn stalled_attr_formats_duration_and_tool() {
        // idle 195s = 3m15s；en 文案 `stalled {dur} · {tool}`
        assert_eq!(
            stalled_attr(195, "bash", crate::core::i18n::Language::En),
            "stalled 3m15s · bash"
        );
        assert_eq!(
            stalled_attr(45, "Bash", crate::core::i18n::Language::En),
            "stalled 45s · Bash"
        );
    }

    #[test]
    fn stalled_with_tool_shows_attribution() {
        let mut s = summary(vec![AgentRecord {
            name: "a".into(),
            is_active: true,
            last_tool_call_secs: Some(100),
            last_tool_name: Some("bash".into()),
            ..Default::default()
        }]);
        s.timestamps_reliable = true;
        s.last_event_secs = Some(160);
        let w = AgentDetail::new();
        w.update_transcript(&s);
        let out = w.render_compact(
            &SessionData::default(),
            &Theme::default(),
            &WidgetConfig::default(),
        );
        // idle 依赖真实时钟（now − 100 巨大）→ 只断言稳定子串
        assert!(out.contains("stalled"), "attribution word: {}", out);
        assert!(out.contains("· bash"), "tool attribution: {}", out);
    }

    #[test]
    fn stalled_without_tool_keeps_plain_elapsed() {
        let mut s = summary(vec![AgentRecord {
            name: "a".into(),
            is_active: true,
            start_time_secs: 100,
            last_tool_call_secs: Some(100),
            last_tool_name: None,
            ..Default::default()
        }]);
        s.timestamps_reliable = true;
        s.last_event_secs = Some(160);
        let w = AgentDetail::new();
        w.update_transcript(&s);
        let out = w.render_compact(
            &SessionData::default(),
            &Theme::default(),
            &WidgetConfig::default(),
        );
        // 无工具记录 → 维持现状（elapsed 仍显示，无归因文本）
        assert!(out.contains("1m0s"), "elapsed unchanged: {}", out);
        assert!(!out.contains("stalled"), "no attribution: {}", out);
    }
}
