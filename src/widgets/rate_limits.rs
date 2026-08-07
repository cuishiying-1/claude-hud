use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Gauge;

use crate::core::ansi;
use crate::core::i18n::tr;
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
        // 代理未提供 rate_limits（如 DeepSeek）→ 诚实占位而非伪精确 0%
        if fh == 0.0 && sd == 0.0 {
            let dash = ansi::ansi_fg("—", &theme.muted);
            return format!("5h:{dash} 7d:{dash}");
        }
        let fc = if fh >= warn { &theme.danger } else { &theme.success };
        let sc = if sd >= warn { &theme.danger } else { &theme.success };
        format!("5h:{} 7d:{}",
            ansi::ansi_fg(&format!("{:.0}%", fh), fc),
            ansi::ansi_fg(&format!("{:.0}%", sd), sc))
    }

    fn render_dashboard(&self, data: &SessionData, area: Rect, frame: &mut Frame, theme: &Theme, config: &WidgetConfig) {
        let fh = data.rate_limits.five_hour.used_percentage;
        let sd = data.rate_limits.seven_day.used_percentage;
        let (sr, sg, sb) = Theme::parse_hex(&theme.success).unwrap_or((0, 255, 0));
        // 代理未提供 rate_limits → 语言提示占位（与 compact 诚实降级同口径）
        let label = if fh == 0.0 && sd == 0.0 {
            tr(config.lang, "runtime.rate_not_provided").to_string()
        } else {
            format!("5h: {:.0}%  7d: {:.0}%", fh, sd)
        };
        frame.render_widget(
            Gauge::default()
                .gauge_style(Style::default().fg(Color::Rgb(sr, sg, sb)))
                .ratio(fh / 100.0)
                .label(label),
            area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::theme::Theme;

    #[test]
    fn both_zero_shows_dash_not_fake_zero_percent() {
        let data = SessionData::default();
        let out = RateLimits.render_compact(&data, &Theme::default(), &WidgetConfig::default());
        assert!(out.contains("—"), "代理未提供 → 诚实占位: {}", out);
        assert!(!out.contains('%'), "不得显示伪精确 0%: {}", out);
    }

    #[test]
    fn any_nonzero_shows_percent() {
        let mut data = SessionData::default();
        data.rate_limits.five_hour.used_percentage = 12.5;
        let out = RateLimits.render_compact(&data, &Theme::default(), &WidgetConfig::default());
        assert!(out.contains("12%"), "{}", out);
    }
}
