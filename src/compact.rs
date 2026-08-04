use std::path::PathBuf;

use crate::alert;
use crate::core::ansi;
use crate::core::config::AppConfig;
use crate::core::history::HistoryStore;
use crate::core::pricing;
use crate::core::session::SessionData;
use crate::core::state::{self, SnapshotSegment, StateFile};
use crate::core::theme::Theme;
use crate::core::transcript::{TranscriptReader, TranscriptSummary};
use crate::core::widget::WidgetRegistry;
use unicode_width::UnicodeWidthStr;

/// 出厂 minimal 布局（无 compact_widgets 快照时的布局 ID 映射）。
pub const MINIMAL_WIDGETS: [&str; 4] =
    ["model_display", "context_bar", "cost_display", "git_status"];
/// 出厂 activity 布局（glacier-workstation 等双行工作台）。
pub const ACTIVITY_WIDGETS: [&str; 7] = [
    "model_display", "context_bar", "agent_overview",
    "git_status", "skills_mcp", "cost_display", "rate_limits",
];

/// 布局解析：compact_widgets 快照 > 布局 ID 映射（minimal/activity）> 其他。
/// 未知布局 ID 返回 Err（render 报错路径，hud_err_marker 上屏）。
pub fn layout_from_mod(
    compact_widgets: Option<&Vec<String>>,
    layout_compact: &str,
) -> Result<Vec<String>, String> {
    if let Some(widgets) = compact_widgets {
        return Ok(widgets.clone());
    }
    let ids: &[&str] = match layout_compact {
        "minimal" => &MINIMAL_WIDGETS,
        "activity" => &ACTIVITY_WIDGETS,
        other => return Err(format!("compact layout '{}' not implemented", other)),
    };
    Ok(ids.iter().map(|s| s.to_string()).collect())
}

/// 行数三层优先级：runtime_overrides > mod.layout > theme。
pub fn lines_from_layers(runtime: Option<u8>, mod_lines: Option<u8>, theme: u8) -> u8 {
    runtime.or(mod_lines).unwrap_or(theme)
}

/// 当前生效的 compact widget 数组（mod 灌入优先，fallback config）。
pub fn resolve_compact_layout(config: &AppConfig) -> Result<Vec<String>, String> {
    if !config.active_mod.is_empty() {
        if let Ok(pkg) = AppConfig::load_mod(&config.active_mod) {
            return layout_from_mod(
                pkg.compact_widgets.as_ref(),
                pkg.layout.as_ref().map(|l| l.compact.as_str()).unwrap_or(""),
            );
        }
    }
    Ok(config.compact_layout.clone())
}

/// 当前生效的 mod compact_lines（无 mod 或加载失败 → None）。
pub fn mod_compact_lines(config: &AppConfig) -> Option<u8> {
    if config.active_mod.is_empty() {
        return None;
    }
    AppConfig::load_mod(&config.active_mod)
        .ok()
        .and_then(|pkg| pkg.layout)
        .map(|l| l.compact_lines)
}

