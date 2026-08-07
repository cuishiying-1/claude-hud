use crate::compact;
use crate::core::config::AppConfig;
use crate::core::i18n::{tr, Language};
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
    let lang = config.language();
    let mut failures = 0usize;

    let exe = std::env::current_exe().unwrap_or_default();
    failures += check(
        lang,
        tr(lang, "runtime.d_check_binary"),
        true,
        &format!("{} (v{})", exe.display(), env!("CARGO_PKG_VERSION")),
        "",
    );
    let config_ok = match AppConfig::config_path() {
        Ok(p) => p.exists() && AppConfig::load().is_ok(),
        Err(_) => false,
    };
    failures += check(
        lang,
        tr(lang, "runtime.d_check_config"),
        config_ok,
        tr(lang, "runtime.d_config_ok"),
        tr(lang, "runtime.d_config_hint"),
    );
    failures += check(
        lang,
        tr(lang, "runtime.d_check_statusline"),
        status_line_ok(),
        tr(lang, "runtime.d_statusline_ok"),
        tr(lang, "runtime.d_statusline_hint"),
    );

    let state_path = AppConfig::state_path();
    let state_ok = state_path
        .as_ref()
        .map(|p| !p.exists() || StateFile::read(p).snapshot.timestamp_secs != 0)
        .unwrap_or(true);
    failures += check(
        lang,
        tr(lang, "runtime.d_check_state"),
        state_ok,
        tr(lang, "runtime.d_state_ok"),
        tr(lang, "runtime.d_state_hint"),
    );

    let last_err = match &state_path {
        Ok(p) => StateFile::read(p).last_error,
        Err(_) => None,
    };
    failures += check(
        lang,
        tr(lang, "runtime.d_check_last_render"),
        last_err.is_none(),
        tr(lang, "runtime.d_last_render_ok"),
        tr(lang, "runtime.d_last_render_hint"),
    );
    if let Some(le) = &last_err {
        println!(
            "{} {}: {}",
            tr(lang, "runtime.d_last_failure"),
            le.ts_iso,
            le.msg
        );
    }

    failures += check(
        lang,
        tr(lang, "runtime.d_check_icon"),
        true,
        &format!("{:?}", theme.icon_set),
        "",
    );

    match crate::probe::git::probe_git() {
        Some(s) => {
            println!(
                "{}",
                tr(lang, "runtime.d_git_ok").replace("{branch}", &s.branch)
            )
        }
        None => println!("{}", tr(lang, "runtime.d_git_degraded")),
    }

    failures += check(
        lang,
        tr(lang, "runtime.d_check_sample"),
        sample_render(registry, config, theme).is_ok(),
        tr(lang, "runtime.d_sample_ok"),
        tr(lang, "runtime.d_sample_hint"),
    );

    contract_probe();
    pricing_check(config, lang, &mut failures);
    model_check(config, lang, &mut failures);
    budget_check(lang);
    let lang_ok = Language::from_str(&config.language).is_some();
    failures += check(
        lang,
        "language",
        lang_ok,
        &format!("{}", config.language),
        "valid values: en, zh",
    );
    update_check(lang);

    if failures == 0 {
        println!("{}", tr(lang, "runtime.doctor_all_passed"));
        Ok(())
    } else {
        Err(tr(lang, "runtime.d_failed").replace("{n}", &failures.to_string()))
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
        Some(serde_json::Value::String(cmd)) => {
            cmd.contains("claude-hud") && cmd.contains(" render")
        }
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
/// ① 末尾追加内置表信息项（与用户表无关，恒显示）。
fn pricing_check(config: &AppConfig, lang: Language, failures: &mut usize) {
    if config.pricing.is_empty() {
        println!("{}", tr(lang, "runtime.d_no_pricing"));
    } else {
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
                "{}",
                tr(lang, "runtime.d_pricing_ok").replace("{n}", &config.pricing.len().to_string())
            );
        } else {
            println!(
                "{}",
                tr(lang, "runtime.d_pricing_neg").replace("{models}", &bad.join(", "))
            );
            *failures += 1;
        }
    }
    println!(
        "{}",
        tr(lang, "runtime.d_builtin_pricing")
            .replace("{n}", &crate::core::pricing::builtin_pricing().len().to_string())
    );
}

/// v0.7 [models] 校验（纯函数，可单测）：返回 (坏窗口模型, 负价模型)。
/// 窗口 ≤0 / 任币种任价格 <0 计入；字段缺失（None）不算。
fn model_issues(config: &AppConfig) -> (Vec<String>, Vec<String>) {
    let bad_window: Vec<String> = config
        .models
        .iter()
        .filter(|(_, m)| m.context_window == Some(0))
        .map(|(id, _)| id.clone())
        .collect();
    let bad_price: Vec<String> = config
        .models
        .iter()
        .filter(|(_, m)| {
            [m.price_usd.as_ref(), m.price_cny.as_ref()].into_iter().flatten().any(|p| {
                p.input < 0.0 || p.output < 0.0 || p.cache_read < 0.0 || p.cache_creation < 0.0
            })
        })
        .map(|(id, _)| id.clone())
        .collect();
    (bad_window, bad_price)
}

