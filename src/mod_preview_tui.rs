//! `mod preview` 交互式浏览：候选列表 + 实时主题样例 + Enter 切换。
//!
//! TTY 下为 ratatui 全屏（左列表 ↑/↓ 浏览，右预览元数据 + 主题样例行）；
//! 非 TTY（黑盒 / `!` 命令）回退为数字列表选择，与 `mod pick` 同交互。

use std::io::{self, IsTerminal};

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::core::ansi::ansi_fg;
use crate::core::config::AppConfig;
use crate::core::i18n::{tr, Language};
use crate::core::theme::Theme;
use crate::BUILTIN_MODS;

/// 可选 mod 全列表（出厂包 + 主题预设 + 用户 mods），与 mod pick 同口径。
pub fn candidates() -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    for name in BUILTIN_MODS {
        if AppConfig::load_mod(name).is_ok() {
            items.push(name.to_string());
        }
    }
    for name in Theme::preset_names() {
        items.push(name.to_string());
    }
    if let Ok(mods_dir) = AppConfig::mods_dir() {
        if let Ok(entries) = std::fs::read_dir(mods_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let fname = entry.file_name().to_string_lossy().to_string();
                if let Some(name) = fname.strip_suffix(".toml") {
                    items.push(name.to_string());
                }
            }
        }
    }
    items
}

/// 用解析出的主题渲染一行示例状态栏（真彩 ANSI）；预览不切换也能看到配色。
/// 色位与 compact 实际布局一致：model_color 模型名 / muted 分隔与空位 /
/// success 成本与进度填充 / fg 百分比 / warning 速率。
pub fn theme_sample(theme: &Theme) -> String {
    let filled: String = theme
        .bar_filled
        .to_string()
        .repeat(theme.bar_width as usize);
    let empty: String = theme.bar_empty.to_string().repeat(4);
    let sep = theme.separator.clone();
    format!(
        "{} {} {} {} {} {} {} {}",
        ansi_fg("⬢ deepseek-v4-flash", &theme.model_color),
        ansi_fg(&sep, &theme.muted),
        ansi_fg("$0.034 · ≈$0.3/h", &theme.success),
        ansi_fg(&sep, &theme.muted),
        ansi_fg("ctx", &theme.muted),
        format!(
            "{}{}",
            ansi_fg(&filled, &theme.success),
            ansi_fg(&empty, &theme.muted)
        ),
        ansi_fg("68% 136k/200k", &theme.fg),
        ansi_fg("· 12.5k/min", &theme.warning),
    )
}

/// 交互入口：TTY 全屏浏览，非 TTY 数字列表。
/// 返回 Some(name) = 用户 Enter 选中的 mod；None = 退出未切换。
pub fn run(config: &AppConfig) -> Result<Option<String>, String> {
    let items = candidates();
    if items.is_empty() {
        return Err(tr(config.language(), "runtime.mod_none").to_string());
    }
    if !io::stdout().is_terminal() {
        return run_fallback(&items, config);
    }
    run_tui(&items, config)
}

/// 非 TTY 回退：编号列表 + 行输入（与 mod pick 相同交互）；EOF 直接退出。
fn run_fallback(items: &[String], config: &AppConfig) -> Result<Option<String>, String> {
    let lang = config.language();
    for (i, name) in items.iter().enumerate() {
        let active = if *name == config.active_mod { " [active]" } else { "" };
        println!("  {}. {}{}", i + 1, name, active);
    }
    print!(
        "{}",
        tr(lang, "runtime.mod_select").replace("{n}", &items.len().to_string())
    );
    use std::io::Write;
    io::stdout().flush().map_err(|e| format!("flush: {}", e))?;
    let mut line = String::new();
    let read = io::stdin()
        .read_line(&mut line)
        .map_err(|e| format!("read: {}", e))?;
    if read == 0 {
        return Ok(None);
    }
    let idx = parse_choice(&line, items.len(), lang)?;
    Ok(Some(items[idx - 1].clone()))
}

/// 数字选择解析（1-based，越界/非数字报错）。
fn parse_choice(line: &str, n: usize, lang: Language) -> Result<usize, String> {
    let idx: usize = line
        .trim()
        .parse()
        .map_err(|_| tr(lang, "runtime.mod_invalid_num").to_string())?;
    if idx == 0 || idx > n {
        return Err(
            tr(lang, "runtime.mod_invalid_choice")
                .replace("{idx}", &idx.to_string())
                .replace("{n}", &n.to_string()),
        );
    }
    Ok(idx)
}