/// Render the compact status bar from stdin JSON data.
pub fn render(
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> Result<String, String> {
    let stdin_data = read_stdin()?;
    let data = SessionData::from_stdin_json(&stdin_data)
        .map_err(|e| format!("parse stdin JSON: {}", e))?;
    run_pipeline(&data, registry, config, theme)
}

/// The 5s render pipeline: restore state → transcript → git/scripts →
/// render → persist. Returns the rendered status line.
fn run_pipeline(
    data: &SessionData,
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> Result<String, String> {
    let state_path = AppConfig::state_path()?;
    let mut state = StateFile::read(&state_path);

    // Transcript: restore cumulative state only when the path is unchanged
    // (双窗口/新会话天然隔离，path 变化即全新 reader)。
    let mut reader = if should_restore(&state.transcript.path, data.transcript_path.as_deref()) {
        TranscriptReader::from_state(&state.transcript)
    } else {
        match &data.transcript_path {
            Some(p) => TranscriptReader::new(PathBuf::from(p)),
            None => TranscriptReader::new(PathBuf::new()),
        }
    };
    let summary = reader.read_updates();
    for widget in &registry.widgets {
        widget.update_transcript(&summary);
    }

    let output = render_with_data(data, registry, config, theme, Some(&summary))?;

    // ⑦ 越阈告警：render 是跨进程冷却权威（加载 → 判定 → 回写 state.alerts）
    let now = state::now_secs();
    let mut cooldown = alert::AlertCooldown::from_state(&state.alerts);
    let fired = alert::check_alerts(&data, &config.alerts, &mut cooldown, now);
    let (effective_cost, _) = pricing::effective_cost(data, &summary, &config.pricing);
    alert::send_notifications(
        &fired,
        &data,
        &config.alerts,
        &config.currency_symbol,
        effective_cost,
    );
    state.alerts = cooldown.to_state();

    // ⑨ 会话切换结账：transcript_path 变化 → 上一会话写入历史库（失败仅警告，不中断渲染）
    if should_checkout(
        state.snapshot.timestamp_secs,
        state.snapshot.transcript_path.as_deref(),
        data.transcript_path.as_deref(),
    ) {
        match HistoryStore::open() {
            Ok(h) => {
                let last = state.snapshot.to_session();
                if let Err(e) = h
                    .record_session(&last, state.snapshot.agent_count, &config.active_mod)
                {
                    eprintln!("[claude-hud] warning: session checkout failed: {}", e);
                }
            }
            Err(e) => eprintln!("[claude-hud] warning: cannot open history db: {}", e),
        }
    }

    // 持久化（best-effort：写失败不中断状态栏，仅 stderr 警告）。
    // 脚本/git widget 可能在管线中途写了 cache 窄键 → 先合并磁盘 cache。
    state.snapshot = SnapshotSegment::from_session(data, now);
    state.transcript = reader.to_state();
    state.last_error = None;
    state.merge_cache_from_disk(&state_path);
    if let Err(e) = state.write(&state_path) {
        eprintln!("[claude-hud] warning: state write failed: {}", e);
    }

    Ok(output)
}

/// True when the persisted transcript segment matches the current stdin
/// path, i.e. the reader should resume from the persisted offset instead of
/// re-parsing the whole file.
pub fn should_restore(state_path: &str, data_path: Option<&str>) -> bool {
    !state_path.is_empty() && data_path == Some(state_path)
}

/// 解析 COLUMNS 值（None = 缺失）：非法 → 80；最小 40（statusLine 最小可用宽度）。
pub fn columns_from(value: Option<&str>) -> u16 {
    value
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(80)
        .max(40)
}

/// 当前终端可见宽度源：COLUMNS 环境变量（statusLine 场景唯一可靠来源）。
pub fn columns_env() -> u16 {
    columns_from(std::env::var("COLUMNS").ok().as_deref())
}

/// ⑮ 从行尾整组丢弃直至可见宽度 ≤ max_width（剥 ANSI 后按 unicode 宽度测）；
/// 至少保留 1 组；sep 为空时原样返回。
pub fn fit_line(line: &str, sep: &str, max_width: usize) -> String {
    if sep.is_empty() {
        return line.to_string();
    }
    let groups: Vec<&str> = line.split(sep).collect();
    let mut keep = groups.len();
    while keep > 1 {
        let candidate = groups[..keep].join(sep);
        if ansi::strip_ansi(&candidate).as_str().width() <= max_width {
            break;
        }
        keep -= 1;
    }
    groups[..keep].join(sep)
}

/// ⑨ 会话切换结账判定：前次快照有结账信息（ts≠0、path 非空）且 path 变化 → 结账。
pub fn should_checkout(prev_ts: u64, prev_path: Option<&str>, cur_path: Option<&str>) -> bool {
    prev_ts != 0
        && !prev_path.map(|p| p.is_empty()).unwrap_or(true)
        && prev_path != cur_path
}

/// Build the stdout error marker for render failures. The message is
/// truncated so the marker stays readable in a terminal status line.
pub fn hud_err_marker(msg: &str) -> String {
    let short: String = msg.chars().take(80).collect();
    format!("[hud err] {} — run 'claude-hud doctor'", short)
}

/// Render the compact status bar from an already-parsed session snapshot.
/// Shared by `render` (stdin) and `doctor` (sample data). No transcript
/// parsing here — that lives in `run_pipeline`.
pub fn render_with_data(
    data: &SessionData,
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
    summary: Option<&TranscriptSummary>,
) -> Result<String, String> {
    let layout = resolve_compact_layout(config)?;
    if layout.is_empty() {
        return Ok(String::new());
    }

    let lines = lines_from_layers(
        config.runtime_overrides.as_ref().and_then(|o| o.compact_lines),
        mod_compact_lines(config),
        theme.compact_lines,
    ) as usize;

    if lines == 0 {
        return Ok(String::new());
    }

    let sep = &config.separator;
    let widgets_per_line = if lines == 1 {
        layout.len()
    } else {
        (layout.len() + lines - 1) / lines
    };

    let mut output = String::new();
    for line_idx in 0..lines {
        let start = line_idx * widgets_per_line;
        let end = (start + widgets_per_line).min(layout.len());
        if start >= end {
            break;
        }
        let line_widgets: Vec<String> = layout[start..end]
            .iter()
            .filter_map(|id| {
                let w = registry.get(id)?;
                let mut widget_config = config.widget_config(id);
                pricing::inject_cost(data, summary, config, &mut widget_config);
                let rendered = w.render_compact(data, theme, &widget_config);
                if rendered.is_empty() {
                    None
                } else {
                    Some(rendered)
                }
            })
            .collect();
        if !line_widgets.is_empty() {
            let joined = line_widgets.join(sep);
            // ⑮ 宽度感知：超出终端列宽时从行尾整组丢弃
            output.push_str(&fit_line(&joined, sep, columns_env() as usize));
            output.push('\n');
        }
    }

    Ok(output.trim_end().to_string())
}

fn read_stdin() -> Result<String, String> {
    use std::io::Read;
    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|e| format!("read stdin: {}", e))?;
    Ok(buffer)
}

/// 调试输出：原始 stdin JSON + 顶层键分类（recognized = SessionData
/// 已知字段含 camelCase alias / unknown = 其余）。解析失败走 render
/// 的错误路径（[hud err] + last_error，行为与 render 一致）。
pub fn dump_stdin() -> Result<(), String> {
    let stdin_data = read_stdin()?;
    let value: serde_json::Value = serde_json::from_str(&stdin_data)
        .map_err(|e| format!("parse stdin JSON: {}", e))?;
    let _ = SessionData::from_stdin_json(&stdin_data)
        .map_err(|e| format!("parse stdin JSON: {}", e))?;

    let recognized = [
        "model", "context_window", "cost", "rate_limits",
        "transcript_path", "subagent_status_line", "subagentStatusLine",
    ];
    let mut unknown: Vec<String> = Vec::new();
    if let Some(obj) = value.as_object() {
        for key in obj.keys() {
            if !recognized.contains(&key.as_str()) {
                unknown.push(key.clone());
            }
        }
    }
    unknown.sort();
    println!("recognized: {}", recognized.join(", "));
    println!(
        "unknown: {}",
        if unknown.is_empty() {
            "(none)".to_string()
        } else {
            unknown.join(", ")
        }
    );
    println!("--- raw stdin ---");
    println!(
        "{}",
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| stdin_data)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_from_missing_or_invalid_defaults_to_80() {
        assert_eq!(columns_from(None), 80);
        assert_eq!(columns_from(Some("abc")), 80);
        assert_eq!(columns_from(Some("-5")), 80); // 解析失败走默认
    }

    #[test]
    fn columns_from_parses_and_clamps_min_40() {
        assert_eq!(columns_from(Some("120")), 120);
        assert_eq!(columns_from(Some("30")), 40);
    }

    #[test]
    fn fit_line_drops_tail_groups_when_over_width() {
        let line = "aaaa │ bbbb │ cccc";
        assert_eq!(fit_line(line, " │ ", 12), "aaaa │ bbbb");
        assert_eq!(fit_line(line, " │ ", 18), line);   // 修正：18 而非 17（全文宽度 18）
    }

    #[test]
    fn fit_line_keeps_single_overwide_group() {
        assert_eq!(fit_line("toolonggroup", " │ ", 5), "toolonggroup");
    }

    #[test]
    fn fit_line_ignores_ansi_width() {
        let line = "\x1b[31mabc\x1b[0m │ x";
        assert_eq!(fit_line(line, " │ ", 7), line);
    }

    #[test]
    fn fit_line_measures_cjk_width() {
        assert_eq!(fit_line("中文 │ abc", " │ ", 8), "中文");
        assert_eq!(fit_line("中文 │ abc", " │ ", 10), "中文 │ abc");
    }

    #[test]
    fn should_restore_matches_same_path() {
        assert!(should_restore("a/b.jsonl", Some("a/b.jsonl")));
        assert!(!should_restore("", Some("a/b.jsonl"))); // 无持久化状态
        assert!(!should_restore("a/b.jsonl", Some("c/d.jsonl"))); // path 变化
        assert!(!should_restore("a/b.jsonl", None)); // 本次无 transcript
    }

    #[test]
    fn should_checkout_four_states() {
        assert!(!should_checkout(0, Some("/a.jsonl"), Some("/b.jsonl"))); // ts=0 不结账
        assert!(!should_checkout(100, Some(""), Some("/b.jsonl"))); // prev path 为空
        assert!(!should_checkout(100, None, Some("/b.jsonl")));
        assert!(!should_checkout(100, Some("/a.jsonl"), Some("/a.jsonl"))); // 同 path 不重复
        assert!(should_checkout(100, Some("/a.jsonl"), Some("/b.jsonl"))); // 不同 path
        assert!(should_checkout(100, Some("/a.jsonl"), None)); // 新会话无 path 也结账
    }

    #[test]
    fn hud_err_marker_short_and_truncated() {
        let short = hud_err_marker("parse stdin JSON: bad");
        assert!(short.starts_with("[hud err] parse stdin JSON: bad"));
        assert!(short.contains("claude-hud doctor"));

        let long_msg = "x".repeat(200);
        let long = hud_err_marker(&long_msg);
        assert_eq!(
            long,
            format!("[hud err] {} — run 'claude-hud doctor'", "x".repeat(80))
        );
    }

    #[test]
    fn layout_from_mod_widgets_win() {
        let widgets = vec!["model_display".to_string(), "cost_display".to_string()];
        let got = layout_from_mod(Some(&widgets), "minimal").unwrap();
        assert_eq!(got, widgets);
    }

    #[test]
    fn layout_from_mod_minimal_maps() {
        let got = layout_from_mod(None, "minimal").unwrap();
        assert_eq!(got, vec!["model_display", "context_bar", "cost_display", "git_status"]);
    }

    #[test]
    fn layout_from_mod_activity_maps() {
        let got = layout_from_mod(None, "activity").unwrap();
        assert_eq!(got.len(), 7);
        assert_eq!(got[0], "model_display");
        assert_eq!(got[6], "rate_limits");
    }

    #[test]
    fn layout_from_mod_unknown_errors() {
        let err = layout_from_mod(None, "agent-centric").unwrap_err();
        assert!(err.contains("not implemented"));
    }

    #[test]
    fn lines_from_layers_priority() {
        assert_eq!(lines_from_layers(Some(3), Some(2), 1), 3);
        assert_eq!(lines_from_layers(None, Some(2), 1), 2);
        assert_eq!(lines_from_layers(None, None, 1), 1);
    }
}
