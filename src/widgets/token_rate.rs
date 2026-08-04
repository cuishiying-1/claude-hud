use std::sync::Mutex;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::core::ansi;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::transcript::TranscriptSummary;
use crate::core::widget::{Widget, WidgetConfig};
use crate::widgets::cost_display::format_tokens;

/// 8 级块条（0 级 = 空格），盲文频谱风格。
const SPECTRUM_LEVELS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// 仪表盘最多绘制最近 24 桶（24 分钟窗口）。
const SPECTRUM_BUCKETS: usize = 24;

pub struct TokenRate {
    summary: Mutex<Option<TranscriptSummary>>,
}

impl TokenRate {
    pub fn new() -> Self {
        Self { summary: Mutex::new(None) }
    }
}

/// 速率 = 最近 60s 桶内 token 量（桶为 60s epoch，累计口径 → 尾桶减前桶增量；
/// 单桶时桶内累计即窗口量）。空 timeline → None。
pub fn rate_per_min(summary: &TranscriptSummary) -> Option<f64> {
    let tl = &summary.token_timeline;
    let last = tl.last()?;
    let prev_total = tl.get(tl.len().wrapping_sub(2)).map(|b| b.total_tokens).unwrap_or(0);
    Some(last.total_tokens.saturating_sub(prev_total) as f64)
}

/// 最近 max_buckets 桶归一化为 8 级块条；空 timeline → "—"。
pub fn spectrum_bars(timeline: &[crate::core::transcript::TokenSnapshot], max_buckets: usize) -> String {
    if timeline.is_empty() {
        return "—".to_string();
    }
    let start = timeline.len().saturating_sub(max_buckets);
    let buckets = &timeline[start..];
    let max = buckets.iter().map(|b| b.total_tokens).max().unwrap_or(1).max(1);
    buckets
        .iter()
        .map(|b| {
            let level = ((b.total_tokens as f64 / max as f64) * 8.0).round() as usize;
            SPECTRUM_LEVELS[level.min(8)]
        })
        .collect()
}

impl Widget for TokenRate {
    fn id(&self) -> &str { "token_rate" }

    fn display_name(&self) -> &str { "Token Rate" }

    fn render_compact(&self, _data: &SessionData, theme: &Theme, _config: &WidgetConfig) -> String {
        let guard = self.summary.lock().ok();
        let summary = guard.as_deref().and_then(|o| o.as_ref());
        let Some(rate) = summary.and_then(rate_per_min) else {
            return "—".to_string();
        };
        let rate_str = ansi::ansi_fg(&format!("{}/min", format_tokens(rate.round() as u64)), &theme.muted);
        format!("tok {}", rate_str)
    }

    fn render_dashboard(
        &self,
        _data: &SessionData,
        area: Rect,
        frame: &mut Frame,
        theme: &Theme,
        _config: &WidgetConfig,
    ) {
        let mut lines = vec![Line::from(Span::styled(
            "Token Rate",
            Style::default().fg(ansi::parse_ratatui_color(&theme.accent)),
        ))];
        let guard = self.summary.lock().ok();
        let summary = guard.as_deref().and_then(|o| o.as_ref());
        let bars = summary
            .map(|s| spectrum_bars(&s.token_timeline, SPECTRUM_BUCKETS))
            .unwrap_or_else(|| "—".to_string());
        lines.push(Line::from(bars));
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
    use crate::core::transcript::TokenSnapshot;

    fn snapshot(total: u64) -> TokenSnapshot {
        TokenSnapshot {
            timestamp_secs: 0,
            input_tokens: total,
            output_tokens: 0,
            total_tokens: total,
        }
    }

    #[test]
    fn rate_from_last_bucket_per_minute() {
        let mut s = TranscriptSummary::default();
        s.token_timeline.push(snapshot(3000));
        assert_eq!(rate_per_min(&s), Some(3000.0)); // 单桶 = 窗口内量
        let mut s2 = TranscriptSummary::default();
        s2.token_timeline.push(snapshot(3000));
        s2.token_timeline.push(snapshot(3100));
        assert_eq!(rate_per_min(&s2), Some(100.0)); // 尾桶增量 = 最近窗口量
    }

    #[test]
    fn rate_none_on_empty_timeline() {
        assert_eq!(rate_per_min(&TranscriptSummary::default()), None);
    }

    #[test]
    fn spectrum_normalizes_to_max() {
        assert_eq!(spectrum_bars(&[], 24), "—");
        assert_eq!(spectrum_bars(&[snapshot(0), snapshot(0)], 24), "  ");
        assert_eq!(spectrum_bars(&[snapshot(0), snapshot(100)], 24), " █");
        assert_eq!(spectrum_bars(&[snapshot(100)], 24), "█");
        assert_eq!(spectrum_bars(&[snapshot(50)], 24), "█"); // 单桶自归一化为满
    }

    #[test]
    fn spectrum_keeps_last_buckets_only() {
        let timeline: Vec<TokenSnapshot> = (0..30).map(|i| snapshot((i % 3) as u64)).collect();
        let bars = spectrum_bars(&timeline, 24);
        assert_eq!(bars.chars().count(), 24);
    }
}
