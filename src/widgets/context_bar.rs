use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
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
        let color = if pct >= critical { &theme.danger } else if pct >= warn { &theme.warning } else { &theme.success };
        let filled_str = theme.bar_filled.to_string().repeat(filled);
        let empty_str = theme.bar_empty.to_string().repeat(empty);
        format!("ctx {}{}{} {:.0}% {}/{} tok",
            ansi::ansi_fg(&filled_str, color),
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
