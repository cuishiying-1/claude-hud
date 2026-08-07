use std::sync::Mutex;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Gauge;

use crate::core::ansi;
use crate::core::i18n::{tr, Language};
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::transcript::TranscriptSummary;
use crate::core::widget::{Widget, WidgetConfig};

/// ④ 内部可变性：Widget trait 是 &self，transcript summary 用 Mutex 存
/// （token_rate 同款模式），render 时锁读做压缩外推。
pub struct ContextBar {
    summary: Mutex<Option<TranscriptSummary>>,
}

impl ContextBar {
    pub fn new() -> Self {
        Self { summary: Mutex::new(None) }
    }

    /// ④ 压缩预测（分钟）：复用 transcript::compaction_prediction（v0.1 斜率
    /// 模块）；时间轴不可靠 / 桶 <2 / 速率为 0 → None（诚实降级，不显示）。
    fn compaction_eta(&self, data: &SessionData) -> Option<u64> {
        self.summary
            .lock()
            .ok()
            .as_deref()
            .and_then(|o| o.as_ref())
            .and_then(|s| {
                s.compaction_prediction(
                    data.context_window.used_percentage,
                    data.context_window.context_window_size,
                )
            })
    }
}

impl Widget for ContextBar {
    fn id(&self) -> &str { "context_bar" }
    fn display_name(&self) -> &str { "Context Bar" }

    fn update_transcript(&self, summary: &TranscriptSummary) {
        *self.summary.lock().expect("context bar summary") = Some(summary.clone());
    }

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
        // 输入/输出标注：裸 `X/Y tok` 用户看不出哪个是哪个。
        let tokens = tr(config.lang, "widget.tokens_in_out")
            .replace("{in}", &format_k(data.context_window.total_input_tokens))
            .replace("{out}", &format_k(data.context_window.total_output_tokens));
        let mut out = format!("ctx {}{}{} {:.0}% {}",
            filled_str,
            ansi::ansi_fg(&empty_str, &theme.border),
            ansi::ansi_reset(),
            pct,
            tokens);
        // ④ 压缩预测标注（数据不足 → 无标注，诚实降级）。
        if let Some(m) = self.compaction_eta(data) {
            out.push_str(&format!(
                " · {}",
                ansi::ansi_fg(
                    &tr(config.lang, "widget.compaction_eta")
                        .replace("{m}", &m.to_string()),
                    &theme.muted,
                )
            ));
        }
        out
    }

    fn render_dashboard(&self, data: &SessionData, area: Rect, frame: &mut Frame, theme: &Theme, config: &WidgetConfig) {
        let pct = data.context_window.used_percentage;
        let used = data.context_window.total_input_tokens + data.context_window.total_output_tokens;
        let max = data.context_window.context_window_size;
        let warn = config.get_f64("warn_threshold", 80.0);
        let color = if pct >= 95.0 { ansi::parse_ratatui_color(&theme.danger) }
            else if pct >= warn { ansi::parse_ratatui_color(&theme.warning) }
            else { ansi::parse_ratatui_color(&theme.success) };
        // ④ dashboard 上下文卡片同样标注（数据不足 → 无标注）；
        // 无任何用量数据时降级为占位（上游不提供 usage，0/1M 是假精确）。
        let label = gauge_label(pct, used, max, config.lang);
        let label = if let Some(m) = self.compaction_eta(data) {
            format!(
                "{label} · {}",
                tr(config.lang, "widget.compaction_eta")
                    .replace("{m}", &m.to_string())
            )
        } else {
            label
        };
        frame.render_widget(
            Gauge::default().gauge_style(Style::default().fg(color))
                .ratio(pct / 100.0)
                .label(label),
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

/// Dashboard Gauge 标签：无任何用量数据（in+out 全 0）→ 「—」占位，
/// 否则 `68% — 136k/200k tokens`。
fn gauge_label(pct: f64, used: u64, max: u64, lang: Language) -> String {
    if used == 0 {
        tr(lang, "widget.no_data").to_string()
    } else {
        format!("{:.0}% — {}/{} tokens", pct, used, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::session::SessionData;
    use crate::core::transcript::{TokenSnapshot, TranscriptSummary};
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
            context_bar_present: false,
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
        let out = ContextBar::new().render_compact(&data, &Theme::default(), &cfg(true));
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
        let out = ContextBar::new().render_compact(&data, &Theme::default(), &cfg(false));
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
        let out = ContextBar::new().render_compact(&data, &Theme::default(), &cfg(true));
        assert!(out.contains("ctx "), "empty bar still renders: {}", out);
    }

    #[test]
    fn compact_eta_shown_when_prediction_available() {
        let data = session_data(68.0);
        let bar = ContextBar::new();
        let mut s = TranscriptSummary::default();
        s.timestamps_reliable = true;
        s.token_timeline.push(TokenSnapshot { timestamp_secs: 0, input_tokens: 100, output_tokens: 50, total_tokens: 150 });
        s.token_timeline.push(TokenSnapshot { timestamp_secs: 60, input_tokens: 300, output_tokens: 130, total_tokens: 430 });
        bar.update_transcript(&s);
        let out = bar.render_compact(&data, &Theme::default(), &cfg(true));
        // window=200000 (session_data 构造) → remaining 64000, rate 280/60 → 228m
        assert!(out.contains("compact ≈228m"), "eta text: {}", out);
    }

    #[test]
    fn compact_eta_hidden_when_insufficient_data() {
        let data = session_data(68.0);
        let bar = ContextBar::new();
        let s = TranscriptSummary::default(); // 空 timeline → 无预测
        bar.update_transcript(&s);
        let out = bar.render_compact(&data, &Theme::default(), &cfg(true));
        assert!(!out.contains("compact ≈"), "no eta text: {}", out);
    }

    #[test]
    fn compact_tokens_labelled_in_out() {
        // in=1000, out=2000（session_data 构造）→ 标注输入/输出，用户可读
        let data = session_data(44.0);
        let out = ContextBar::new().render_compact(&data, &Theme::default(), &cfg(true));
        assert!(out.contains("1.0k in / 2.0k out tok"), "labelled tokens: {}", out);
        assert!(!out.contains("/2.0k tok"), "no bare x/y tok: {}", out);
    }

    #[test]
    fn compact_tokens_labelled_zh() {
        let data = session_data(44.0);
        let mut config = cfg(true);
        config.lang = crate::core::i18n::Language::Zh;
        let out = ContextBar::new().render_compact(&data, &Theme::default(), &config);
        assert!(out.contains("输入 1.0k / 输出 2.0k tok"), "zh labelled tokens: {}", out);
    }

    #[test]
    fn gauge_label_placeholder_when_no_token_data() {
        assert_eq!(gauge_label(0.0, 0, 1_000_000, Language::Zh), "—");
        assert_eq!(gauge_label(0.0, 0, 1_000_000, Language::En), "—");
    }

    #[test]
    fn gauge_label_shows_pct_and_tokens_when_data_present() {
        assert_eq!(
            gauge_label(3.4, 11_800, 200_000, Language::En),
            "3% — 11800/200000 tokens"
        );
    }
}
