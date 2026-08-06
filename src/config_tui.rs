use std::io::{self, IsTerminal};

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::core::config::AppConfig;
use crate::core::config_schema::{self, Group};
use crate::core::i18n::tr;
use crate::core::widget::WidgetRegistry;

/// 启动键盘配置表单；非 TTY（黑盒）渲染一帧即退出。
pub fn run(registry: &WidgetRegistry, config: &AppConfig) -> Result<(), String> {
    if !io::stdout().is_terminal() {
        return render_single_frame(registry, config);
    }
    enable_raw_mode().map_err(|e| format!("enable raw mode: {}", e))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .map_err(|e| format!("enter alt screen: {}", e))?;
    let mut terminal = ratatui::Terminal::new(CrosstermBackend::new(io::stdout()))
        .map_err(|e| format!("init terminal: {}", e))?;
    let result = run_loop(&mut terminal, registry, config);
    disable_raw_mode().map_err(|e| format!("disable raw mode: {}", e))?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .map_err(|e| format!("leave alt screen: {}", e))?;
    result
}

/// 骨架循环：渲染 + q/Esc 退出（完整交互 Task 5 补）。
fn run_loop(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    registry: &WidgetRegistry,
    config: &AppConfig,
) -> Result<(), String> {
    let mut group_idx = 0usize;
    loop {
        terminal
            .draw(|frame| {
                render_form(frame, registry, config, group_idx);
            })
            .map_err(|e| format!("draw: {}", e))?;
        if event::poll(std::time::Duration::from_millis(200))
            .map_err(|e| format!("poll: {}", e))?
        {
            if let Event::Key(key) = event::read().map_err(|e| format!("read: {}", e))? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Tab => {
                        group_idx = (group_idx + 1) % Group::all().len();
                    }
                    _ => {}
                }
            }
        }
    }
}

/// 表单渲染：左分组栏 + 右字段列表。
fn render_form(
    frame: &mut Frame,
    registry: &WidgetRegistry,
    config: &AppConfig,
    group_idx: usize,
) {
    let _ = registry;
    let lang = config.language();
    let groups = Group::all();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22), Constraint::Min(0)])
        .split(frame.area());

    let tab_items: Vec<ListItem> = groups
        .iter()
        .map(|g| {
            let label = tr(lang, g.name());
            ListItem::new(label).style(if *g == groups[group_idx] {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            })
        })
        .collect();
    frame.render_widget(
        List::new(tab_items)
            .block(Block::default().borders(Borders::ALL).title(tr(lang, "config.title"))),
        chunks[0],
    );

    let current = groups[group_idx];
    let field_lines: Vec<Line> = config_schema::fields()
        .iter()
        .filter(|f| f.group == current)
        .map(|f| {
            let value = config_schema::get_value(config, f.key)
                .unwrap_or_default();
            Line::from(vec![
                Span::raw(tr(lang, f.label)),
                Span::raw(": "),
                Span::raw(value),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(field_lines)
            .block(Block::default().borders(Borders::ALL).title(tr(lang, "config.groups_display"))),
        chunks[1],
    );
}

/// 非 TTY 单帧（黑盒可测）：打印分组标题 + 字段数后退出。
fn render_single_frame(registry: &WidgetRegistry, config: &AppConfig) -> Result<(), String> {
    let _ = registry;
    let lang = config.language();
    println!("{}", tr(lang, "config.title"));
    for g in Group::all() {
        let count = config_schema::fields()
            .iter()
            .filter(|f| f.group == g)
            .count();
        println!("{}: {} fields", tr(lang, g.name()), count);
    }
    Ok(())
}
