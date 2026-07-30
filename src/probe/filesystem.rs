use std::fs;
use std::path::PathBuf;

/// Count skills found in ~/.claude/skills/ and project .claude/skills/
pub fn count_skills() -> usize {
    let mut count = 0;
    if let Some(home) = dirs::home_dir() {
        count += count_dirs(&home.join(".claude").join("skills"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        count += count_dirs(&cwd.join(".claude").join("skills"));
    }
    count
}

/// Count MCP servers configured in settings.json (best-effort).
pub fn count_mcp_servers() -> usize {
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".claude").join("settings.json");
        if let Ok(content) = fs::read_to_string(&path) {
            // Simple heuristic: count "mcpServers" entries
            return content.matches("mcpServers").count();
        }
    }
    0
}

fn count_dirs(path: &PathBuf) -> usize {
    if !path.exists() {
        return 0;
    }
    fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .count()
        })
        .unwrap_or(0)
}
