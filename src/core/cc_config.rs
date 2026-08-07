use serde_json::{Map, Value};

/// Merge the Claude HUD statusLine into Claude Code settings.json content.
/// Returns the pretty-printed merged JSON. Empty input starts from {}.
pub fn merge_status_line(existing: &str, command: &str) -> Result<String, String> {
    let mut root = parse_root(existing)?;
    if root.is_null() {
        root = Value::Object(Map::new());
    }
    if !root.is_object() {
        return Err("settings.json must be a JSON object at top level".to_string());
    }
    root["statusLine"] = serde_json::json!({
        "type": "command",
        "command": command,
        "refreshInterval": 5
    });
    pretty(&root)
}

/// statusLine 命令：当前可执行文件完整路径 + render。
/// Windows 用正斜杠——Claude Code 以 bash 执行 statusLine，反斜杠路径会被
/// 当作转义序列，且 bash 无法解析裸名 .cmd（本地 stub 模式），
/// 完整 exe 路径两者都规避。
pub fn default_status_line_command() -> String {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "claude-hud".to_string());
    let exe = if cfg!(windows) {
        exe.replace('\\', "/")
    } else {
        exe
    };
    format!("{} render", exe)
}

/// Remove the Claude HUD statusLine key from settings.json content.
/// Returns the pretty-printed JSON without the statusLine key.
pub fn remove_status_line(existing: &str) -> Result<String, String> {
    let mut root = parse_root(existing)?;
    if root.is_null() {
        root = Value::Object(Map::new());
    }
    if let Some(obj) = root.as_object_mut() {
        obj.remove("statusLine");
    }
    pretty(&root)
}

/// True when the settings JSON contains a statusLine key (any shape).
/// Unparseable JSON returns false.
pub fn has_status_line(existing: &str) -> bool {
    match serde_json::from_str::<Value>(existing) {
        Ok(v) => v.get("statusLine").is_some(),
        Err(_) => false,
    }
}

pub const ENV_WINDOW_KEY: &str = "CLAUDE_CODE_MAX_CONTEXT_TOKENS";

/// 在 settings.json 的 env 块写入 CLAUDE_CODE_MAX_CONTEXT_TOKENS（字符串值）。
/// env 不存在则创建；其他 env 键与顶层键全部保留。
pub fn set_env_window(existing: &str, window: u64) -> Result<String, String> {
    let mut root = parse_root(existing)?;
    if root.is_null() {
        root = Value::Object(Map::new());
    }
    if !root.is_object() {
        return Err("settings.json must be a JSON object at top level".to_string());
    }
    let mut env = match root.get("env") {
        None => Value::Object(Map::new()),
        Some(v) if v.is_object() => v.clone(),
        Some(_) => return Err("settings.json env must be a JSON object".to_string()),
    };
    if let Some(obj) = env.as_object_mut() {
        obj.insert(ENV_WINDOW_KEY.to_string(), Value::String(window.to_string()));
    }
    root["env"] = env;
    pretty(&root)
}

/// 从 settings.json env 块移除 CLAUDE_CODE_MAX_CONTEXT_TOKENS；env 变空则整块删除。
pub fn remove_env_window(existing: &str) -> Result<String, String> {
    let mut root = parse_root(existing)?;
    if root.is_null() {
        root = Value::Object(Map::new());
    }
    if let Some(env) = root.get_mut("env") {
        if let Some(obj) = env.as_object_mut() {
            obj.remove(ENV_WINDOW_KEY);
        }
        if env.as_object().map(|o| o.is_empty()).unwrap_or(false) {
            if let Some(obj) = root.as_object_mut() {
                obj.remove("env");
            }
        }
    }
    pretty(&root)
}

/// 读取 env 块中的窗口值；缺失/解析失败 → None。
pub fn get_env_window(existing: &str) -> Option<String> {
    let root = parse_root(existing).ok()?;
    root.get("env")?.get(ENV_WINDOW_KEY)?.as_str().map(String::from)
}

fn parse_root(existing: &str) -> Result<Value, String> {
    if existing.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_str(existing).map_err(|e| format!("parse settings.json: {}", e))
}

