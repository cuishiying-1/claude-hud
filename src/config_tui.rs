use std::collections::{HashMap, HashSet};
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
use crate::core::config_schema::{self, FieldDef, Group};
use crate::core::i18n::{tr, Language};
use crate::core::widget::WidgetRegistry;

/// 编辑态：行内文本 / 数值 / 选项列表 / 多选勾选排序。
enum EditState {
    Text(String),
    Number(String),
    Choice(usize),
    Multi {
        selected: HashSet<String>,
        order: Vec<String>,
        cursor: usize,
    },
    List(String),
}

struct FormState {
    group_idx: usize,
    field_idx: usize,
    editing: Option<EditState>,
    /// 已提交（Enter）的字段编辑：key → raw 值。非空即有未保存修改。
    dirty: HashMap<String, String>,
    error: Option<String>,
    msg: Option<String>,
}

impl FormState {
    fn new() -> Self {
        Self {
            group_idx: 0,
            field_idx: 0,
            editing: None,
            dirty: HashMap::new(),
            error: None,
            msg: None,
        }
    }
}

enum KeyAction {
    None,
    Quit,
    QuitForce,
    Save,
}

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

fn run_loop(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    registry: &WidgetRegistry,
    config: &AppConfig,
) -> Result<(), String> {
    let mut state = FormState::new();
    let lang = config.language();
    loop {
        terminal
            .draw(|frame| {
                render_form(frame, registry, config, &mut state);
            })
            .map_err(|e| format!("draw: {}", e))?;
        if event::poll(std::time::Duration::from_millis(200))
            .map_err(|e| format!("poll: {}", e))?
        {
            if let Event::Key(key) = event::read().map_err(|e| format!("read: {}", e))? {
                if !crate::dashboard::is_press(&key) {
                    continue; // Windows Release 事件：忽略，防按键双触发
                }
                match handle_key(key.code, &mut state, registry, config, lang) {
                    KeyAction::Quit | KeyAction::QuitForce => return Ok(()),
                    KeyAction::Save => {
                        match save_edits(config, &state) {
                            Ok(()) => {
                                state.dirty.clear();
                                state.error = None;
                                state.msg = Some(tr(lang, "config.saved_ok").into());
                            }
                            Err(e) => {
                                state.error = Some(e);
                                state.msg = None;
                            }
                        }
                    }
                    KeyAction::None => {}
                }
            }
        }
    }
}

/// 非编辑态按键：导航 + 进入编辑 + 保存/退出（dirty 两段式确认）。
fn handle_key(
    code: KeyCode,
    state: &mut FormState,
    registry: &WidgetRegistry,
    config: &AppConfig,
    lang: Language,
) -> KeyAction {
    use crossterm::event::KeyCode as K;
    if state.editing.is_some() {
        return handle_editing_key(code, state, registry, config, lang);
    }
    let fields = config_schema::fields();
    match code {
        K::Char('q') => {
            if state.dirty.is_empty() {
                KeyAction::Quit
            } else if state.msg.as_deref() == Some(tr(lang, "config.unsaved_confirm")) {
                KeyAction::QuitForce
            } else {
                state.msg = Some(tr(lang, "config.unsaved_confirm").into());
                KeyAction::None
            }
        }
        K::Esc => {
            if state.dirty.is_empty() {
                KeyAction::Quit
            } else {
                state.msg = None;
                KeyAction::None
            }
        }
        K::Char('s') => KeyAction::Save,
        K::Tab => {
            state.group_idx = (state.group_idx + 1) % Group::all().len();
            state.field_idx = 0;
            state.error = None;
            KeyAction::None
        }
        K::Down | K::Up => {
            let n = fields
                .iter()
                .filter(|f| f.group == Group::all()[state.group_idx])
                .count();
            if n > 0 {
                state.field_idx = if code == K::Down {
                    (state.field_idx + 1) % n
                } else {
                    (state.field_idx + n - 1) % n
                };
            }
            state.error = None;
            KeyAction::None
        }
        K::Enter => {
            let current: Vec<&FieldDef> = fields
                .iter()
                .filter(|f| f.group == Group::all()[state.group_idx])
                .collect();
            if let Some(f) = current.get(state.field_idx) {
                let raw = config_schema::get_value(config, f.key).unwrap_or_default();
                state.editing = Some(match f.kind {
                    config_schema::FieldKind::Bool => {
                        let next = if raw == "true" { "false" } else { "true" };
                        return finish_edit(state, f.key, next.to_string());
                    }
                    config_schema::FieldKind::Multi => {
                        let order: Vec<String> = raw
                            .split(',')
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string())
                            .collect();
                        let selected: HashSet<String> = order.iter().cloned().collect();
                        EditState::Multi { selected, order, cursor: 0 }
                    }
                    config_schema::FieldKind::Choice => {
                        let opts = config_schema::options_for(f, registry);
                        let idx = opts.iter().position(|o| *o == raw).unwrap_or(0);
                        EditState::Choice(idx)
                    }
                    config_schema::FieldKind::NumberList => EditState::List(raw),
                    config_schema::FieldKind::Number => EditState::Number(raw),
                    _ => EditState::Text(raw),
                });
            }
            KeyAction::None
        }
        _ => KeyAction::None,
    }
}