/// v0.7 [models] 检查（信息项 + failure）：窗口 ≤0 / 负价 → failure；
/// 信息项：内置表数量 + 各 synced_at 条目来源时间。
fn model_check(config: &AppConfig, lang: Language, failures: &mut usize) {
    let (bad_window, bad_price) = model_issues(config);
    if !bad_window.is_empty() {
        println!(
            "{}",
            tr(lang, "runtime.d_model_window_zero")
                .replace("{models}", &bad_window.join(", "))
        );
        *failures += 1;
    }
    if !bad_price.is_empty() {
        println!(
            "{}",
            tr(lang, "runtime.d_model_price_neg")
                .replace("{models}", &bad_price.join(", "))
        );
        *failures += 1;
    }
    println!(
        "{}",
        tr(lang, "runtime.d_builtin_models")
            .replace("{n}", &crate::core::pricing::builtin_models().len().to_string())
    );
    for (id, m) in &config.models {
        if let Some(ts) = &m.synced_at {
            println!(
                "{}",
                tr(lang, "runtime.d_model_synced").replace("{id}", id).replace("{ts}", ts)
            );
        }
    }
}

/// ⑱ 升级检查（信息项，永不计数为 failure）。
fn update_check(lang: Language) {
    let status = crate::core::update::check_update();
    match &status {
        crate::core::update::UpdateStatus::UpToDate(v) => {
            println!("{}", tr(lang, "runtime.d_update_ok").replace("{v}", v))
        }
        crate::core::update::UpdateStatus::Available(v) => {
            println!("{}", tr(lang, "runtime.d_update_avail").replace("{v}", v))
        }
        _ => println!("  [..] update: {}", crate::core::update::describe(&status, lang)),
    }
}

fn check(lang: Language, label: &str, ok: bool, ok_detail: &str, hint: &str) -> usize {
    if ok {
        println!("  [ok] {}: {}", label, ok_detail);
    } else {
        println!(
            "{}",
            tr(lang, "runtime.d_fail_line")
                .replace("{label}", label)
                .replace("{hint}", hint)
        );
    }
    usize::from(!ok)
}

/// ⑳ 预算/告警冷却状态（信息项，恒 exit 0）：读 state.json 的
/// alerts 冷却记录 + budget_tier（单调最高档位）。
fn budget_check(lang: Language) {
    let state_path = match AppConfig::state_path() {
        Ok(p) => p,
        Err(_) => {
            println!("{}", tr(lang, "runtime.d_budget_no_state"));
            return;
        }
    };
    let state = StateFile::read(&state_path);
    if state.budget_tier == 0 && state.alerts.is_empty() {
        println!("{}", tr(lang, "runtime.d_budget_no_records"));
        return;
    }
    let now = crate::core::state::now_secs();
    for (kind, ts) in &state.alerts {
        println!(
            "  [..] alerts: {:?} last fired {}s ago",
            kind,
            now.saturating_sub(*ts)
        );
    }
    if state.budget_tier > 0 {
        println!(
            "{}",
            tr(lang, "runtime.d_budget_tier").replace("{tier}", &state.budget_tier.to_string())
        );
    }
}

#[cfg(test)]
mod tests {
    use super::model_issues;
    use crate::core::config::AppConfig;
    use crate::core::pricing::{ModelEntry, PriceEntry};

    fn cfg_with(entries: Vec<(String, ModelEntry)>) -> AppConfig {
        let mut c = AppConfig::default();
        c.models = entries.into_iter().collect();
        c
    }

    #[test]
    fn model_issues_clean_config() {
        let cfg = cfg_with(vec![(
            "m".into(),
            ModelEntry { context_window: Some(1000), ..Default::default() },
        )]);
        let (bad_window, bad_price) = model_issues(&cfg);
        assert!(bad_window.is_empty() && bad_price.is_empty());
    }

    #[test]
    fn model_issues_zero_window_and_negative_price() {
        let cfg = cfg_with(vec![
            ("a".into(), ModelEntry { context_window: Some(0), ..Default::default() }),
            (
                "b".into(),
                ModelEntry {
                    price_cny: Some(PriceEntry { input: -1.0e-6, ..Default::default() }),
                    ..Default::default()
                },
            ),
        ]);
        let (bad_window, bad_price) = model_issues(&cfg);
        assert_eq!(bad_window, vec!["a".to_string()]);
        assert_eq!(bad_price, vec!["b".to_string()]);
    }

    #[test]
    fn model_issues_ignores_absent_optional_fields() {
        let cfg = cfg_with(vec![("m".into(), ModelEntry::default())]);
        let (bad_window, bad_price) = model_issues(&cfg);
        assert!(bad_window.is_empty() && bad_price.is_empty(), "all-None entry is fine");
    }
}
