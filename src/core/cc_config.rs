use serde_json::{Map, Value};

/// Merge the Claude HUD statusLine into Claude Code settings.json content.
/// Returns the pretty-printed merged JSON. Empty input starts from {}.
pub fn merge_status_line(existing: &str) -> Result<String, String> {
    let mut root = parse_root(existing)?;
    if root.is_null() {
        root = Value::Object(Map::new());
    }
    if !root.is_object() {
        return Err("settings.json must be a JSON object at top level".to_string());
    }
    root["statusLine"] = serde_json::json!({
        "type": "command",
        "command": "claude-hud render",
        "refreshInterval": 5
    });
    pretty(&root)
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
        let out = merge_status_line("").unwrap();
        assert!(out.contains(EXPECTED_COMMAND));
        assert!(out.contains("\"statusLine\""));
    }

    #[test]
    fn merge_preserves_existing_keys() {
        let out = merge_status_line(r#"{"apiKeyHelper":{"alwaysAllowedTools":[]}}"#).unwrap();
        let root: Value = serde_json::from_str(&out).unwrap();
        assert!(root.get("apiKeyHelper").is_some());
        assert!(root.get("statusLine").is_some());
        assert_eq!(root["statusLine"]["command"], "claude-hud render");
        assert_eq!(root["statusLine"]["refreshInterval"], 5);
    }

    #[test]
    fn merge_replaces_existing_status_line_without_duplication() {
        let out = merge_status_line(r#"{"statusLine":{"type":"command","command":"old-cmd","refreshInterval":1}}"#).unwrap();
        let root: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(root["statusLine"]["command"], "claude-hud render");
        assert_eq!(root.as_object().unwrap().get("statusLine").unwrap().as_object().unwrap().len(), 3);
    }

    #[test]
    fn merge_invalid_json_returns_err() {
        assert!(merge_status_line("{not json").is_err());
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
        assert!(merge_status_line("[1,2,3]").is_err());
        assert!(merge_status_line("\"foo\"").is_err());
    }

    #[test]
    fn merge_null_root_is_treated_as_empty_object() {
        let out = merge_status_line("null").unwrap();
        assert!(out.contains("\"statusLine\""));
    }

    #[test]
    fn remove_null_root_returns_empty_object() {
        let out = remove_status_line("null").unwrap();
        assert_eq!(out, "{}");
    }
}
