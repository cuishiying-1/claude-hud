pub mod agent_detail;
pub mod agent_overview;
pub mod agent_timeline;
pub mod alerts;
pub mod context_bar;
pub mod cost_display;
pub mod git_status;
pub mod model_display;
pub mod rate_limits;
pub mod script_widget;
pub mod session_stats;
pub mod skills_mcp;
pub mod skills_mcp_dynamic;
pub mod token_attribution;
pub mod token_rate;

use crate::core::widget::WidgetRegistry;

/// Register all Phase 1-3 widgets.
pub fn register_all(registry: &mut WidgetRegistry, _config: &crate::core::config::AppConfig) {
    // Phase 1
    registry.register(Box::new(context_bar::ContextBar));
    registry.register(Box::new(model_display::ModelDisplay));
    registry.register(Box::new(cost_display::CostDisplay::new()));
    registry.register(Box::new(agent_overview::AgentOverview));
    registry.register(Box::new(skills_mcp::SkillsMcp));
    registry.register(Box::new(rate_limits::RateLimits));
    registry.register(Box::new(git_status::GitStatusWidget::new(
        crate::core::config::AppConfig::state_path().unwrap_or_default(),
    )));

    // Phase 2
    registry.register(Box::new(agent_detail::AgentDetail::new()));
    registry.register(Box::new(token_attribution::TokenAttribution::new()));
    registry.register(Box::new(agent_timeline::AgentTimeline::new()));
    registry.register(Box::new(session_stats::SessionStats::new()));
    registry.register(Box::new(skills_mcp_dynamic::SkillsMcpDynamic::new()));
    registry.register(Box::new(alerts::Alerts::new()));
    registry.register(Box::new(token_rate::TokenRate::new()));

    // Phase 3: user-registered script widgets are added at runtime
    // via config. Script widget instances are created per-config.
}

/// Create script widgets from configuration.
pub fn register_script_widgets(
    registry: &mut WidgetRegistry,
    config: &crate::core::config::AppConfig,
) {
    let state_path = crate::core::config::AppConfig::state_path().unwrap_or_default();
    for (_name, value) in &config.widgets {
        if let toml::Value::Table(table) = value {
            let widget_type = table
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match widget_type {
                "rhai_script" => {
                    if let Some(path) = table.get("script_path").and_then(|v| v.as_str()) {
                        registry.register(Box::new(
                            script_widget::ScriptWidget::new_rhai(path.to_string(), state_path.clone()),
                        ));
                    }
                }
                "shell_output" => {
                    if let Some(cmd) = table.get("command").and_then(|v| v.as_str()) {
                        let refresh = table
                            .get("refresh_seconds")
                            .and_then(|v| v.as_integer())
                            .unwrap_or(30) as u64;
                        registry.register(Box::new(
                            script_widget::ScriptWidget::new_shell(cmd.to_string(), refresh, state_path.clone()),
                        ));
                    }
                }
                "http_poll" => {
                    if let Some(url) = table.get("url").and_then(|v| v.as_str()) {
                        let refresh = table
                            .get("refresh_seconds")
                            .and_then(|v| v.as_integer())
                            .unwrap_or(30) as u64;
                        registry.register(Box::new(
                            script_widget::ScriptWidget::new_http(url.to_string(), refresh, state_path.clone()),
                        ));
                    }
                }
                _ => {}
            }
        }
    }
}
