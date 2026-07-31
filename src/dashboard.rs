use std::io;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::text::Text;
use ratatui::Frame;

use crate::core::config::AppConfig;
use crate::core::history::HistoryStore;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::transcript::TranscriptReader;
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
    let tick_rate = std::time::Duration::from_millis(config.dashboard.refresh_interval_ms);
    let mut transcript_reader: Option<TranscriptReader> = None;
    let mut last_agent_count: usize = 0;

    // Open history store for session recording
    let history = HistoryStore::open().ok();

    loop {
        // Parse current session data from stdin
        let data = read_current_data().unwrap_or_default();

        // Init transcript reader if we have a path
        if transcript_reader.is_none() {
            if let Some(ref path) = data.transcript_path {
                transcript_reader = Some(TranscriptReader::new(PathBuf::from(path)));
            }
        }

        // Read transcript updates and push to all widgets
        if let Some(ref mut reader) = transcript_reader {
            let summary = reader.read_updates();
            // Push transcript summary to all widgets that accept it
            for widget in &registry.widgets {
                widget.update_transcript(&summary);
            }
        }

        // Check for notification triggers
        check_alerts(&data, &last_agent_count);

        terminal
            .draw(|frame| {
                draw_dashboard(frame, registry, &data, theme, config);
            })
            .map_err(|e| format!("draw: {}", e))?;

        if event::poll(tick_rate).map_err(|e| format!("poll: {}", e))? {
            if let Event::Key(key) = event::read().map_err(|e| format!("read event: {}", e))? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        // Record session before exit
                        if let Some(ref h) = history {
                            let _ = h.record_session(&data, last_agent_count, &config.active_mod);
                        }
                        return Ok(());
                    }
                    KeyCode::Char('1'..='9') => {
                        // Tab switching between dashboard layouts (future)
                    }
                    _ => {}
                }
            }
        }
    }
}

fn draw_dashboard(
    frame: &mut Frame,
    registry: &WidgetRegistry,
    data: &SessionData,
    theme: &Theme,
    config: &AppConfig,
) {
    let area = frame.area();

    // Build layout based on config
    let layout = match config.dashboard.default_layout.as_str() {
        "grid-2x2" => build_grid_2x2(area),
        "sidebar" => build_sidebar(area),
        "tabbed" | "focus" => build_single_panel(area),
        _ => build_grid_2x2(area),
    };

    // Map widgets to panels (use compact_layout order as panel assignment)
    let widget_ids: Vec<&str> = config.compact_layout.iter()
        .map(|s| s.as_str())
        .collect();

    for (i, panel_area) in layout.iter().enumerate() {
        let widget_id = widget_ids.get(i).copied().unwrap_or("context_bar");
        if let Some(widget) = registry.get(widget_id) {
            let widget_config = config.widget_config(widget_id);
            widget.render_dashboard(data, *panel_area, frame, theme, &widget_config);
        }
    }
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

fn build_single_panel(area: ratatui::layout::Rect) -> Vec<ratatui::layout::Rect> {
    vec![area]
}

fn read_current_data() -> Option<SessionData> {
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).ok()?;
    SessionData::from_stdin_json(&buf).ok()
}

/// Check conditions and fire OS notifications.
fn check_alerts(data: &SessionData, last_agent_count: &usize) {
    let pct = data.context_window.used_percentage;
    if pct >= 95.0 {
        crate::notify::context_critical(pct);
    }
    if data.cost.total_cost_usd >= 10.0 {
        crate::notify::cost_threshold(data.cost.total_cost_usd, 10.0);
    }
    if data.rate_limits.five_hour.used_percentage >= 90.0 {
        crate::notify::rate_limit_warning(data.rate_limits.five_hour.used_percentage);
    }
}