fn pretty(root: &Value) -> Result<String, String> {
    serde_json::to_string_pretty(root).map_err(|e| format!("serialize settings.json: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECTED_COMMAND: &str = "\"command\": \"claude-hud render\"";

    #[test]
    fn merge_empty_input_creates_status_line() {
        let out = merge_status_line("", "claude-hud render").unwrap();
        assert!(out.contains(EXPECTED_COMMAND));
        assert!(out.contains("\"statusLine\""));
    }

    #[test]
    fn merge_preserves_existing_keys() {
        let out = merge_status_line(r#"{"apiKeyHelper":{"alwaysAllowedTools":[]}}"#, "claude-hud render").unwrap();
        let root: Value = serde_json::from_str(&out).unwrap();
        assert!(root.get("apiKeyHelper").is_some());
        assert!(root.get("statusLine").is_some());
        assert_eq!(root["statusLine"]["command"], "claude-hud render");
        assert_eq!(root["statusLine"]["refreshInterval"], 5);
    }

    #[test]
    fn merge_replaces_existing_status_line_without_duplication() {
        let out = merge_status_line(r#"{"statusLine":{"type":"command","command":"old-cmd","refreshInterval":1}}"#, "claude-hud render").unwrap();
        let root: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(root["statusLine"]["command"], "claude-hud render");
        assert_eq!(root.as_object().unwrap().get("statusLine").unwrap().as_object().unwrap().len(), 3);
    }

    #[test]
    fn merge_invalid_json_returns_err() {
        assert!(merge_status_line("{not json", "claude-hud render").is_err());
    }

    #[test]
    fn remove_empty_status_line_keeps_other_keys() {
        let out = remove_status_line(r#"{"statusLine":{},"permissions":{}}"#).unwrap();
        let root: Value = serde_json::from_str(&out).unwrap();
        assert!(root.get("statusLine").is_none());
        assert!(root.get("permissions").is_some());
    }

    #[test]
    fn remove_missing_status_line_is_noop() {
        let out = remove_status_line(r#"{"permissions":{}}"#).unwrap();
        let root: Value = serde_json::from_str(&out).unwrap();
        assert!(root.get("permissions").is_some());
    }

    #[test]
    fn remove_empty_input_returns_empty_object() {
        let out = remove_status_line("").unwrap();
        assert_eq!(out, "{}");
    }

    #[test]
    fn merge_non_object_root_returns_err() {
        assert!(merge_status_line("[1,2,3]", "claude-hud render").is_err());
        assert!(merge_status_line("\"foo\"", "claude-hud render").is_err());
    }

    #[test]
    fn merge_null_root_is_treated_as_empty_object() {
        let out = merge_status_line("null", "claude-hud render").unwrap();
        assert!(out.contains("\"statusLine\""));
    }

    #[test]
    fn merge_uses_given_command() {
        let out = merge_status_line("", "D:/hud/claude-hud.exe render").unwrap();
        assert!(out.contains("\"command\": \"D:/hud/claude-hud.exe render\""));
    }

    #[test]
    fn default_status_line_command_has_exe_and_render() {
        let cmd = default_status_line_command();
        assert!(cmd.ends_with(" render"), "got: {}", cmd);
        assert!(cmd.contains("claude-hud"), "got: {}", cmd);
        if cfg!(windows) {
            assert!(!cmd.contains('\\'), "windows path must use forward slashes: {}", cmd);
        }
    }

    #[test]
    fn remove_null_root_returns_empty_object() {
        let out = remove_status_line("null").unwrap();
        assert_eq!(out, "{}");
    }

    #[test]
    fn has_status_line_present_any_shape() {
        assert!(has_status_line(r#"{"statusLine":{}}"#));
        assert!(has_status_line(
            r#"{"statusLine":{"type":"command","command":"old-cmd"}}"#
        ));
    }

    #[test]
    fn has_status_line_absent() {
        assert!(!has_status_line(r#"{"permissions":{}}"#));
        assert!(!has_status_line(""));
        assert!(!has_status_line("{}"));
    }

    #[test]
    fn has_status_line_invalid_json_is_false() {
        assert!(!has_status_line("{not json"));
    }

    #[test]
    fn set_env_window_creates_block() {
        let out = set_env_window("{}", 1_000_000).unwrap();
        let root: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(root["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "1000000");
    }

    #[test]
    fn set_env_window_preserves_other_keys_and_env() {
        let out = set_env_window(
            r#"{"apiKeyHelper":{"alwaysAllowedTools":[]},"env":{"ANTHROPIC_BASE_URL":"https://x"}}"#,
            1_000_000,
        )
        .unwrap();
        let root: Value = serde_json::from_str(&out).unwrap();
        assert!(root.get("apiKeyHelper").is_some(), "other top-level keys kept");
        assert_eq!(root["env"]["ANTHROPIC_BASE_URL"], "https://x", "other env kept");
        assert_eq!(root["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "1000000");
    }

    #[test]
    fn set_env_window_replaces_existing_value() {
        let out = set_env_window(r#"{"env":{"CLAUDE_CODE_MAX_CONTEXT_TOKENS":"200000"}}"#, 500_000).unwrap();
        let root: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(root["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "500000");
        assert_eq!(root["env"].as_object().unwrap().len(), 1, "no duplication");
    }

    #[test]
    fn set_env_window_invalid_inputs_err() {
        assert!(set_env_window("{not json", 1).is_err());
        assert!(set_env_window("[1,2]", 1).is_err(), "non-object root");
        assert!(set_env_window(r#"{"env": 42}"#, 1).is_err(), "non-object env");
    }

    #[test]
    fn remove_env_window_removes_key_keeps_env() {
        let out = remove_env_window(r#"{"env":{"CLAUDE_CODE_MAX_CONTEXT_TOKENS":"1000000","X":"y"}}"#).unwrap();
        let root: Value = serde_json::from_str(&out).unwrap();
        assert!(root["env"].get("CLAUDE_CODE_MAX_CONTEXT_TOKENS").is_none());
        assert_eq!(root["env"]["X"], "y", "other env kept");
    }

    #[test]
    fn remove_env_window_drops_empty_env_block() {
        let out = remove_env_window(r#"{"env":{"CLAUDE_CODE_MAX_CONTEXT_TOKENS":"1000000"}}"#).unwrap();
        let root: Value = serde_json::from_str(&out).unwrap();
        assert!(root.get("env").is_none(), "empty env removed");
    }

    #[test]
    fn remove_env_window_missing_is_noop() {
        let out = remove_env_window(r#"{"permissions":{}}"#).unwrap();
        let root: Value = serde_json::from_str(&out).unwrap();
        assert!(root.get("permissions").is_some());
        assert!(root.get("env").is_none());
    }

    #[test]
    fn get_env_window_reads_value() {
        assert_eq!(get_env_window(r#"{"env":{"CLAUDE_CODE_MAX_CONTEXT_TOKENS":"1000000"}}"#),
                   Some("1000000".to_string()));
        assert_eq!(get_env_window(r#"{"permissions":{}}"#), None);
        assert_eq!(get_env_window(""), None);
        assert_eq!(get_env_window("{bad"), None, "unparseable → None");
    }
}
