//! 多窗口列表布局(`windows`):扫描 windows/ 目录,每窗口一行。
//! 数据来自目录扫描而非单会话灌入,故不注册进 WidgetRegistry。

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table},
    Frame,
};

use crate::core::{
    config::AppConfig,
    state,
    theme::Theme,
    windows::{self, WindowInfo, WindowStatus},
};
use crate::core::i18n::{tr, Language};

/// 渲染多窗口列表(全屏区域)。
pub fn draw(frame: &mut Frame, area: Rect, theme: &Theme, config: &AppConfig) {
    let lang = config.language();
    let wins = windows::scan_windows(state::now_secs());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(crate::core::ansi::parse_ratatui_color(
            &theme.border,
        )));
    let inner = block.inner(area);
    if wins.is_empty() {
        frame.render_widget(
            block.title(Line::from(vec![
                Span::styled(
                    tr(lang, "web.windows_title"),
                    Style::default().fg(crate::core::ansi::parse_ratatui_color(&theme.accent)),
                )
            ])),
            area,
        );
        let empty = Paragraph::new(tr(lang, "web.win_none").to_string());
        frame.render_widget(empty, inner);
        return;
    }
    let symbol = config.currency();
    let headers = [
        tr(lang, "web.w_col_status"),
        tr(lang, "web.w_col_dir"),
        tr(lang, "web.w_col_model"),
        tr(lang, "web.w_col_ctx"),
        tr(lang, "web.w_col_cost"),
        tr(lang, "web.w_col_tokens"),
        tr(lang, "web.w_col_agents"),
    ];
    let rows: Vec<Row> = wins
        .iter()
        .map(|w| {
            Row::new(format_row(w, symbol, lang)).style(window_style(w, theme))
        })
        .collect();
    let widths = [
        ratatui::layout::Constraint::Length(10),
        ratatui::layout::Constraint::Min(16),
        ratatui::layout::Constraint::Length(12),
        ratatui::layout::Constraint::Length(6),
        ratatui::layout::Constraint::Length(10),
        ratatui::layout::Constraint::Length(12),
        ratatui::layout::Constraint::Length(8),
    ];
    let table = Table::new(rows, widths)
        .header(Row::new(headers).style(
            Style::default()
                .fg(crate::core::ansi::parse_ratatui_color(&theme.accent))
                .add_modifier(Modifier::BOLD),
        ))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(
                    crate::core::ansi::parse_ratatui_color(&theme.border),
                ))
                .title(Line::from(vec![
                    Span::styled(
                        tr(lang, "web.windows_title"),
                        Style::default().fg(crate::core::ansi::parse_ratatui_color(
                            &theme.accent,
                        )),
                    )
                ])),
        );
    frame.render_widget(table, area);
}

/// 单窗口行:状态 / 目录 / 模型 / 上下文% / 成本 / token / 代理。
pub fn format_row(w: &WindowInfo, symbol: &str, lang: Language) -> Vec<String> {
    let status = match &w.status {
        WindowStatus::Active => tr(lang, "runtime.t_win_active").to_string(),
        WindowStatus::Idle => tr(lang, "runtime.t_win_idle").to_string(),
        WindowStatus::Ended => tr(lang, "runtime.t_win_ended").to_string(),
    };
    let name = if w.corrupt {
        tr(lang, "runtime.t_win_corrupt").to_string()
    } else {
        w.dir_name.clone()
    };
    vec![
        status,
        name,
        w.model.clone(),
        format!("{:.0}%", w.used_pct),
        format!("{}{:.2}", symbol, w.cost),
        format!("{:.1}k", (w.tokens_in + w.tokens_out) as f64 / 1000.0),
        w.agent_count.to_string(),
    ]
}

fn window_style(w: &WindowInfo, theme: &Theme) -> Style {
    let color = match &w.status {
        WindowStatus::Active => &theme.success,
        WindowStatus::Idle => &theme.warning,
        WindowStatus::Ended => &theme.muted,
    };
    Style::default().fg(crate::core::ansi::parse_ratatui_color(color))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(status: WindowStatus) -> WindowInfo {
        WindowInfo {
            key: "k".to_string(),
            dir_name: "proj-a".to_string(),
            status,
            model: "op-us".to_string(),
            used_pct: 42.0,
            tokens_in: 1000,
            tokens_out: 500,
            cost: 1.25,
            agent_count: 2,
            ts: 0,
            corrupt: false,
        }
    }

    #[test]
    fn format_row_contains_all_columns() {
        let lang = Language::En;
        let row = format_row(&win(WindowStatus::Active), "$", lang);
        assert_eq!(row[0], "active");
        assert_eq!(row[1], "proj-a");
        assert_eq!(row[2], "op-us");
        assert_eq!(row[3], "42%");
        assert_eq!(row[4], "$1.25");
        assert_eq!(row[5], "1.5k");
        assert_eq!(row[6], "2");
    }

    #[test]
    fn format_row_corrupt_shows_label() {
        let mut w = win(WindowStatus::Ended);
        w.corrupt = true;
        let row = format_row(&w, "$", Language::En);
        assert_eq!(row[0], "ended");
        assert_eq!(row[1], "corrupt");
    }
}