/// TTY 全屏：左列表 ↑/↓ 浏览，右预览元数据 + 主题样例行；Enter 切换、q/Esc 退出。
fn run_tui(items: &[String], config: &AppConfig) -> Result<Option<String>, String> {
    enable_raw_mode().map_err(|e| format!("enable raw mode: {}", e))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .map_err(|e| format!("enter alt screen: {}", e))?;
    let mut terminal = ratatui::Terminal::new(CrosstermBackend::new(io::stdout()))
        .map_err(|e| format!("init terminal: {}", e))?;
    let mut idx = items
        .iter()
        .position(|i| *i == config.active_mod)
        .unwrap_or(0);
    let result = loop {
        terminal
            .draw(|frame| render(frame, items, config, idx))
            .map_err(|e| format!("draw: {}", e))?;
        if event::poll(std::time::Duration::from_millis(200))
            .map_err(|e| format!("poll: {}", e))?
        {
            if let Event::Key(key) =
                event::read().map_err(|e| format!("read: {}", e))?
            {
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => {
                        idx = (idx + 1) % items.len();
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        idx = (idx + items.len() - 1) % items.len();
                    }
                    KeyCode::Enter => break Ok(Some(items[idx].clone())),
                    KeyCode::Char('q') | KeyCode::Esc => break Ok(None),
                    _ => {}
                }
            }
        }
    };
    disable_raw_mode().map_err(|e| format!("disable raw mode: {}", e))?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .map_err(|e| format!("leave alt screen: {}", e))?;
    result
}

/// 单帧渲染：左候选列表（选中反显 + [active] 标记），右预览面板。
fn render(frame: &mut Frame, items: &[String], config: &AppConfig, idx: usize) {
    let lang = config.language();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(34), Constraint::Min(0)])
        .split(frame.area());

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let active = if *name == config.active_mod { " [active]" } else { "" };
            let style = if i == idx {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            ListItem::new(format!(" {}. {}{}", i + 1, name, active)).style(style)
        })
        .collect();
    frame.render_widget(
        List::new(list_items)
            .block(Block::default().borders(Borders::ALL).title(tr(lang, "runtime.mod_pick_title"))),
        chunks[0],
    );

    let mut lines: Vec<Line> = Vec::new();
    let name = &items[idx];
    match AppConfig::load_mod(name) {
        Ok(pkg) => {
            lines.push(Line::raw(
                tr(lang, "runtime.mod_preview").replace("{name}", &pkg.mod_info.name),
            ));
            if !pkg.mod_info.scene.is_empty() {
                lines.push(Line::raw(
                    tr(lang, "runtime.mod_scene").replace("{scene}", &pkg.mod_info.scene),
                ));
            }
            if let Some(l) = &pkg.layout {
                lines.push(Line::raw(
                    tr(lang, "runtime.mod_layout_line")
                        .replace("{compact}", &l.compact)
                        .replace("{dashboard}", &l.dashboard),
                ));
            }
            if let Some(t) = &pkg.theme {
                lines.push(Line::raw(
                    tr(lang, "runtime.mod_theme").replace("{theme}", &t.preset),
                ));
            }
            if let Some(a) = &pkg.animation {
                lines.push(Line::raw(
                    tr(lang, "runtime.mod_animation_line")
                        .replace("{enabled}", &a.enabled.to_string())
                        .replace("{effects}", &format!("{:?}", a.effects)),
                ));
            }
            let mut probe = config.clone();
            probe.active_mod = name.clone();
            let sample = theme_sample(&probe.resolve_theme().theme);
            lines.push(Line::raw(""));
            lines.push(Line::raw(
                tr(lang, "runtime.mod_preview_sample").replace("{sample}", &sample),
            ));
        }
        Err(e) => {
            lines.push(Line::raw(format!("load failed: {}", e)));
        }
    }
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default().borders(Borders::ALL).title(tr(lang, "runtime.mod_pick_preview")),
        ),
        chunks[1],
    );

    let hint = tr(lang, "runtime.mod_pick_hint");
    frame.render_widget(
        Paragraph::new(hint).block(Block::default().borders(Borders::ALL)),
        frame.area(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::i18n::Language;

    #[test]
    fn theme_sample_contains_dracula_model_color() {
        let t = Theme::load_preset("dracula").unwrap();
        let s = theme_sample(&t);
        assert!(s.contains("38;2;189;147;249"), "model 色码 #bd93f9: {s}");
        assert!(s.contains("38;2;80;250;123"), "success 绿 #50fa7b: {s}");
    }

    #[test]
    fn theme_sample_uses_theme_bar_chars() {
        let t = Theme::load_preset("nord").unwrap();
        let s = theme_sample(&t);
        assert!(s.contains(&t.bar_filled.to_string()), "填充字符: {s}");
        assert!(s.contains(&t.bar_empty.to_string()), "空位字符: {s}");
    }

    #[test]
    fn candidates_includes_builtin_and_presets() {
        let items = candidates();
        assert!(items.contains(&"glacier-workstation".to_string()));
        assert!(items.contains(&"noir-tabbed".to_string()));
        assert!(items.contains(&"dracula".to_string()));
        assert!(items.contains(&"nord".to_string()));
    }

    #[test]
    fn parse_choice_accepts_bounds() {
        assert_eq!(parse_choice("2\n", 6, Language::En).unwrap(), 2);
        assert!(parse_choice("0\n", 6, Language::En).is_err());
        assert!(parse_choice("7\n", 6, Language::En).is_err());
        assert!(parse_choice("x\n", 6, Language::En).is_err());
    }
}
