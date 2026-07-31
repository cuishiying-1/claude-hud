use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Text;

use crate::core::ansi;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::widget::{Widget, WidgetConfig};

pub struct CostDisplay;

impl Widget for CostDisplay {
    fn id(&self) -> &str { "cost_display" }
    fn display_name(&self) -> &str { "Cost Display" }

    fn render_compact(&self, data: &SessionData, theme: &Theme, config: &WidgetConfig) -> String {
        let symbol = config.get_str("currency_symbol", "¥");
        let cost = data.cost.total_cost_usd;
        let warn = config.get_f64("warn_threshold_usd", 10.0);
        let color = if cost >= warn { &theme.warning } else { &theme.success };
        format!(
            "{}{:.2}{}",
            ansi::ansi_fg(&symbol, color),
            cost,
            ansi::ansi_reset()
        )
    }

    fn render_dashboard(&self, data: &SessionData, area: Rect, frame: &mut Frame, _theme: &Theme, _config: &WidgetConfig) {
        let dur = data.cost.total_duration_ms / 1000;
        let text = format!("Cost: ${:.4} | {}m {}s | +{}/-{} lines",
            data.cost.total_cost_usd, dur / 60, dur % 60, data.cost.total_lines_added, data.cost.total_lines_removed);
        frame.render_widget(Text::from(text), area);
    }
}
