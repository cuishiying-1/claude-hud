use std::sync::Mutex;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Text;

use crate::core::ansi;
use crate::core::i18n::tr;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::widget::{Widget, WidgetConfig};

const EASE_DURATION: f64 = 0.8;

/// 仪表盘缓动计数器（唯一进程内动画状态）：target 变化重置锚点，
/// ease_out 曲线 0.8s 内从当前显示值滚到新值。
struct EasedValue {
    target: f64,
    start: f64,
    elapsed: f64,
}

impl EasedValue {
    fn new() -> Self {
        Self { target: 0.0, start: 0.0, elapsed: 0.0 }
    }

    /// 帧推进：delta = 距上帧秒数；target 变化 → 以当前显示值为锚点重置。
    fn tick(&mut self, target: f64, delta: f64) -> f64 {
        if self.target != target {
            self.start = self.value();
            self.target = target;
            self.elapsed = 0.0;
        }
        self.elapsed = (self.elapsed + delta.max(0.0)).min(EASE_DURATION);
        self.value()
    }

    fn value(&self) -> f64 {
        self.start + (self.target - self.start) * crate::core::animation::ease_out(self.elapsed / EASE_DURATION)
    }
}

pub struct CostDisplay {
    eased: Mutex<EasedValue>,
    last_frame: Mutex<std::time::Instant>,
}

impl CostDisplay {
    pub fn new() -> Self {
        Self {
            eased: Mutex::new(EasedValue::new()),
            last_frame: Mutex::new(std::time::Instant::now()),
        }
    }
}

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
        if no_cost_data(cost, t_in, t_out) && !estimated {
            return "—".to_string();
        }
        let warn = config.get_f64("warn_threshold_usd", 10.0);
        let color = if cost >= warn { &theme.warning } else { &theme.success };
        let prefix = if estimated { "≈" } else { "" };
        // token 段与 context_bar 同屏时重复（context_bar 已展示用量）。
        // 显式 show_tokens 键优先；未配置时布局含 context_bar 自动隐藏，
        // 无 context_bar 的极简布局保持默认开。
        let show_tokens = match config.values.get("show_tokens") {
            Some(v) => v == "true",
            None => !config.context_bar_present,
        };
        let mut group = format!("{}{}{:.2}", prefix, symbol, cost);
        if show_tokens {
            // 输入/输出标注（与 context_bar 同 key，避免裸 X/Y tok 语义不明）
            group.push_str(&format!(
                " · {}",
                crate::core::i18n::tr(config.lang, "widget.tokens_in_out")
                    .replace("{in}", &format_tokens(t_in))
                    .replace("{out}", &format_tokens(t_out))
            ));
        }
        // ③ 成本速率：成本 ÷ 活跃时长（小时）。零时长/零成本 → 不显示（诚实降级）。
        let duration_ms = data.cost.total_duration_ms;
        if cost > 0.0 && duration_ms > 0 {
            let hours = duration_ms as f64 / 3_600_000.0;
            let rate = cost / hours;
            group.push_str(&format!(" · {}{}{:.1}/h", prefix, symbol, rate));
        }
        // ⑳ 预算占比：仅当配置了 cap 且成本 > 0 时显示（避免 0/0 噪音）。
        let budget_cap = config.get_f64("budget_cap_usd", 0.0);
        if budget_cap > 0.0 && cost > 0.0 {
            group.push_str(&format!(" · {:.0}%", (cost / budget_cap) * 100.0));
        }
        ansi::ansi_fg(&group, color)
    }

    fn render_dashboard(&self, data: &SessionData, area: Rect, frame: &mut Frame, _theme: &Theme, config: &WidgetConfig) {
        // ⑲ dashboard 同款诚实降级：无成本/用量 → —（不显示 ¥0.0000 假精确）
        if no_cost_data(
            data.cost.total_cost_usd,
            data.context_window.total_input_tokens,
            data.context_window.total_output_tokens,
        ) {
            frame.render_widget(Text::from("—"), area);
            return;
        }
        let now = std::time::Instant::now();
        let delta = now
            .duration_since(*self.last_frame.lock().expect("frame clock"))
            .as_secs_f64();
        *self.last_frame.lock().expect("frame clock") = now;
        let display_cost = self.eased.lock().expect("eased value").tick(data.cost.total_cost_usd, delta);
        let dur = data.cost.total_duration_ms / 1000;
        let symbol = config.get_str("currency_symbol", "$");
        let mut text = format!(
            "{}: {}{:.4} | {}m {}s | +{}/-{} {}",
            tr(config.lang, "runtime.cost_title"),
            symbol,
            display_cost,
            dur / 60,
            dur % 60,
            data.cost.total_lines_added,
            data.cost.total_lines_removed,
            tr(config.lang, "runtime.lines")
        );
        // ⑲ 未命中 [pricing] → 完整数据视图标注（命中时省略）
        if !config.get_bool("pricing_configured", false) {
            text.push_str(&format!(" | {} (model.id: {})", tr(config.lang, "runtime.no_pricing"), data.model.id));
        }
        frame.render_widget(Text::from(text), area);
    }
}