/// 编辑态按键：行内输入 / Choice 上下选 / Multi 空格勾选 + 上下移动游标。
fn handle_editing_key(
    code: KeyCode,
    state: &mut FormState,
    registry: &WidgetRegistry,
    _config: &AppConfig,
    _lang: Language,
) -> KeyAction {
    use crossterm::event::KeyCode as K;
    let current: Vec<FieldDef> = config_schema::fields()
        .into_iter()
        .filter(|f| f.group == Group::all()[state.group_idx])
        .collect();
    let Some(f) = current.get(state.field_idx) else {
        return KeyAction::None;
    };
    match (&mut state.editing, code) {
        (Some(EditState::Text(buf)) | Some(EditState::Number(buf)) | Some(EditState::List(buf)),
         K::Char(c)) => {
            buf.push(c);
            KeyAction::None
        }
        (Some(EditState::Text(buf)) | Some(EditState::Number(buf)) | Some(EditState::List(buf)),
         K::Backspace) => {
            buf.pop();
            KeyAction::None
        }
        (_, K::Enter) => {
            let raw = editing_raw(state, registry, f);
            state.editing = None;
            finish_edit(state, f.key, raw)
        }
        (Some(EditState::Choice(idx)), K::Up) => {
            let n = config_schema::options_for(f, registry).len();
            *idx = (*idx + n - 1) % n;
            KeyAction::None
        }
        (Some(EditState::Choice(idx)), K::Down) => {
            let n = config_schema::options_for(f, registry).len();
            *idx = (*idx + 1) % n;
            KeyAction::None
        }
        (Some(EditState::Multi { selected, order, cursor }), K::Char(' ')) => {
            let opts = config_schema::options_for(f, registry);
            if let Some(opt) = opts.get(*cursor).cloned() {
                if selected.contains(&opt) {
                    selected.remove(&opt);
                    order.retain(|o| o != &opt);
                } else {
                    selected.insert(opt.clone());
                    order.push(opt);
                }
            }
            KeyAction::None
        }
        (Some(EditState::Multi { cursor, .. }), K::Down) => {
            let n = config_schema::options_for(f, registry).len();
            if n > 0 {
                *cursor = (*cursor + 1) % n;
            }
            KeyAction::None
        }
        (Some(EditState::Multi { cursor, .. }), K::Up) => {
            let n = config_schema::options_for(f, registry).len();
            if n > 0 {
                *cursor = (*cursor + n - 1) % n;
            }
            KeyAction::None
        }
        (_, K::Esc) => {
            state.editing = None;
            state.error = None;
            KeyAction::None
        }
        _ => KeyAction::None,
    }
}

