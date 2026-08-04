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
        let mut group = format!(
            "{}{}{:.2} · {}/{} tok",
            prefix,
            symbol,
            cost,
            format_tokens(t_in),
            format_tokens(t_out)
        );
        // ⑳ 预算占比：仅当配置了 cap 且成本 > 0 时显示（避免 0/0 噪音）。
        let budget_cap = config.get_f64("budget_cap_usd", 0.0);
        if budget_cap > 0.0 && cost > 0.0 {
            group.push_str(&format!(" · {:.0}%", (cost / budget_cap) * 100.0));
        }
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
    use super::{format_tokens, CostDisplay};
    use crate::core::session::SessionData;
    use crate::core::theme::Theme;
    use crate::core::widget::{Widget, WidgetConfig};

    fn session_data() -> SessionData {
        SessionData::from_stdin_json(
            r#"{"model":{"id":"m","display_name":"M"},
                "context_window":{"total_input_tokens":1000,"total_output_tokens":2000,
                                 "context_window_size":200000},
                "cost":{"total_cost_usd":0.0,"total_duration_ms":0}}"#,
        )
        .unwrap()
    }

    fn cfg(extra: &[(&str, &str)]) -> WidgetConfig {
        WidgetConfig {
            values: extra
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

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

    #[test]
    fn budget_pct_shown_when_configured() {
        let data = session_data();
        let theme = Theme::default();
        let config = cfg(&[
            ("effective_cost", "3.1"),
            ("cost_estimated", "true"),
            ("budget_cap_usd", "5.0"),
        ]);
        let out = CostDisplay.render_compact(&data, &theme, &config);
        assert!(out.contains("· 62%"), "got: {}", out);
        assert!(out.contains("≈$3.10"), "got: {}", out);
    }

    #[test]
    fn budget_pct_hidden_when_cap_zero() {
        let data = session_data();
        let theme = Theme::default();
        let config = cfg(&[("effective_cost", "3.1"), ("cost_estimated", "true")]);
        let out = CostDisplay.render_compact(&data, &theme, &config);
        assert!(!out.contains('%'), "got: {}", out);
    }

    #[test]
    fn zero_data_still_downgrades_to_dash() {
        let data = SessionData::from_stdin_json(
            r#"{"model":{"id":"m","display_name":"M"},
                "context_window":{"total_input_tokens":0,"total_output_tokens":0,
                                 "context_window_size":200000},
                "cost":{"total_cost_usd":0.0,"total_duration_ms":0}}"#,
        )
        .unwrap();
        let theme = Theme::default();
        let out = CostDisplay.render_compact(&data, &theme, &WidgetConfig::default());
        assert_eq!(out, "—");
    }
}
