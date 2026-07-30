use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;

/// Parsed summary of a Claude Code transcript session.
#[derive(Debug, Clone, Default)]
pub struct TranscriptSummary {
    /// Per-agent statistics
    pub agents: Vec<AgentRecord>,
    /// Tool call counts by tool name
    pub tool_counts: HashMap<String, usize>,
    /// Skill invocations detected in this session
    pub skill_calls: Vec<SkillCall>,
    /// MCP tool invocations
    pub mcp_calls: Vec<McpCall>,
    /// Token consumption timeline (per-minute snapshots)
    pub token_timeline: Vec<TokenSnapshot>,
    /// Total tokens consumed
    pub total_tokens: TokenTotal,
}

#[derive(Debug, Clone)]
pub struct AgentRecord {
    pub name: String,
    pub model: String,
    pub task_description: String,
    pub start_time_secs: u64,
    pub end_time_secs: Option<u64>,
    pub is_active: bool,
    pub last_tool_call_secs: Option<u64>,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub tool_calls: usize,
}

#[derive(Debug, Clone)]
pub struct SkillCall {
    pub name: String,
    pub call_count: usize,
    pub last_call_secs: u64,
    pub is_active: bool,
}

#[derive(Debug, Clone)]
pub struct McpCall {
    pub server: String,
    pub tool: String,
    pub call_count: usize,
}