/// ⑲ 诚实降级判定：无任何成本且无任何用量数据（网关不提供 usage/cost）。
fn no_cost_data(cost: f64, t_in: u64, t_out: u64) -> bool {
    cost == 0.0 && t_in == 0 && t_out == 0
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
    use super::{format_tokens, no_cost_data, CostDisplay, EasedValue};
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
            lang: crate::core::i18n::Language::En,
            context_bar_present: false,
        }
    }

    fn cfg_with_layout(extra: &[(&str, &str)], context_bar_present: bool) -> WidgetConfig {
        let mut c = cfg(extra);
        c.context_bar_present = context_bar_present;
        c
    }

    fn session_with_duration(ms: u64) -> SessionData {
        SessionData::from_stdin_json(
            &format!(
                r#"{{"model":{{"id":"m","display_name":"M"}},
                    "context_window":{{"total_input_tokens":1000,"total_output_tokens":2000,
                                     "context_window_size":200000}},
                    "cost":{{"total_cost_usd":0.0,"total_duration_ms":{}}}}}"#,
                ms
            ),
        )
        .unwrap()
    }

    #[test]
    fn rate_shown_when_duration_and_cost_present() {
        let data = session_with_duration(600_000); // 10min
        let theme = Theme::default();
        let config = cfg(&[("effective_cost", "1.8"), ("cost_estimated", "true")]);
        let out = CostDisplay::new().render_compact(&data, &theme, &config);
        // 1.8 / (10/60)h = 10.8
        assert!(out.contains("≈$10.8/h"), "rate segment: {}", out);
    }

    #[test]
    fn rate_hidden_when_duration_zero() {
        let data = session_data(); // duration 0
        let theme = Theme::default();
        let config = cfg(&[("effective_cost", "1.8"), ("cost_estimated", "true")]);
        let out = CostDisplay::new().render_compact(&data, &theme, &config);
        assert!(!out.contains("/h"), "no rate segment: {}", out);
    }

    #[test]
    fn rate_hidden_when_cost_zero() {
        let data = session_with_duration(600_000);
        let theme = Theme::default();
        let config = cfg(&[("effective_cost", "0"), ("cost_estimated", "false")]);
        let out = CostDisplay::new().render_compact(&data, &theme, &config);
        assert!(!out.contains("/h"), "no rate segment: {}", out);
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
        let out = CostDisplay::new().render_compact(&data, &theme, &config);
        assert!(out.contains("· 62%"), "got: {}", out);
        assert!(out.contains("≈$3.10"), "got: {}", out);
    }

    #[test]
    fn tokens_hidden_when_show_tokens_false() {
        let data = session_with_duration(600_000);
        let theme = Theme::default();
        let config = cfg(&[
            ("effective_cost", "1.8"),
            ("cost_estimated", "true"),
            ("show_tokens", "false"),
        ]);
        let out = CostDisplay::new().render_compact(&data, &theme, &config);
        assert!(!out.contains("tok"), "no token segment: {}", out);
        assert!(out.contains("≈$10.8/h"), "rate kept: {}", out);
    }

    #[test]
    fn no_cost_data_true_only_when_cost_and_tokens_all_zero() {
        assert!(no_cost_data(0.0, 0, 0));
        assert!(!no_cost_data(0.5, 0, 0), "cost present");
        assert!(!no_cost_data(0.0, 100, 0), "input tokens present");
        assert!(!no_cost_data(0.0, 0, 7), "output tokens present");
    }

    #[test]
    fn tokens_shown_by_default() {
        let data = session_data();
        let theme = Theme::default();
        let out = CostDisplay::new().render_compact(&data, &theme, &WidgetConfig::default());
        assert!(out.contains("1.0k in / 2.0k out tok"), "labelled tokens by default: {}", out);
    }

    #[test]
    fn tokens_auto_hidden_when_context_bar_in_layout() {
        let data = session_data();
        let theme = Theme::default();
        let config = cfg_with_layout(&[], true);
        let out = CostDisplay::new().render_compact(&data, &theme, &config);
        assert!(!out.contains("tok"), "auto dedup with context_bar: {}", out);
    }

    #[test]
    fn explicit_show_tokens_true_wins_over_layout() {
        let data = session_data();
        let theme = Theme::default();
        let config = cfg_with_layout(&[("show_tokens", "true")], true);
        let out = CostDisplay::new().render_compact(&data, &theme, &config);
        assert!(out.contains("1.0k in / 2.0k out tok"), "explicit true wins: {}", out);
    }

    #[test]
    fn explicit_show_tokens_false_wins_over_no_context_bar() {
        let data = session_data();
        let theme = Theme::default();
        let config = cfg_with_layout(&[("show_tokens", "false")], false);
        let out = CostDisplay::new().render_compact(&data, &theme, &config);
        assert!(!out.contains("tok"), "explicit false wins: {}", out);
    }

    #[test]
    fn budget_pct_hidden_when_cap_zero() {
        let data = session_data();
        let theme = Theme::default();
        let config = cfg(&[("effective_cost", "3.1"), ("cost_estimated", "true")]);
        let out = CostDisplay::new().render_compact(&data, &theme, &config);
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
        let out = CostDisplay::new().render_compact(&data, &theme, &WidgetConfig::default());
        assert_eq!(out, "—");
    }

    #[test]
    fn ease_reaches_target_after_duration() {
        let mut v = EasedValue::new();
        assert_eq!(v.tick(100.0, 0.0), 0.0);
        assert!((v.tick(100.0, 0.4) - 75.0).abs() < 0.001, "t=0.5 → ease 0.75");
        assert_eq!(v.tick(100.0, 0.4), 100.0); // elapsed clamp 0.8 → 1.0
    }

    #[test]
    fn target_change_resets_anchor_to_current_display() {
        let mut v = EasedValue::new();
        v.tick(100.0, 0.8); // settle at 100
        assert_eq!(v.tick(50.0, 0.0), 100.0); // 锚点 = 当前显示值，未开始移动
        assert!((v.tick(50.0, 0.4) - 62.5).abs() < 0.001); // 100→50, t=0.5 → ease 0.75 → 62.5
        assert_eq!(v.tick(50.0, 0.4), 50.0);
    }

    #[test]
    fn negative_delta_clamped() {
        let mut v = EasedValue::new();
        v.tick(100.0, -1.0);
        assert_eq!(v.tick(100.0, 0.0), 0.0); // elapsed 不倒退
    }
}
