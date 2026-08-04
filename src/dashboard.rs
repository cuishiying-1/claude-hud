use std::collections::HashSet;
use std::io;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::alert;
use crate::core::ansi;
use crate::core::i18n::tr;
use crate::core::animation;
use crate::core::config::AppConfig;
use crate::core::history::HistoryStore;
use crate::core::pricing;
use crate::core::session::SessionData;
use crate::core::state::{self, StateFile};
use crate::core::theme::Theme;
use crate::core::transcript::{TranscriptReader, TranscriptSummary};
use crate::core::widget::WidgetRegistry;

/// Launch the full-screen ratatui dashboard.
pub fn run(
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> Result<(), String> {
    enable_raw_mode().map_err(|e| format!("enable raw mode: {}", e))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .map_err(|e| format!("enter alt screen: {}", e))?;

    let mut terminal = ratatui::Terminal::new(CrosstermBackend::new(io::stdout()))
        .map_err(|e| format!("init terminal: {}", e))?;

    let result = run_loop(&mut terminal, registry, config, theme);

    disable_raw_mode().map_err(|e| format!("disable raw mode: {}", e))?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .map_err(|e| format!("leave alt screen: {}", e))?;

    result
}

fn run_loop(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> Result<(), String> {
    let lang = config.language();
    let tick_rate = std::time::Duration::from_millis(config.dashboard.refresh_interval_ms);
    let mut last_agent_count: usize = 0;
    let mut notified_stalled: HashSet<String> = HashSet::new();
    let mut layout_name = config.dashboard.default_layout.clone();
    let mut tab_idx: usize = 0;
    let mut show_help = false;

    // 启动时从 state.json 恢复：数据（新鲜快照）、transcript 游标、告警冷却
    let initial = StateFile::read(&AppConfig::state_path().unwrap_or_default());
    let mut data = initial
        .snapshot
        .to_session_if_fresh(state::now_secs())
        .unwrap_or_default();
    let mut transcript_reader: Option<TranscriptReader> = if initial.transcript.path.is_empty() {
        None
    } else {
        Some(TranscriptReader::from_state(&initial.transcript))
    };

    // 告警冷却只 seed 一次，运行期仅内存（render 是跨进程权威）
    let mut cooldown = alert::AlertCooldown::from_state(&initial.alerts);

    // Open history store for session recording
    let history = HistoryStore::open().ok();

    let mut summary: Option<TranscriptSummary> = None;

    loop {
        // TTY → state.json 快照；非 TTY → 旧 stdin 路径。None 时保留上次数据
        // （占位显示，避免空白闪烁）。
        if let Some(d) = state::read_current_data() {
            data = d;
        }

        // Init transcript reader if we have a path
        if transcript_reader.is_none() {
            if let Some(ref path) = data.transcript_path {
                transcript_reader = Some(TranscriptReader::new(PathBuf::from(path)));
            }
        }

        // Read transcript updates and push to all widgets
        if let Some(ref mut reader) = transcript_reader {
            let s = reader.read_updates();
            // Push transcript summary to all widgets that accept it
            for widget in &registry.widgets {
                widget.update_transcript(&s);
            }
            summary = Some(s);
        }

        // Check for notification triggers
        let fired = alert::check_alerts(&data, &config.alerts, &mut cooldown, state::now_secs());
        let effective_cost = pricing::effective_cost(
            &data,
            summary.as_ref().unwrap_or(&TranscriptSummary::default()),
            &config.pricing,
        )
        .0;
        alert::send_notifications(
            &fired,
            &data,
            &config.alerts,
            &config.currency_symbol,
            effective_cost,
            lang,
        );

        // ⑪ 通知接线：代理全部结束（agents_edge 上升沿） / 代理卡顿（进程内去重）
        let now = state::now_secs();
        let active = data
            .subagent_status_line
            .as_ref()
            .map(|s| s.agents.len())
            .unwrap_or(0);
        if let Some(done) = agents_edge(last_agent_count, active) {
            crate::notify::agents_complete(done, lang);
        }
        last_agent_count = active;

        if let Some(ref s) = summary {
            let threshold = config
                .widget_config("agent_overview")
                .get_u64("stall_threshold_sec", 30);
            let stalled = s.stalled_agents(threshold, now);
            if stalled.is_empty() {
                notified_stalled.clear();
            } else {
                for agent in stalled {
                    if notified_stalled.insert(agent.name.clone()) {
                        let idle = agent
                            .last_tool_call_secs
                            .map(|t| now.saturating_sub(t))
                            .unwrap_or(0);
                        crate::notify::agent_stalled(&agent.name, idle, lang);
                    }
                }
            }
        }

        terminal
            .draw(|frame| {
                draw_dashboard(
                    frame, registry, &data, theme, config, summary.as_ref(),
                    &layout_name, tab_idx, show_help,
                );
            })
            .map_err(|e| format!("draw: {}", e))?;

        if event::poll(tick_rate).map_err(|e| format!("poll: {}", e))? {
            if let Event::Key(key) = event::read().map_err(|e| format!("read event: {}", e))? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        // Record session before exit
                        if let Some(ref h) = history {
                            let _ = h.record_session(&data, last_agent_count);
                        }
                        return Ok(());
                    }
                    KeyCode::Char('l') => {
                        layout_name = next_layout(&layout_name);
                        persist_layout(&layout_name); // best-effort
                    }
                    KeyCode::Char('?') => {
                        show_help = !show_help;
                    }
                    KeyCode::Left | KeyCode::Right => {
                        if layout_name == "tabbed" {
                            let len = config.compact_layout.len();
                            tab_idx = next_tab(
                                tab_idx,
                                len,
                                if key.code == KeyCode::Left { -1 } else { 1 },
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// ⑪ 通知边界：活跃代理数从 >0 降到 0 → Some(前值)（"全部代理已结束"触发条件）。
pub fn agents_edge(prev: usize, cur: usize) -> Option<usize> {
    if prev > 0 && cur == 0 {
        Some(prev)
    } else {
        None
    }
}

/// ⑯ 'l' 键布局循环：grid-2x2 → sidebar → focus → tabbed → grid-2x2；未知值从 grid-2x2 起步。
pub fn next_layout(cur: &str) -> String {
    match cur {
        "grid-2x2" => "sidebar".to_string(),
        "sidebar" => "focus".to_string(),
        "focus" => "tabbed".to_string(),
        "tabbed" => "grid-2x2".to_string(),
        _ => "grid-2x2".to_string(),
    }
}

/// tab 切换（wrap）：dir>0 右移，dir<0 左移；len=0 → 0。
pub fn next_tab(cur: usize, len: usize, dir: i8) -> usize {
    if len == 0 {
        return 0;
    }
    let d = if dir > 0 { 1 } else { len - 1 };
    (cur + d) % len
}

fn draw_dashboard(
    frame: &mut Frame,
    registry: &WidgetRegistry,
    data: &SessionData,
    theme: &Theme,
    config: &AppConfig,
    summary: Option<&TranscriptSummary>,
    layout_name: &str,
    tab_idx: usize,
    show_help: bool,
) {
    let area = frame.area();
    let lang = config.language();

    // 底部 1 行 footer；帮助面板展开时在其上方让出空间
    let areas = if show_help {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(HELP_PANEL_HEIGHT),
                Constraint::Length(1),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(area)
    };
    let main_area = areas[0];
    let footer_area = areas[areas.len() - 1];
    let help_area = if show_help { Some(areas[1]) } else { None };

    if config.dashboard.scanlines {
        render_scanlines(frame, main_area, theme);
    }

    if layout_name == "tabbed" {
        draw_tabbed(
            frame, registry, data, theme, config, summary, main_area, tab_idx,
        );
    } else {
        let layout = match layout_name {
            "sidebar" => build_sidebar(main_area),
            "focus" => vec![main_area],
            _ => build_grid_2x2(main_area),
        };
        // Map widgets to panels (use compact_layout order as panel assignment)
        let widget_ids: Vec<&str> = config.compact_layout.iter()
            .map(|s| s.as_str())
            .collect();
        for (i, panel_area) in layout.iter().enumerate() {
            let widget_id = widget_ids.get(i).copied().unwrap_or("context_bar");
            let render_area = if layout_name == "focus" {
                render_pseudo3d(*panel_area, frame, theme)
            } else {
                *panel_area
            };
            if let Some(widget) = registry.get(widget_id) {
                let mut widget_config = config.widget_config(widget_id);
                pricing::inject_cost(data, summary, config, &mut widget_config);
                widget.render_dashboard(data, render_area, frame, theme, &widget_config);
            }
        }
    }

    if let Some(h) = help_area {
        render_help(frame, h, config);
    }
    let footer = format!(
        "{} {} · {} {} · {}",
        tr(lang, "runtime.footer_layout"),
        layout_name,
        tr(lang, "runtime.footer_mod"),
        config.active_mod,
        tr(lang, "runtime.footer_hint")
    );
    frame.render_widget(Paragraph::new(Text::from(footer)), footer_area);
}

fn build_grid_2x2(area: ratatui::layout::Rect) -> Vec<ratatui::layout::Rect> {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);

    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    vec![top[0], top[1], bottom[0], bottom[1]]
}

fn build_sidebar(area: ratatui::layout::Rect) -> Vec<ratatui::layout::Rect> {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(33), Constraint::Percentage(67)])
        .split(area);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(columns[1]);

    vec![columns[0], right[0], right[1]]
}

/// 伪 3D 面板：accent 边框（光源）+ 右下偏移 1 格 border 色阴影块
/// （ratatui 0.29 无按侧边框样式，用偏移阴影实现 bevel 立体感）。
/// 返回内边距 1 的内容区。
fn render_pseudo3d(area: ratatui::layout::Rect, frame: &mut Frame, theme: &Theme) -> ratatui::layout::Rect {
    use ratatui::layout::Margin;
    use ratatui::widgets::{Block, Borders};
    if area.width < 3 || area.height < 3 {
        return area;
    }
    let panel = ratatui::layout::Rect::new(area.x, area.y, area.width - 1, area.height - 1);
    let shadow = ratatui::layout::Rect::new(area.x + 1, area.y + 1, panel.width, panel.height);
    frame.render_widget(
        Block::bordered()
            .border_style(Style::default().fg(ansi::parse_ratatui_color(&theme.border))),
        shadow,
    );
    frame.render_widget(
        Block::bordered()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ansi::parse_ratatui_color(&theme.accent))),
        panel,
    );
    panel.inner(Margin::new(1, 1))
}

/// tabbed 布局：顶部 1 行 tab 条（compact_layout 各 widget 名，激活项 accent）
/// + 下方伪 3D 内容面板（当前 tab 的 widget）。
fn draw_tabbed(
    frame: &mut Frame,
    registry: &WidgetRegistry,
    data: &SessionData,
    theme: &Theme,
    config: &AppConfig,
    summary: Option<&TranscriptSummary>,
    area: ratatui::layout::Rect,
    tab_idx: usize,
) {
    let lang = config.language();
    let tab_bar = ratatui::layout::Rect::new(area.x, area.y, area.width, 1);
    let mut spans: Vec<Span> = Vec::new();
    for (i, id) in config.compact_layout.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        // widget.<id> key 存在 → 翻译；否则回退原显示名（如脚本路径）
        let translated = crate::core::i18n::tr_dyn(lang, id);
        let name = if translated == id.as_str() {
            registry
                .get(id)
                .map(|w| w.display_name().to_string())
                .unwrap_or_else(|| id.clone())
        } else {
            translated.into_owned()
        };
        let color = if i == tab_idx {
            ansi::parse_ratatui_color(&theme.accent)
        } else {
            ansi::parse_ratatui_color(&theme.muted)
        };
        spans.push(Span::styled(name, Style::default().fg(color)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), tab_bar);

    let content = ratatui::layout::Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(1));
    let inner = render_pseudo3d(content, frame, theme);
    let widget_id = config
        .compact_layout
        .get(tab_idx)
        .cloned()
        .unwrap_or_else(|| "context_bar".to_string());
    if let Some(widget) = registry.get(&widget_id) {
        let mut widget_config = config.widget_config(&widget_id);
        pricing::inject_cost(data, summary, config, &mut widget_config);
        widget.render_dashboard(data, inner, frame, theme, &widget_config);
    }
}

/// CRT 扫描线背景层：每 4 行一行 border 色 dim 行 + 1 行 accent 扫描带
/// （相位行进）。widget 渲染在其上，不遮挡内容。
fn render_scanlines(frame: &mut Frame, area: ratatui::layout::Rect, theme: &Theme) {
    let scan_row = animation::scanline_offset(animation::now_phase(8.0), area.height);
    let mut lines: Vec<Line> = Vec::new();
    for y in 0..area.height {
        let color = if y == scan_row {
            Some(&theme.accent)
        } else if y % 4 == 0 {
            Some(&theme.border)
        } else {
            None
        };
        let line = match color {
            Some(c) => Line::styled(
                " ".repeat(area.width as usize),
                Style::default().fg(ansi::parse_ratatui_color(c)),
            ),
            None => Line::raw(" ".repeat(area.width as usize)),
        };
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

/// 帮助面板高度（7 行内容 + 边框 2 行）。
const HELP_PANEL_HEIGHT: u16 = 9;

/// ⑯ 帮助面板：全部按键 + 全局生效说明。
fn render_help(frame: &mut Frame, area: ratatui::layout::Rect, config: &AppConfig) {
    let lang = config.language();
    let lines = vec![
        Line::from(tr(lang, "runtime.help_quit")),
        Line::from(tr(lang, "runtime.help_cycle")),
        Line::from(tr(lang, "runtime.help_tab")),
        Line::from(tr(lang, "runtime.help_help")),
        Line::from(""),
        Line::from(tr(lang, "runtime.help_global")),
        Line::from(
            tr(lang, "runtime.help_persist")
                .replace("{mod}", &config.active_mod),
        ),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(ratatui::widgets::Block::bordered().title(tr(lang, "runtime.help_title"))),
        area,
    );
}

/// ⑯ 读-改-写 config.toml 的 dashboard.default_layout；失败 eprintln 警告不中断。
/// TOML 往返会丢失注释（拍板取舍，doctor 与文档提示）。
fn persist_layout(layout: &str) {
    let config_path = match AppConfig::config_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[claude-hud] warning: cannot persist layout: {}", e);
            return;
        }
    };
    let Some(mut root) = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|c| toml::from_str::<toml::Value>(&c).ok())
        .filter(|v| v.is_table())
    else {
        eprintln!("[claude-hud] warning: config.toml unreadable; layout switch not persisted");
        return;
    };
    let Some(dashboard) = root
        .as_table_mut()
        .expect("filtered to a table")
        .entry("dashboard")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
    else {
        eprintln!("[claude-hud] warning: [dashboard] is not a table; layout switch not persisted");
        return;
    };
    dashboard.insert(
        "default_layout".to_string(),
        toml::Value::String(layout.to_string()),
    );
    let Ok(out) = toml::to_string_pretty(&root) else {
        eprintln!("[claude-hud] warning: serialize config failed; layout switch not persisted");
        return;
    };
    if let Err(e) = std::fs::write(&config_path, out) {
        eprintln!("[claude-hud] warning: write config: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_edge_three_states() {
        assert_eq!(agents_edge(0, 0), None);
        assert_eq!(agents_edge(2, 2), None);
        assert_eq!(agents_edge(2, 0), Some(2));
        assert_eq!(agents_edge(0, 2), None);
    }

    #[test]
    fn next_layout_cycles_four_layouts() {
        assert_eq!(next_layout("grid-2x2"), "sidebar");
        assert_eq!(next_layout("sidebar"), "focus");
        assert_eq!(next_layout("focus"), "tabbed");
        assert_eq!(next_layout("tabbed"), "grid-2x2");
    }

    #[test]
    fn next_layout_unknown_starts_from_grid() {
        assert_eq!(next_layout(""), "grid-2x2");
        assert_eq!(next_layout("weird"), "grid-2x2");
    }

    #[test]
    fn next_tab_wraps_both_directions() {
        assert_eq!(next_tab(0, 4, 1), 1);
        assert_eq!(next_tab(3, 4, 1), 0);
        assert_eq!(next_tab(0, 4, -1), 3);
        assert_eq!(next_tab(2, 4, -1), 1);
        assert_eq!(next_tab(0, 0, 1), 0);
    }
}
