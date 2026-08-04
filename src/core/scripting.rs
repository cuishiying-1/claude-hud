use rhai::{Engine, Scope};
use std::fs;

use super::session::SessionData;
use super::theme::Theme;

/// Rhai scripting engine wrapper for user-defined widgets.
pub struct ScriptEngine {
    engine: Engine,
}

impl ScriptEngine {
    pub fn new() -> Self {
        let engine = Engine::new();
        Self { engine }
    }

    /// Execute a Rhai script file and return the rendered output.
    pub fn run_widget_script(
        &self,
        script_path: &str,
        data: &SessionData,
        theme: &Theme,
    ) -> Result<String, String> {
        let source = fs::read_to_string(script_path)
            .map_err(|e| format!("read script '{}': {}", e, script_path))?;

        let mut scope = Scope::new();

        // Inject session data as a Rhai object map
        let data_map = rhai::Map::from_iter([
            ("model_id".into(), rhai::Dynamic::from(data.model.id.clone())),
            ("model_name".into(), rhai::Dynamic::from(data.model.display_name.clone())),
            ("context_pct".into(), rhai::Dynamic::from(data.context_window.used_percentage)),
            ("input_tokens".into(), rhai::Dynamic::from(data.context_window.total_input_tokens as i64)),
            ("output_tokens".into(), rhai::Dynamic::from(data.context_window.total_output_tokens as i64)),
            ("cost_usd".into(), rhai::Dynamic::from(data.cost.total_cost_usd)),
            ("duration_ms".into(), rhai::Dynamic::from(data.cost.total_duration_ms as i64)),
            ("lines_added".into(), rhai::Dynamic::from(data.cost.total_lines_added as i64)),
            ("lines_removed".into(), rhai::Dynamic::from(data.cost.total_lines_removed as i64)),
            ("rate_5h_pct".into(), rhai::Dynamic::from(data.rate_limits.five_hour.used_percentage)),
            ("rate_7d_pct".into(), rhai::Dynamic::from(data.rate_limits.seven_day.used_percentage)),
        ]);
        scope.push("data", data_map);

        // Inject theme colors
        let theme_map = rhai::Map::from_iter([
            ("bg".into(), rhai::Dynamic::from(theme.bg.clone())),
            ("fg".into(), rhai::Dynamic::from(theme.fg.clone())),
            ("accent".into(), rhai::Dynamic::from(theme.accent.clone())),
            ("success".into(), rhai::Dynamic::from(theme.success.clone())),
            ("warning".into(), rhai::Dynamic::from(theme.warning.clone())),
            ("danger".into(), rhai::Dynamic::from(theme.danger.clone())),
            ("muted".into(), rhai::Dynamic::from(theme.muted.clone())),
        ]);
        scope.push("theme", theme_map);

        // Helper functions for ANSI output
        let engine = &self.engine;
        // Note: Rhai inline functions would be registered here

        let result: String = engine
            .eval_with_scope(&mut scope, &source)
            .map_err(|e| format!("script error: {}", e))?;

        Ok(result)
    }
}

impl Default for ScriptEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Execute a shell command. On Windows use `cmd /C`, elsewhere `sh -c`.
pub fn run_shell_command(command: &str) -> Result<String, String> {
    use std::process::Command;
    #[cfg(windows)]
    let mut cmd = Command::new("cmd");
    #[cfg(not(windows))]
    let mut cmd = Command::new("sh");
    #[cfg(windows)]
    cmd.arg("/C").arg(command);
    #[cfg(not(windows))]
    cmd.arg("-c").arg(command);
    let output = cmd
        .output()
        .map_err(|e| format!("shell command failed: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("shell error: {}", stderr.trim()))
    }
}

/// Poll an HTTP endpoint and return the response body.
pub fn http_poll(url: &str) -> Result<String, String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("http request failed: {}", e))?;
    response
        .into_string()
        .map_err(|e| format!("read response: {}", e))
}
