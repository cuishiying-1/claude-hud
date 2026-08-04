use serde::{Deserialize, Serialize};

/// Full session data ingested from Claude Code status line stdin JSON.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionData {
    pub model: ModelInfo,
    pub context_window: ContextWindow,
    pub cost: CostInfo,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub rate_limits: RateLimits,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default, alias = "subagentStatusLine")]
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

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Default, Clone)]
pub struct RateLimits {
    pub five_hour: RateLimitBucket,
    pub seven_day: RateLimitBucket,
}

/// 双形态解析：嵌套对象（Claude Code 现行 `five_hour`/`seven_day`）与
/// 扁平 `five_hour_pct`/`seven_day_pct`（state.json 段命名）都接受；
/// 空/混合对象容错为缺省。显式 null 由字段级 deserialize_null_as_default 兜底。
impl<'de> Deserialize<'de> for RateLimits {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let mut limits = RateLimits::default();
        if let Some(obj) = value.as_object() {
            for (key, val) in obj {
                match key.as_str() {
                    "five_hour" => {
                        if let Ok(bucket) = serde_json::from_value(val.clone()) {
                            limits.five_hour = bucket;
                        }
                    }
                    "seven_day" => {
                        if let Ok(bucket) = serde_json::from_value(val.clone()) {
                            limits.seven_day = bucket;
                        }
                    }
                    "five_hour_pct" => {
                        if let Some(pct) = val.as_f64() {
                            limits.five_hour.used_percentage = pct;
                        }
                    }
                    "seven_day_pct" => {
                        if let Some(pct) = val.as_f64() {
                            limits.seven_day.used_percentage = pct;
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(limits)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn json(rate_limits: &str, status: &str) -> String {
        format!(
            r#"{{"model":{{"id":"m","display_name":"M"}},
                "context_window":{{"used_percentage":1,"total_input_tokens":1,
                "context_window_size":100}},
                "cost":{{"total_cost_usd":0.1,"total_duration_ms":1}},
                "rate_limits":{rate_limits},
                {status}}}"#
        )
    }

    #[test]
    fn camel_case_alias_and_flat_rate_limits_parse() {
        let input = json(
            r#"{"five_hour_pct":12.5,"seven_day_pct":3.0}"#,
            r#""subagentStatusLine":{"agents":[{"name":"a","model":"m"}]}"#,
        );
        let data = SessionData::from_stdin_json(&input).unwrap();
        assert_eq!(data.rate_limits.five_hour.used_percentage, 12.5);
        assert_eq!(data.rate_limits.seven_day.used_percentage, 3.0);
        let agents = data.subagent_status_line.expect("camelCase alias parsed");
        assert_eq!(agents.agents.len(), 1);
        assert!(!agents.agents[0].is_active);
    }

    #[test]
    fn snake_case_nested_rate_limits_still_parse() {
        let input = json(
            r#"{"five_hour":{"used_percentage":42},"seven_day":{"used_percentage":7}}"#,
            r#""subagent_status_line":{"agents":[{"name":"b","model":"m"}]}"#,
        );
        let data = SessionData::from_stdin_json(&input).unwrap();
        assert_eq!(data.rate_limits.five_hour.used_percentage, 42.0);
        assert_eq!(data.rate_limits.seven_day.used_percentage, 7.0);
        assert!(data.subagent_status_line.is_some());
    }

    #[test]
    fn null_rate_limits_falls_back_to_default() {
        let input = json("null", r#""subagent_status_line":null"#);
        let data = SessionData::from_stdin_json(&input).unwrap();
        assert_eq!(data.rate_limits.five_hour.used_percentage, 0.0);
        assert!(data.subagent_status_line.is_none());
    }
}
