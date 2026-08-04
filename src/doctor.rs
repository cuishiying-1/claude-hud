use crate::compact;
use crate::core::config::AppConfig;
use crate::core::session::SessionData;
use crate::core::state::StateFile;
use crate::core::theme::Theme;
use crate::core::widget::WidgetRegistry;

/// Run all self-checks, print a report, and return Err with the failure
/// count when any check fails (main exits non-zero).
pub fn run(
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> Result<(), String> {
    let mut failures = 0usize;

    let exe = std::env::current_exe().unwrap_or_default();
    failures += check(
        "binary",
        true,
        &format!("{} (v{})", exe.display(), env!("CARGO_PKG_VERSION")),
        "",
    );
    let config_ok = match AppConfig::config_path() {
        Ok(p) => p.exists() && AppConfig::load().is_ok(),
        Err(_) => false,
    };
    failures += check(
        "config.toml",
        config_ok,
        "exists and parses",
        "run 'claude-hud setup' to create it",
    );
    failures += check(
        "statusLine configured",
        status_line_ok(),
        "points at claude-hud render",
        "run 'claude-hud setup' to merge it into ~/.claude/settings.json",
    );

    let state_path = AppConfig::state_path();
    let state_ok = state_path
        .as_ref()
        .map(|p| !p.exists() || StateFile::read(p).snapshot.timestamp_secs != 0)
        .unwrap_or(true);
    failures += check(
        "state.json",
        state_ok,
        "exists and parses (missing = never rendered yet)",
        "run 'claude-hud render' once with real stdin JSON",
    );

    let last_err = match &state_path {
        Ok(p) => StateFile::read(p).last_error,
        Err(_) => None,
    };
    failures += check(
        "last render",
        last_err.is_none(),
        "no recorded failure",
        "inspect state.json last_error, then run 'claude-hud render' to clear",
    );
    if let Some(le) = &last_err {
        println!("    last failure at {}: {}", le.ts_iso, le.msg);
    }

    failures += check("icon set", true, &format!("{:?}", theme.icon_set), "");

    match crate::probe::git::probe_git() {
        Some(s) => {
            println!("  [ok] git: branch '{}' readable", s.branch)
        }
        None => println!(
            "  [..] git: unavailable or not a repo (widget degrades silently)"
        ),
    }

    failures += check(
        "sample render",
        sample_render(registry, config, theme).is_ok(),
        "renders without panic",
        "check 'claude-hud render' with real stdin JSON",
    );

    contract_probe();
    pricing_check(config, &mut failures);
    update_check();

    if failures == 0 {
        println!("All checks passed.");
        Ok(())
    } else {
        Err(format!(
            "{} check(s) failed — see hints above",
            failures
        ))
    }
}

fn status_line_ok() -> bool {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return false,
    };
    let path = home.join(".claude").join("settings.json");
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let root: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return false,
    };
    match root.get("statusLine").and_then(|v| v.get("command")) {
        Some(serde_json::Value::String(cmd)) => cmd.contains("claude-hud render"),
        _ => false,
    }
}

fn sample_render(
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> Result<String, String> {
    let sample = serde_json::json!({
        "model": {"id": "test", "display_name": "Test"},
        "context_window": {
            "used_percentage": 50,
            "total_input_tokens": 1000,
            "context_window_size": 200000
        },
        "cost": {"total_cost_usd": 0.1, "total_duration_ms": 60000}
    });
    let data = SessionData::from_stdin_json(&sample.to_string())
        .map_err(|e| format!("parse sample JSON: {}", e))?;
    compact::render_with_data(&data, registry, config, theme, None)
}

/// 契约探针（信息项）：内置双命名样例各一份，解析后报告各顶层键识别
/// 状态。未知键不算 failure——探针的目的就是暴露未来契约漂移。
fn contract_probe() {
    let known = [
        "model", "context_window", "cost", "rate_limits",
        "transcript_path", "subagent_status_line", "subagentStatusLine",
    ];
    let model = serde_json::json!({"id": "probe", "display_name": "Probe"});
    let ctx = serde_json::json!({
        "used_percentage": 1,
        "total_input_tokens": 1,
        "total_output_tokens": 1,
        "context_window_size": 200000
    });
    let cost = serde_json::json!({"total_cost_usd": 0.0, "total_duration_ms": 0});
    let samples = [
        (
            "snake_case",
            serde_json::json!({
                "model": model,
                "context_window": ctx,
                "cost": cost,
                "rate_limits": {
                    "five_hour": {"used_percentage": 0},
                    "seven_day": {"used_percentage": 0}
                },
                "transcript_path": null,
                "subagent_status_line": {"agents": []}
            }),
        ),
        (
            "camelCase",
            serde_json::json!({
                "model": model,
                "context_window": ctx,
                "cost": cost,
                "rate_limits": {"five_hour_pct": 0, "seven_day_pct": 0},
                "subagentStatusLine": {"agents": []}
            }),
        ),
    ];
    for (label, obj) in samples {
        let parses = SessionData::from_stdin_json(&obj.to_string()).is_ok();
        let unknown: Vec<String> = obj
            .as_object()
            .map(|m| {
                m.keys()
                    .filter(|k| !known.contains(&k.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        println!(
            "  [{}] contract probe {}: parses={} unknown_keys={:?}",
            if parses { "ok" } else { ".." },
            label,
            parses,
            unknown
        );
    }
}

/// ⑭ [pricing] 校验：负单价为 failure（含模型名定位）；否则信息项。
fn pricing_check(config: &AppConfig, failures: &mut usize) {
    if config.pricing.is_empty() {
        println!("  [..] pricing: no [pricing] table (cost shown from official data)");
        return;
    }
    let bad: Vec<&str> = config
        .pricing
        .iter()
        .filter(|(_, p)| {
            p.input < 0.0 || p.output < 0.0 || p.cache_read < 0.0 || p.cache_creation < 0.0
        })
        .map(|(m, _)| m.as_str())
        .collect();
    if bad.is_empty() {
        println!(
            "  [ok] pricing: {} model(s) configured, prices non-negative",
            config.pricing.len()
        );
    } else {
        println!(
            "  [!!] pricing: negative price for model(s): {}",
            bad.join(", ")
        );
        *failures += 1;
    }
}

/// ⑱ 升级检查（信息项，永不计数为 failure）。
fn update_check() {
    let status = crate::core::update::check_update();
    match &status {
        crate::core::update::UpdateStatus::UpToDate(v) => {
            println!("  [ok] update: up to date (v{})", v)
        }
        crate::core::update::UpdateStatus::Available(v) => {
            println!(
                "  [ok] update: update available v{} — re-run the install script",
                v
            )
        }
        _ => println!("  [..] update: {}", crate::core::update::describe(&status)),
    }
}

fn check(label: &str, ok: bool, ok_detail: &str, hint: &str) -> usize {
    if ok {
        println!("  [ok] {}: {}", label, ok_detail);
    } else {
        println!("  [!!] {}: fix: {}", label, hint);
    }
    usize::from(!ok)
}
