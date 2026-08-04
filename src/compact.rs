use std::collections::HashMap;
use std::path::PathBuf;

use crate::alert;
use crate::core::ansi;
use crate::core::config::AppConfig;
use crate::core::i18n::{tr, Language};
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
/// 出厂 agent-centric 布局（obsidian-command：重度代理三行，代理信息前置）。
pub const AGENT_CENTRIC_WIDGETS: [&str; 6] = [
    "agent_overview", "model_display", "context_bar",
    "cost_display", "skills_mcp", "token_rate",
];
/// 出厂 kpi 布局（ember-night：深夜编码双行，成本/token 速率优先）。
pub const KPI_WIDGETS: [&str; 6] = [
    "model_display", "context_bar", "cost_display",
    "token_rate", "agent_overview", "alerts",
];

/// 布局解析：compact_widgets 快照 > 布局 ID 映射（minimal/activity）> 其他。
/// 未知布局 ID 返回 Err（render 报错路径，hud_err_marker 上屏）。
pub fn layout_from_mod(
    compact_widgets: Option<&Vec<String>>,
    layout_compact: &str,
    lang: Language,
    active: bool,
) -> Result<Vec<String>, String> {
    if let Some(widgets) = compact_widgets {
        return Ok(widgets.clone());
    }
    let ids: &[&str] = match layout_compact {
        "minimal" => &MINIMAL_WIDGETS,
        "activity" => &ACTIVITY_WIDGETS,
        "agent-centric" => &AGENT_CENTRIC_WIDGETS,
        "kpi" => &KPI_WIDGETS,
        "contextual" => {
            if active { &ACTIVITY_WIDGETS } else { &MINIMAL_WIDGETS }
        }
        other => {
            return Err(format!(
                "{} '{}' {}",
                tr(lang, "runtime.layout_not_impl"),
                other,
                tr(lang, "runtime.not_implemented")
            ))
        }
    };
    Ok(ids.iter().map(|s| s.to_string()).collect())
}

/// 行数三层优先级：runtime_overrides > mod.layout > theme。
pub fn lines_from_layers(runtime: Option<u8>, mod_lines: Option<u8>, theme: u8) -> u8 {
    runtime.or(mod_lines).unwrap_or(theme)
}

