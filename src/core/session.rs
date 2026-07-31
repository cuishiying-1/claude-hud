use serde::Deserialize;

/// Full session data ingested from Claude Code status line stdin JSON.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionData {
    pub model: ModelInfo,
    pub context_window: ContextWindow,
    pub cost: CostInfo,
    #[serde(default)]
    pub rate_limits: RateLimits,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub subagent_status_line: Option<SubagentStatusLine>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ContextWindow {
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub used_percentage: f64,
    pub total_input_tokens: u64,
    #[serde(default)]
    pub total_output_tokens: u64,
    pub context_window_size: u64,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub current_usage: CurrentUsage,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct CurrentUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CostInfo {
    pub total_cost_usd: f64,
    pub total_duration_ms: u64,
    #[serde(default)]
    pub total_lines_added: u64,
    #[serde(default)]
    pub total_lines_removed: u64,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct RateLimits {
    #[serde(default)]
    pub five_hour: RateLimitBucket,
    #[serde(default)]
    pub seven_day: RateLimitBucket,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct RateLimitBucket {
    #[serde(default)]
    pub used_percentage: f64,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct SubagentStatusLine {
    #[serde(default)]
    pub agents: Vec<SubagentInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubagentInfo {
    pub name: String,
    pub model: String,
    #[serde(default)]
    pub task: String,
    #[serde(default)]
    pub elapsed_secs: u64,
    #[serde(default)]
    pub is_active: bool,
}

impl SessionData {
    /// Parse from the stdin JSON string provided by Claude Code.
    pub fn from_stdin_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// Treat explicit JSON `null` as the field's default value.
///
/// Claude Code may send `null` for usage fields (e.g. at session start);
/// `#[serde(default)]` alone only covers missing fields, not `null`.
fn deserialize_null_as_default<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'de> + Default,
    D: serde::Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}