/// 编辑缓冲 → 提交 raw（Enter 时固化，供 dirty map 使用）。
fn editing_raw(state: &FormState, registry: &WidgetRegistry, f: &FieldDef) -> String {
    match &state.editing {
        Some(EditState::Text(b)) | Some(EditState::Number(b)) | Some(EditState::List(b)) => {
            b.clone()
        }
        Some(EditState::Choice(idx)) => config_schema::options_for(f, registry)
            .get(*idx)
            .cloned()
            .unwrap_or_default(),
        Some(EditState::Multi { order, .. }) => order.join(","),
        None => String::new(),
    }
}

/// 提交单字段编辑：校验（probe 副本）+ 成功写入 dirty map。
fn finish_edit(state: &mut FormState, key: &str, raw: String) -> KeyAction {
    let mut probe = AppConfig::default();
    match config_schema::set_value(&mut probe, key, &raw) {
        Ok(()) => {
            state.dirty.insert(key.to_string(), raw);
            state.error = None;
        }
        Err(e) => {
            state.error = Some(e);
            state.msg = None;
        }
    }
    KeyAction::None
}

/// 编辑集 → 克隆 config 应用修改（纯函数，可测；原 config 不变）。
fn apply_edits_to(
    config: &AppConfig,
    edits: &[(&str, String)],
) -> Result<AppConfig, String> {
    let mut next = config.clone();
    for (key, raw) in edits {
        config_schema::set_value(&mut next, key, raw)
            .map_err(|e| format!("{}: {}", key, e))?;
    }
    config_schema::validate_config(&next)?;
    Ok(next)
}

/// 保存：克隆 config → 应用 dirty 字段 → 校验 → save（备份重建原子写）。
fn save_edits(config: &AppConfig, state: &FormState) -> Result<(), String> {
    let edits: Vec<(&str, String)> = state
        .dirty
        .iter()
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect();
    let next = apply_edits_to(config, &edits)?;
    next.save(&AppConfig::config_path()?)
}

/// 当前编辑字段 key（渲染高亮用）；未在编辑态返回空串。
fn editing_key(state: &FormState) -> &'static str {
    if state.editing.is_none() {
        return "";
    }
    let current = Group::all()[state.group_idx];
    config_schema::fields()
        .into_iter()
        .filter(|f| f.group == current)
        .nth(state.field_idx)
        .map(|f| f.key)
        .unwrap_or("")
}

