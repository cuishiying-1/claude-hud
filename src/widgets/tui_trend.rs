use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::core::ansi;
use crate::core::history::HistoryStore;
use crate::core::i18n::tr;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::widget::{Widget, WidgetConfig};

pub struct TuiTrend;

/// ⑪ 趋势面板文本行：近 7 天成本柱状（固定 8 行柱区 + 1 行日期标签，
/// 标签取首/中/尾三日去重）。空输入 → 占位「—」；全零成本 → 无柱（仅标签行）。
pub fn trend_lines(days: &[(String, f64)], width: u16) -> Vec<String> {
    if days.is_empty() {
        return vec!["—".to_string()];
    }
    let n = days.len();
    let max = days.iter().map(|(_, c)| *c).fold(0.0, f64::max).max(0.0001);
    let bar_rows = 8usize;
    let col_w = ((width as usize).saturating_sub(1) / n).max(1);
    let cols = col_w * n;
    let mut grid: Vec<Vec<char>> = vec![vec![' '; cols]; bar_rows];
    for (i, (_, cost)) in days.iter().enumerate() {
        let h = ((cost / max) * bar_rows as f64).round() as usize;
        for r in 0..h.min(bar_rows) {
            let row = bar_rows - 1 - r;
            for c in 0..col_w {
                grid[row][i * col_w + c] = '█';
            }
        }
    }
    let mut lines: Vec<String> = grid
        .into_iter()
        .map(|row| row.into_iter().collect())
        .collect();
    let mut label_row: Vec<char> = vec![' '; cols];
    let mut last: Option<usize> = None;
    for idx in [0usize, n / 2, n - 1] {
        if last == Some(idx) {
            continue;
        }
        last = Some(idx);
        let short = days[idx].0.get(5..).unwrap_or(&days[idx].0);
        let x = idx * col_w + col_w.saturating_sub(short.chars().count()) / 2;
        for (k, ch) in short.chars().enumerate() {
            if x + k < cols {
                label_row[x + k] = ch;
            }
        }
    }
    lines.push(label_row.into_iter().collect());
    lines
}

impl Widget for TuiTrend {
    fn id(&self) -> &str {
        "tui_trend"
    }

    fn display_name(&self) -> &str {
        "Trend"
    }

    /// dashboard-only：紧凑模式输出空串（用户若将其加入 compact_layout 不会报错）。
    fn render_compact(
        &self,
        _data: &SessionData,
        _theme: &Theme,
        _config: &WidgetConfig,
    ) -> String {
        String::new()
    }

    fn render_dashboard(
        &self,
        _data: &SessionData,
        area: Rect,
        frame: &mut Frame,
        theme: &Theme,
        config: &WidgetConfig,
    ) {
        let days = HistoryStore::open()
            .ok()
            .and_then(|h| h.daily_cost_trend().ok())
            .unwrap_or_default();
        let mut text = Text::default();
        text.push_line(Line::from(Span::styled(
            tr(config.lang, "web.cost_trend"),
            Style::default().fg(ansi::parse_ratatui_color(&theme.accent)),
        )));
        for line in trend_lines(&days, area.width.saturating_sub(2)) {
            text.push_line(Line::from(line));
        }
        frame.render_widget(Paragraph::new(text), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trend_lines_empty_placeholder() {
        let lines = trend_lines(&[], 60);
        assert_eq!(lines, vec!["—".to_string()]);
    }

    #[test]
    fn trend_lines_bars_and_labels() {
        let days = vec![
            ("2026-08-01".to_string(), 1.0),
            ("2026-08-02".to_string(), 3.0),
            ("2026-08-03".to_string(), 2.0),
        ];
        let lines = trend_lines(&days, 30);
        assert_eq!(lines.len(), 9); // 8 柱行 + 1 标签行
        assert!(lines.iter().any(|l| l.contains('█')));
        assert!(lines[8].contains("08-01"));
        assert!(lines[8].contains("08-03"));
    }

    #[test]
    fn trend_lines_zero_cost_no_bars() {
        let days = vec![
            ("2026-08-01".to_string(), 0.0),
            ("2026-08-02".to_string(), 0.0),
        ];
        let lines = trend_lines(&days, 30);
        assert_eq!(lines.len(), 9);
        assert!(lines.iter().all(|l| !l.contains('█')));
    }

    #[test]
    fn trend_lines_single_day_full_bar() {
        let days = vec![("2026-08-01".to_string(), 5.0)];
        let lines = trend_lines(&days, 30);
        assert_eq!(lines.len(), 9);
        assert!(lines[0].contains('█'));
        assert!(lines[8].contains("08-01"));
    }
}
