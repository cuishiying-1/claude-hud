use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Gauge;

use crate::core::ansi;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::widget::{Widget, WidgetConfig};

pub struct ContextBar;

impl Widget for ContextBar {
    fn id(&self) -> &str { "context_bar" }
    fn display_name(&self) -> &str { "Context Bar" }

    fn render_compact(&self, data: &SessionData, theme: &Theme, config: &WidgetConfig) -> String {
        let pct = data.context_window.used_percentage;
        let bar_width = config.get_u64("bar_width", theme.bar_width as u64) as usize;
        let filled = ((pct / 100.0) * (bar_width as f64)).round() as usize;
        let filled = filled.min(bar_width);
        let empty = bar_width - filled;
        let warn = config.get_f64("warn_threshold", 80.0);
        let critical = config.get_f64("critical_threshold", 95.0);
        let gradient_on = config.get_bool("gradient", true);
        let filled_str = if gradient_on && filled > 0 {
            let mut s = String::new();
            for i in 0..filled {
                let t = i as f64 / (bar_width.saturating_sub(1) as f64).max(1.0);
                let (r, g, b) = crate::core::animation::gradient(&theme.success, &theme.danger, t);
                s.push_str(&ansi::ansi_fg(
                    &theme.bar_filled.to_string(),
                    &format!("#{:02x}{:02x}{:02x}", r, g, b),
                ));
            }
            s
        } else {
            let color = if pct >= critical {
                &theme.danger
            } else if pct >= warn {
                &theme.warning
            } else {
                &theme.success
            };
            ansi::ansi_fg(&theme.bar_filled.to_string().repeat(filled), color)
        };
        let empty_str = theme.bar_empty.to_string().repeat(empty);
        format!("ctx {}{}{} {:.0}% {}/{} tok",
            filled_str,
            ansi::ansi_fg(&empty_str, &theme.border),
            ansi::ansi_reset(),
            pct,
            format_k(data.context_window.total_input_tokens),
            format_k(data.context_window.total_output_tokens))
    }

    fn render_dashboard(&self, data: &SessionData, area: Rect, frame: &mut Frame, theme: &Theme, config: &WidgetConfig) {
        let pct = data.context_window.used_percentage;
        let used = data.context_window.total_input_tokens + data.context_window.total_output_tokens;
        let max = data.context_window.context_window_size;
        let warn = config.get_f64("warn_threshold", 80.0);
        let color = if pct >= 95.0 { ansi::parse_ratatui_color(&theme.danger) }
            else if pct >= warn { ansi::parse_ratatui_color(&theme.warning) }
            else { ansi::parse_ratatui_color(&theme.success) };
        frame.render_widget(
            Gauge::default().gauge_style(Style::default().fg(color))
                .ratio(pct / 100.0)
                .label(format!("{:.0}% — {}/{} tokens", pct, used, max)),
            area);
    }
}

/// k 缩写：≥1000 时 x.xk（12.3k），否则原样。
fn format_k(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::session::SessionData;
    use crate::core::widget::{Widget, WidgetConfig};

    fn session_data(pct: f64) -> SessionData {
        SessionData::from_stdin_json(
            &format!(
                r#"{{"model":{{"id":"m","display_name":"M"}},
                    "context_window":{{"used_percentage":{},"total_input_tokens":1000,
                                     "total_output_tokens":2000,"context_window_size":200000}},
                    "cost":{{"total_cost_usd":0.0,"total_duration_ms":0}}}}"#,
                pct
            ),
        )
        .unwrap()
    }

    fn cfg(gradient: bool) -> WidgetConfig {
        WidgetConfig {
            values: [
                ("bar_width".to_string(), "4".to_string()),
                ("gradient".to_string(), gradient.to_string()),
            ]
            .into_iter()
            .collect(),
            lang: crate::core::i18n::Language::En,
        }
    }

    /// 统计输出中不同的 truecolor 色码（38;2;R;G;B）。
    fn distinct_colors(out: &str) -> Vec<&str> {
        let mut v: Vec<&str> = Vec::new();
        for part in out.split("\x1b[") {
            if let Some(code) = part.strip_prefix("38;2;") {
                let end = code.find('m').unwrap_or(code.len());
                let c = &code[..end];
                if !v.contains(&c) {
                    v.push(c);
                }
            }
        }
        v
    }

    #[test]
    fn gradient_on_produces_multiple_colors() {
        let data = session_data(90.0);
        let out = ContextBar.render_compact(&data, &Theme::default(), &cfg(true));
        let colors = distinct_colors(&out);
        assert!(
            colors.len() >= 3,
            "gradient on must yield >=3 distinct colors (cells + border), got {:?}: {}",
            colors, out
        );
        assert!(colors.contains(&"163;190;140"), "start cell = success: {}", out);
        assert!(colors.contains(&"191;97;106"), "end cell = danger: {}", out);
    }

    #[test]
    fn gradient_off_uses_single_filled_color() {
        let data = session_data(97.0);
        let out = ContextBar.render_compact(&data, &Theme::default(), &cfg(false));
        let colors = distinct_colors(&out);
        assert!(
            colors.len() <= 2,
            "gradient off must yield at most 2 colors (filled + border), got {:?}: {}",
            colors, out
        );
        assert!(colors.contains(&"191;97;106"), "pct 97 >= critical 95 → danger: {}", out);
    }

    #[test]
    fn gradient_empty_bar_no_crash() {
        let data = session_data(3.4); // filled = round(3.4/100*4) = 0
        let out = ContextBar.render_compact(&data, &Theme::default(), &cfg(true));
        assert!(out.contains("ctx "), "empty bar still renders: {}", out);
    }
}
