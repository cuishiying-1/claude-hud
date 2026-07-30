use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Gauge;

use crate::core::ansi;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::widget::{Widget, WidgetConfig};

pub struct RateLimits;

impl Widget for RateLimits {
    fn id(&self) -> &str { "rate_limits" }
    fn display_name(&self) -> &str { "Rate Limits" }

    fn render_compact(&self, data: &SessionData, theme: &Theme, config: &WidgetConfig) -> String {
        let fh = data.rate_limits.five_hour.used_percentage;
        let sd = data.rate_limits.seven_day.used_percentage;
        let warn = config.get_f64("rate_limit_warn", 90.0);
        let fc = if fh >= warn { &theme.danger } else { &theme.success };
        let sc = if sd >= warn { &theme.danger } else { &theme.success };
        format!("5h:{}{:.0}%{} 7d:{}{:.0}%{}",
            ansi::ansi_fg("", fc), fh, ansi::ansi_reset(),
            ansi::ansi_fg("", sc), sd, ansi::ansi_reset())
    }

    fn render_dashboard(&self, data: &SessionData, area: Rect, frame: &mut Frame, theme: &Theme, _config: &WidgetConfig) {
        let fh = data.rate_limits.five_hour.used_percentage;
        let (sr, sg, sb) = Theme::parse_hex(&theme.success).unwrap_or((0, 255, 0));
        frame.render_widget(
            Gauge::default()
                .gauge_style(Style::default().fg(Color::Rgb(sr, sg, sb)))
                .ratio(fh / 100.0)
                .label(format!("5h: {:.0}%  7d: {:.0}%", fh, data.rate_limits.seven_day.used_percentage)),
            area);
    }
}
