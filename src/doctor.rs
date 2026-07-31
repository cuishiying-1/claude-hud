use crate::compact;
use crate::core::config::AppConfig;
use crate::core::session::SessionData;
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
    compact::render_with_data(&data, registry, config, theme)
}

fn check(label: &str, ok: bool, ok_detail: &str, hint: &str) -> usize {
    if ok {
        println!("  [ok] {}: {}", label, ok_detail);
    } else {
        println!("  [!!] {}: fix: {}", label, hint);
    }
    usize::from(!ok)
}