#[derive(Debug, Clone)]
pub struct TokenSnapshot {
    pub timestamp_secs: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TokenTotal {
    pub input: u64,
    pub output: u64,
    pub cache_created: u64,
    pub cache_read: u64,
}

/// Low-level transcript entry (one line of the JSONL file).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum TranscriptEntry {
    #[serde(rename = "tool_use")]
    ToolUse(ToolUseEntry),
    #[serde(rename = "tool_result")]
    ToolResult(ToolResultEntry),
    #[serde(rename = "user")]
    UserEntry(UserEntry),
    #[serde(rename = "assistant")]
    AssistantEntry(AssistantEntry),
    #[serde(rename = "compact_boundary")]
    CompactBoundary,
    #[serde(rename = "subagent_start")]
    SubagentStart(SubagentEntry),
    #[serde(rename = "subagent_stop")]
    SubagentStop { name: String },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolUseEntry {
    pub name: String,
    #[serde(default)]
    pub input: serde_json::Value,
    #[serde(default)]
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolResultEntry {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserEntry {
    #[serde(default)]
    pub message: Option<MessageContent>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssistantEntry {
    #[serde(default)]
    pub message: Option<MessageContent>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageContent {
    #[serde(default)]
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UsageInfo {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubagentEntry {
    pub name: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub task: String,
}

/// Incremental transcript reader: only reads new lines since last position.
pub struct TranscriptReader {
    path: PathBuf,
    last_pos: u64,
    base_time_secs: Option<u64>,
}

impl TranscriptReader {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            last_pos: 0,
            base_time_secs: None,
        }
    }

    /// Parse all new entries since last read. Returns the parsed summary.
    pub fn read_updates(&mut self) -> TranscriptSummary {
        let mut summary = TranscriptSummary::default();

        let file = match fs::File::open(&self.path) {
            Ok(f) => f,
            Err(_) => return summary,
        };

        let metadata = match file.metadata() {
            Ok(m) => m,
            Err(_) => return summary,
        };

        let file_len = metadata.len();
        if file_len <= self.last_pos {
            return summary; // No new data
        }

        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(self.last_pos)).is_err() {
            return summary;
        }

        // Set base time from first entry timestamp if not set
        if self.base_time_secs.is_none() {
            self.base_time_secs = Some(0); // epoch-relative
        }

        let mut line = String::new();
        let mut agent_map: HashMap<String, AgentRecord> = HashMap::new();
        let mut skill_map: HashMap<String, SkillCall> = HashMap::new();
        let mut mcp_map: HashMap<String, McpCall> = HashMap::new();
        let mut total_tokens = TokenTotal::default();
        let mut current_secs: u64 = 0;

        while let Ok(bytes) = reader.read_line(&mut line) {
            if bytes == 0 {
                break;
            }

            // Rough timestamp increment (50ms per entry as fallback)
            current_secs = current_secs.saturating_add(1);

            if let Ok(entry) = serde_json::from_str::<TranscriptEntry>(line.trim()) {
                match entry {
                    TranscriptEntry::ToolUse(tool) => {
                        let name = tool.name.clone();
                        *summary.tool_counts.entry(name.clone()).or_default() += 1;

                        // Detect MCP calls (mcp__server__tool format)
                        if name.starts_with("mcp__") {
                            let parts: Vec<&str> = name.splitn(3, "__").collect();
                            if parts.len() >= 2 {
                                let server = parts[1].to_string();
                                let tool_name = parts.get(2).map(|s| s.to_string()).unwrap_or_default();
                                let key = format!("{}::{}", server, tool_name);
                                let entry = mcp_map.entry(key).or_insert(McpCall {
                                    server,
                                    tool: tool_name,
                                    call_count: 0,
                                });
                                entry.call_count += 1;
                            }
                        }

                        // Detect Skill calls
                        if name == "Skill" {
                            if let Some(skill_name) = tool
                                .input
                                .get("skill")
                                .and_then(|v| v.as_str())
                            {
                                let entry = skill_map.entry(skill_name.to_string()).or_insert(SkillCall {
                                    name: skill_name.to_string(),
                                    call_count: 0,
                                    last_call_secs: current_secs,
                                    is_active: true,
                                });
                                entry.call_count += 1;
                                entry.last_call_secs = current_secs;
                            }
                        }

                        // Update agent last-tool-call timestamp
                        // (We detect agents from subagent_start events, track their tool activity here)
                    }
                    TranscriptEntry::SubagentStart(sub) => {
                        agent_map.entry(sub.name.clone()).or_insert(AgentRecord {
                            name: sub.name.clone(),
                            model: sub.model,
                            task_description: sub.task,
                            start_time_secs: current_secs,
                            end_time_secs: None,
                            is_active: true,
                            last_tool_call_secs: None,
                            tokens_in: 0,
                            tokens_out: 0,
                            tool_calls: 0,
                        });
                    }
                    TranscriptEntry::SubagentStop { name } => {
                        if let Some(agent) = agent_map.get_mut(&name) {
                            agent.is_active = false;
                            agent.end_time_secs = Some(current_secs);
                        }
                    }
                    TranscriptEntry::AssistantEntry(assistant) => {
                        if let Some(msg) = assistant.message {
                            if let Some(usage) = msg.usage {
                                total_tokens.input += usage.input_tokens;
                                total_tokens.output += usage.output_tokens;
                                total_tokens.cache_created +=
                                    usage.cache_creation_input_tokens.unwrap_or(0);
                                total_tokens.cache_read +=
                                    usage.cache_read_input_tokens.unwrap_or(0);
                            }
                        }
                        // Snapshot per ~60s bucket
                        if summary.token_timeline.is_empty()
                            || current_secs - summary.token_timeline.last().unwrap().timestamp_secs >= 60
                        {
                            summary.token_timeline.push(TokenSnapshot {
                                timestamp_secs: current_secs,
                                input_tokens: total_tokens.input,
                                output_tokens: total_tokens.output,
                                total_tokens: total_tokens.input + total_tokens.output,
                            });
                        }
                    }
                    _ => {}
                }
            }

            line.clear();
        }

        // Move position forward
        self.last_pos = file_len;

        // Collect agents
        summary.agents = agent_map.into_values().collect();

        // Collect skill calls
        summary.skill_calls = skill_map.into_values().collect();

        // Collect MCP calls
        summary.mcp_calls = mcp_map.into_values().collect();

        summary.total_tokens = total_tokens;

        summary
    }
}

impl TranscriptSummary {
    /// Compute per-agent token attribution from the summary.
    /// Phase 2 uses a heuristic: tool call counts as a proxy.
    pub fn token_attribution(&self) -> Vec<(&AgentRecord, f64)> {
        let total_tools: usize = self.agents.iter().map(|a| a.tool_calls).sum();
        if total_tools == 0 {
            return self.agents.iter().map(|a| (a, 0.0)).collect();
        }
        self.agents
            .iter()
            .map(|a| {
                let pct = (a.tool_calls as f64 / total_tools as f64) * 100.0;
                (a, pct)
            })
            .collect()
    }

    /// Detect stalled agents: no tool call in >30s but still marked active.
    pub fn stalled_agents(&self, threshold_secs: u64, current_time_secs: u64) -> Vec<&AgentRecord> {
        self.agents
            .iter()
            .filter(|a| {
                a.is_active
                    && a.last_tool_call_secs
                        .map(|t| current_time_secs.saturating_sub(t) > threshold_secs)
                        .unwrap_or(false)
            })
            .collect()
    }

    /// Compute compaction prediction from token consumption rate.
    pub fn compaction_prediction(&self, used_pct: f64, window_size: u64) -> Option<u64> {
        if self.token_timeline.len() < 2 {
            return None;
        }
        let first = &self.token_timeline[0];
        let last = &self.token_timeline[self.token_timeline.len() - 1];
        let elapsed = last.timestamp_secs.saturating_sub(first.timestamp_secs);
        if elapsed == 0 || last.total_tokens <= first.total_tokens {
            return None;
        }
        let rate = (last.total_tokens - first.total_tokens) as f64 / elapsed as f64;
        let remaining = (window_size as f64 * (1.0 - used_pct / 100.0)) as f64;
        let seconds = (remaining / rate) as u64;
        Some(seconds / 60) // return minutes
    }
}