/// 当前生效的 compact widget 数组（mod 灌入优先，fallback config）。
pub fn resolve_compact_layout(config: &AppConfig, active: bool) -> Result<Vec<String>, String> {
    if !config.active_mod.is_empty() {
        if let Ok(pkg) = AppConfig::load_mod(&config.active_mod) {
            return layout_from_mod(
                pkg.compact_widgets.as_ref(),
                pkg.layout.as_ref().map(|l| l.compact.as_str()).unwrap_or(""),
                config.language(),
                active,
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
    let (effective_cost, _) =
        pricing::realtime_cost(data, &pricing::merged_pricing(config));
    alert::send_notifications(
        &fired,
        &data,
        &config.alerts,
        &config.currency_symbol,
        effective_cost,
        config.language(),
    );
    // ⑳ 预算档位：基于实时估算成本（≈），档位单调 + 冷却跨进程去重。
    // 复用上方 realtime_cost 结果（effective_cost），不重复计算。
    let budget_tier = alert::check_budget(
        effective_cost,
        &config.budget,
        config.alerts.cooldown_minutes,
        state.budget_tier,
        &mut cooldown,
        now,
    );
    if let Some(tier) = budget_tier {
        state.budget_tier = tier;
        crate::notify::budget(
            (effective_cost / config.budget.cap_usd) * 100.0,
            config.budget.cap_usd,
            &config.currency_symbol,
            config.language(),
        );
    }
    // ④ 压缩临近通知：复用 compaction_prediction（render 权威，跨进程去重同
    // [alerts] 冷却）。阈值 0 = 关闭；数据不足（None）不触发。
    let eta = summary.compaction_prediction(
        data.context_window.used_percentage,
        data.context_window.context_window_size,
    );
    if alert::check_compaction(
        eta,
        config.alerts.compaction_eta_minutes,
        config.alerts.cooldown_minutes,
        &mut cooldown,
        now,
    ) {
        if let Some(m) = eta {
            crate::notify::compaction(m, config.language());
        }
    }
    state.alerts = cooldown.to_state();

    // ⑨ 会话切换结账：transcript_path 变化 → 上一会话写入历史库（失败仅警告，不中断渲染）。
    // ⑨+ 去重：prev 路径在冷却期内已结账（path→ts 表）→ 跳过，防 double-billing。
    let cooldown_secs = config.alerts.cooldown_minutes * 60;
    if should_checkout(
        state.snapshot.timestamp_secs,
        state.snapshot.transcript_path.as_deref(),
        data.transcript_path.as_deref(),
        &state.checkout_billed,
        now,
        cooldown_secs,
    ) {
        match HistoryStore::open() {
            Ok(h) => {
                let last = state.snapshot.to_session();
                if let Err(e) = h.record_session(&last, state.snapshot.agent_count) {
                    eprintln!("[claude-hud] warning: session checkout failed: {}", e);
                }
            }
            Err(e) => eprintln!("[claude-hud] warning: cannot open history db: {}", e),
        }
        state
            .checkout_billed
            .insert(state.snapshot.transcript_path.clone().unwrap_or_default(), now);
    }
    // 冷却期外的记录不再有用（同 path 再次切换视为新会话）→ 清理，map 有界。
    state
        .checkout_billed
        .retain(|_, ts| now.saturating_sub(*ts) < cooldown_secs);

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
/// ⑨+ 去重：prev path 在 `billed`（path→最近结账时刻）中且冷却期内 → 跳过。
/// 振荡 A→B→A→B 时两 path 交替被记，单槽记忆（只记最后一次）相位错位无法
/// 去重，故按 path 建表：同 path 在冷却期内最多结账一次。
pub fn should_checkout(
    prev_ts: u64,
    prev_path: Option<&str>,
    cur_path: Option<&str>,
    billed: &HashMap<String, u64>,
    now: u64,
    cooldown_secs: u64,
) -> bool {
    prev_ts != 0
        && !prev_path.map(|p| p.is_empty()).unwrap_or(true)
        && prev_path != cur_path
        && !billed
            .get(prev_path.unwrap_or(""))
            .map_or(false, |ts| now.saturating_sub(*ts) < cooldown_secs)
}

/// Build the stdout error marker for render failures. The message is
/// truncated so the marker stays readable in a terminal status line.
pub fn hud_err_marker(msg: &str, lang: Language) -> String {
    let short: String = msg.chars().take(80).collect();
    format!("[hud err] {} {}", short, tr(lang, "runtime.doctor_hint"))
}

/// Render the compact status bar from an already-parsed session snapshot.
/// Shared by `render` (stdin) and `doctor` (sample data). No transcript
/// parsing here — that lives in `run_pipeline`.
pub fn render_with_data(
    data: &SessionData,
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
    _summary: Option<&TranscriptSummary>,
) -> Result<String, String> {
    let active = data
        .subagent_status_line
        .as_ref()
        .map_or(false, |s| !s.agents.is_empty());
    let layout = resolve_compact_layout(config, active)?;
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
                pricing::inject_cost_realtime(data, config, &mut widget_config);
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
pub fn dump_stdin(lang: Language) -> Result<(), String> {
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
    println!("{}: {}", tr(lang, "runtime.recognized"), recognized.join(", "));
    println!(
        "{}: {}",
        tr(lang, "runtime.unknown"),
        if unknown.is_empty() {
            tr(lang, "runtime.none").to_string()
        } else {
            unknown.join(", ")
        }
    );
    println!("{}", tr(lang, "runtime.raw_stdin"));
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
        let none = HashMap::new();
        assert!(!should_checkout(0, Some("/a.jsonl"), Some("/b.jsonl"), &none, 2000, 600)); // ts=0 不结账
        assert!(!should_checkout(100, Some(""), Some("/b.jsonl"), &none, 2000, 600)); // prev path 为空
        assert!(!should_checkout(100, None, Some("/b.jsonl"), &none, 2000, 600));
        assert!(!should_checkout(100, Some("/a.jsonl"), Some("/a.jsonl"), &none, 2000, 600)); // 同 path 不重复
        assert!(should_checkout(100, Some("/a.jsonl"), Some("/b.jsonl"), &none, 2000, 600)); // 不同 path
        assert!(should_checkout(100, Some("/a.jsonl"), None, &none, 2000, 600)); // 新会话无 path 也结账
    }

    #[test]
    fn checkout_skips_rebilling_same_path_within_cooldown() {
        // prev 路径已在冷却期内结账（path→ts 表）→ 跳过
        let billed = HashMap::from([("/a".to_string(), 1000)]);
        assert!(!should_checkout(100, Some("/a"), Some("/b"), &billed, 1200, 600));
        // 不同路径正常结账
        let billed_b = HashMap::from([("/a".to_string(), 1000)]);
        assert!(should_checkout(100, Some("/b"), Some("/c"), &billed_b, 1200, 600));
        // 从未结账（空表）不挡
        let empty = HashMap::new();
        assert!(should_checkout(100, Some("/a"), Some("/b"), &empty, 1200, 600));
        // 冷却过期（600s 窗口外）放行
        let stale = HashMap::from([("/a".to_string(), 1000)]);
        assert!(should_checkout(100, Some("/a"), Some("/b"), &stale, 1700, 600));
        // 边界：恰好 600s 视为过期（< 判定）
        assert!(should_checkout(100, Some("/a"), Some("/b"), &stale, 1600, 600));
    }

    #[test]
    fn checkout_oscillation_bills_each_path_once() {
        // A→B→A→B：首轮后两 path 均在冷却期内 → 交替不再记账（防 double-billing）
        let billed = HashMap::from([
            ("/a".to_string(), 1000),
            ("/b".to_string(), 1100),
        ]);
        assert!(!should_checkout(100, Some("/a"), Some("/b"), &billed, 1200, 600));
        assert!(!should_checkout(100, Some("/b"), Some("/a"), &billed, 1300, 600));
        // 冷却期外的新一轮切换视为新会话
        let stale = HashMap::from([
            ("/a".to_string(), 1000),
            ("/b".to_string(), 1100),
        ]);
        assert!(should_checkout(100, Some("/a"), Some("/b"), &stale, 1800, 600));
    }

    #[test]
    fn hud_err_marker_short_and_truncated() {
        let short = hud_err_marker("parse stdin JSON: bad", Language::En);
        assert!(short.starts_with("[hud err] parse stdin JSON: bad"));
        assert!(short.contains("claude-hud doctor"));

        let long_msg = "x".repeat(200);
        let long = hud_err_marker(&long_msg, Language::En);
        assert_eq!(
            long,
            format!("[hud err] {} — run 'claude-hud doctor'", "x".repeat(80))
        );
    }

    #[test]
    fn layout_from_mod_widgets_win() {
        let widgets = vec!["model_display".to_string(), "cost_display".to_string()];
        let got = layout_from_mod(Some(&widgets), "minimal", Language::En, false).unwrap();
        assert_eq!(got, widgets);
    }

    #[test]
    fn layout_from_mod_minimal_maps() {
        let got = layout_from_mod(None, "minimal", Language::En, false).unwrap();
        assert_eq!(got, vec!["model_display", "context_bar", "cost_display", "git_status"]);
    }

    #[test]
    fn layout_from_mod_activity_maps() {
        let got = layout_from_mod(None, "activity", Language::En, false).unwrap();
        assert_eq!(got.len(), 7);
        assert_eq!(got[0], "model_display");
        assert_eq!(got[6], "rate_limits");
    }

    #[test]
    fn layout_from_mod_agent_centric_maps() {
        let got = layout_from_mod(None, "agent-centric", Language::En, false).unwrap();
        assert_eq!(
            got,
            vec!["agent_overview", "model_display", "context_bar",
                 "cost_display", "skills_mcp", "token_rate"]
        );
    }

    #[test]
    fn layout_from_mod_kpi_maps() {
        let got = layout_from_mod(None, "kpi", Language::En, false).unwrap();
        assert_eq!(
            got,
            vec!["model_display", "context_bar", "cost_display",
                 "token_rate", "agent_overview", "alerts"]
        );
    }

    #[test]
    fn layout_from_mod_contextual_idle_maps_minimal() {
        let got = layout_from_mod(None, "contextual", Language::En, false).unwrap();
        assert_eq!(
            got,
            vec!["model_display", "context_bar", "cost_display", "git_status"]
        );
    }

    #[test]
    fn layout_from_mod_contextual_active_maps_activity() {
        let got = layout_from_mod(None, "contextual", Language::En, true).unwrap();
        assert_eq!(
            got,
            vec!["model_display", "context_bar", "agent_overview",
                 "git_status", "skills_mcp", "cost_display", "rate_limits"]
        );
    }

    #[test]
    fn layout_from_mod_unknown_errors() {
        let err = layout_from_mod(None, "hex-2x3", Language::En, false).unwrap_err();
        assert!(err.contains("not implemented"));
    }

    #[test]
    fn lines_from_layers_priority() {
        assert_eq!(lines_from_layers(Some(3), Some(2), 1), 3);
        assert_eq!(lines_from_layers(None, Some(2), 1), 2);
        assert_eq!(lines_from_layers(None, None, 1), 1);
    }
}
