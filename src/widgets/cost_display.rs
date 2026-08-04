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
        let symbol = config.get_str("currency_symbol", "$");
        let cost = config.get_f64("effective_cost", data.cost.total_cost_usd);
        let estimated = config.get_bool("cost_estimated", false);
        let t_in = data.context_window.total_input_tokens;
        let t_out = data.context_window.total_output_tokens;
        // ⑲ 诚实降级：无任何成本/用量数据 → —（网关无 usage/cost，不显示 $0.00 假精确）
        if cost == 0.0 && t_in == 0 && t_out == 0 && !estimated {
            return "—".to_string();
        }
        let warn = config.get_f64("warn_threshold_usd", 10.0);
        let color = if cost >= warn { &theme.warning } else { &theme.success };
        let prefix = if estimated { "≈" } else { "" };
        let group = format!(
            "{}{}{:.2} · {}/{} tok",
            prefix,
            symbol,
            cost,
            format_tokens(t_in),
            format_tokens(t_out)
        );
        ansi::ansi_fg(&group, color)
    }

    fn render_dashboard(&self, data: &SessionData, area: Rect, frame: &mut Frame, _theme: &Theme, config: &WidgetConfig) {
        let dur = data.cost.total_duration_ms / 1000;
        let mut text = format!("Cost: ${:.4} | {}m {}s | +{}/-{} lines",
            data.cost.total_cost_usd, dur / 60, dur % 60, data.cost.total_lines_added, data.cost.total_lines_removed);
        // ⑲ 未命中 [pricing] → 完整数据视图标注（命中时省略）
        if !config.get_bool("pricing_configured", false) {
            text.push_str(&format!(" | 未配置单价 (model.id: {})", data.model.id));
        }
        frame.render_widget(Text::from(text), area);
    }
}

/// ⑲ k 缩写（spec 样例口径）：≥100k 去小数防溢出；≥1k 一位小数；否则原数。
pub fn format_tokens(n: u64) -> String {
    if n >= 100_000 {
        format!("{}k", n / 1000)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::format_tokens;

    #[test]
    fn tokens_k_abbreviation() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1000), "1.0k");
        assert_eq!(format_tokens(6800), "6.8k");
        assert_eq!(format_tokens(5000), "5.0k");
        assert_eq!(format_tokens(12345), "12.3k");
        assert_eq!(format_tokens(100_000), "100k");
        assert_eq!(format_tokens(450_000), "450k");
    }
}