/// 表单渲染：左分组栏 + 右字段列表（值列：编辑缓冲 → dirty → 磁盘值）。
fn render_form(
    frame: &mut Frame,
    registry: &WidgetRegistry,
    config: &AppConfig,
    state: &mut FormState,
) {
    let lang = config.language();
    let groups = Group::all();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Min(0)])
        .split(frame.area());

    let tab_items: Vec<ListItem> = groups
        .iter()
        .map(|g| {
            let label = tr(lang, g.name());
            let style = if *g == groups[state.group_idx] {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            ListItem::new(label).style(style)
        })
        .collect();
    frame.render_widget(
        List::new(tab_items)
            .block(Block::default().borders(Borders::ALL).title(tr(lang, "config.title"))),
        chunks[0],
    );

    let current = groups[state.group_idx];
    let field_defs: Vec<FieldDef> = config_schema::fields()
        .into_iter()
        .filter(|f| f.group == current)
        .collect();
    let mut lines: Vec<Line> = Vec::new();
    for (i, f) in field_defs.iter().enumerate() {
        let value = if state.editing.is_some() && f.key == editing_key(state) {
            match &state.editing {
                Some(EditState::Text(b)) | Some(EditState::Number(b))
                | Some(EditState::List(b)) => b.clone(),
                Some(EditState::Choice(idx)) => config_schema::options_for(f, registry)
                    .get(*idx)
                    .cloned()
                    .unwrap_or_default(),
                Some(EditState::Multi { order, .. }) => {
                    if order.is_empty() {
                        tr(lang, "config.none_option").into()
                    } else {
                        order.join(",")
                    }
                }
                None => String::new(),
            }
        } else if let Some(raw) = state.dirty.get(f.key) {
            raw.clone()
        } else {
            config_schema::get_value(config, f.key).unwrap_or_default()
        };
        let display = if f.kind == config_schema::FieldKind::Choice && value.is_empty() {
            tr(lang, "config.none_option").into()
        } else {
            value
        };
        let mut spans = vec![
            Span::raw(format!("{:>2} ", i + 1)),
            Span::raw(tr(lang, f.label)),
            Span::raw(": "),
            Span::raw(display),
        ];
        if f.kind == config_schema::FieldKind::Multi
            && state.editing.is_some()
            && f.key == editing_key(state)
        {
            spans.push(Span::raw("  [space toggle · ↑↓ move]"));
        }
        let style = if state.field_idx == i {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        lines.push(Line::from(spans).style(style));
        // Multi 编辑态：字段行下追加选项列表（[x] 标记 + 游标高亮）
        if f.kind == config_schema::FieldKind::Multi
            && state.editing.is_some()
            && f.key == editing_key(state)
        {
            if let Some(EditState::Multi { selected, cursor, .. }) = &state.editing {
                for (oi, opt) in config_schema::options_for(f, registry)
                    .iter()
                    .enumerate()
                {
                    let mark = if selected.contains(opt) { "[x]" } else { "[ ]" };
                    let opt_style = if oi == *cursor {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default()
                    };
                    lines.push(Line::from(Span::raw(format!("   {mark} {opt}"))).style(opt_style));
                }
            }
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(tr(lang, "config.groups_display"))),
        chunks[1],
    );

    let hint = if let Some(err) = &state.error {
        tr(lang, "config.save_failed").replace("{err}", err)
    } else if let Some(m) = &state.msg {
        m.clone()
    } else {
        tr(lang, "config.hint_nav").into()
    };
    frame.render_widget(
        Paragraph::new(hint).block(Block::default().borders(Borders::ALL)),
        frame.area(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyCode as K;

    fn en_state() -> (FormState, WidgetRegistry, AppConfig, Language) {
        (
            FormState::new(),
            WidgetRegistry::new(),
            AppConfig::default(),
            Language::En,
        )
    }

    #[test]
    fn apply_edits_to_clone_and_validate() {
        let base = AppConfig::default();
        let next = apply_edits_to(
            &base,
            &[("language", "zh".into()), ("dashboard.scanlines", "false".into())],
        )
        .unwrap();
        assert_eq!(next.language, "zh");
        assert!(!next.dashboard.scanlines);
        assert!(config_schema::validate_config(&next).is_ok());
        assert_eq!(base.language, "en", "原 config 不可变");
    }

    #[test]
    fn apply_edits_rejects_invalid_field_value() {
        let base = AppConfig::default();
        let err = apply_edits_to(&base, &[("language", "xx".into())]).unwrap_err();
        assert!(err.contains("language"), "err = {err}");
    }

    #[test]
    fn q_twice_confirms_discard_when_dirty() {
        let (mut state, reg, cfg, lang) = en_state();
        state.dirty.insert("language".into(), "zh".into());
        let a = handle_key(K::Char('q'), &mut state, &reg, &cfg, lang);
        assert!(matches!(a, KeyAction::None));
        assert!(state.msg.is_some(), "第一次 q 显示确认提示");
        let b = handle_key(K::Char('q'), &mut state, &reg, &cfg, lang);
        assert!(matches!(b, KeyAction::QuitForce), "第二次 q 放弃修改退出");
    }

    #[test]
    fn esc_dismisses_confirm_and_continues() {
        let (mut state, reg, cfg, lang) = en_state();
        state.dirty.insert("language".into(), "zh".into());
        let a = handle_key(K::Esc, &mut state, &reg, &cfg, lang);
        assert!(matches!(a, KeyAction::None));
        assert!(state.msg.is_none(), "Esc 清除确认提示继续编辑");
    }

    #[test]
    fn clean_q_quits_directly() {
        let (mut state, reg, cfg, lang) = en_state();
        let a = handle_key(K::Char('q'), &mut state, &reg, &cfg, lang);
        assert!(matches!(a, KeyAction::Quit));
    }

    #[test]
    fn save_path_is_config_path_without_env() {
        let p = AppConfig::config_path().unwrap();
        assert!(p.to_string_lossy().contains("claude-hud"));
    }
}
