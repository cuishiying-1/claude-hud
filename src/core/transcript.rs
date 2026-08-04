use serde::{Deserialize, Serialize};
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
    /// 时间轴是否可靠：首条事件带有效 ISO8601 时间戳的会话才可靠；
    /// 不可靠会话所有下游走估算路径（≈ 标注）。
    pub timestamps_reliable: bool,
    /// 最新事件时间戳（可靠=真实 epoch；不可靠=行号估算）。
    pub last_event_secs: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentRecord {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub task_description: String,
    #[serde(default)]
    pub start_time_secs: u64,
    #[serde(default)]
    pub end_time_secs: Option<u64>,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub last_tool_call_secs: Option<u64>,
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    #[serde(default)]
    pub tool_calls: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCall {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub call_count: usize,
    #[serde(default)]
    pub last_call_secs: u64,
    #[serde(default)]
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCall {
    #[serde(default)]
    pub server: String,
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub call_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSnapshot {
    #[serde(default)]
    pub timestamp_secs: u64,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenTotal {
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub cache_created: u64,
    #[serde(default)]
    pub cache_read: u64,
}

/// Low-level transcript entry (one line of the JSONL file).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum TranscriptEntry {
    #[serde(rename = "tool_use")]
    ToolUse(ToolUseEntry),
    #[serde(rename = "assistant")]
    AssistantEntry(AssistantEntry),
    #[serde(rename = "compact_boundary")]
    CompactBoundary,
    #[serde(rename = "subagent_start")]
    SubagentStart(SubagentEntry),
    #[serde(rename = "subagent_stop")]
    SubagentStop {
        name: String,
        #[serde(default)]
        timestamp: Option<String>,
    },
    #[serde(other)]
    Unknown,
}

/// ISO8601 解析（RFC3339 带偏移；无偏移时按 UTC 的本地时间字面量）。
fn parse_iso_ts(ts: &str) -> Option<u64> {
    let s = ts.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp() as u64);
    }
    let naive = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok()?;
    Some(naive.and_utc().timestamp() as u64)
}

/// 统一提取事件时间戳（带 timestamp 的变体共享；缺失/解析失败 = None）。
fn entry_ts(entry: &TranscriptEntry) -> Option<u64> {
    let raw = match entry {
        TranscriptEntry::ToolUse(e) => e.timestamp.as_deref(),
        TranscriptEntry::SubagentStart(e) => e.timestamp.as_deref(),
        TranscriptEntry::SubagentStop { timestamp, .. } => timestamp.as_deref(),
        TranscriptEntry::AssistantEntry(e) => e.timestamp.as_deref(),
        _ => None,
    }?;
    parse_iso_ts(raw)
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
pub struct AssistantEntry {
    #[serde(default)]
    pub message: Option<MessageContent>,
    #[serde(default)]
    pub timestamp: Option<String>,
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
    #[serde(default)]
    pub timestamp: Option<String>,
}

/// Incremental transcript reader with cross-process cumulative state.
pub struct TranscriptReader {
    path: PathBuf,
    last_pos: u64,
    /// 最近激活的 subagent 名（工具调用归属指针，subagent_start 置位、
    /// subagent_stop 匹配清除；跨进程不持久化，恢复后为 None 的近似）
    active_recent: Option<String>,
    timestamps_reliable: bool,
    last_event_secs: Option<u64>,
    agents: HashMap<String, AgentRecord>,
    skills: HashMap<String, SkillCall>,
    mcps: HashMap<String, McpCall>,
    tool_counts: HashMap<String, usize>,
    total_tokens: TokenTotal,
    token_timeline: Vec<TokenSnapshot>,
}

/// 时间线分桶上限：360 桶 × 60s = 6h 滚动窗口（压缩预测只读首尾桶，足够）。
const MAX_TIMELINE_BUCKETS: usize = 360;

/// 裁剪时间线到最近 6h（push 后与 to_state 序列化前调用，恢复旧状态立即封顶）。
fn cap_timeline(timeline: &mut Vec<TokenSnapshot>) {
    let overflow = timeline.len().saturating_sub(MAX_TIMELINE_BUCKETS);
    if overflow > 0 {
        timeline.drain(0..overflow);
    }
}

impl TranscriptReader {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            last_pos: 0,
            active_recent: None,
            timestamps_reliable: false,
            last_event_secs: None,
            agents: HashMap::new(),
            skills: HashMap::new(),
            mcps: HashMap::new(),
            tool_counts: HashMap::new(),
            total_tokens: TokenTotal::default(),
            token_timeline: Vec::new(),
        }
    }

    /// Restore the reader from persisted state (new process continuing a
    /// session: offset + cumulative counts are carried over).
    pub fn from_state(seg: &TranscriptSegment) -> Self {
        let mut reader = Self::new(PathBuf::from(&seg.path));
        reader.last_pos = seg.last_pos;
        reader.active_recent = None;
        reader.timestamps_reliable = seg.timestamps_reliable;
        reader.last_event_secs = seg.last_event_secs;
        reader.tool_counts = seg.tool_counts.clone();
        reader.total_tokens = seg.total_tokens.clone();
        reader.token_timeline = seg.token_timeline.clone();
        for a in &seg.agents {
            reader.agents.insert(a.name.clone(), a.clone());
        }
        for s in &seg.skill_calls {
            reader.skills.insert(s.name.clone(), s.clone());
        }
        for m in &seg.mcp_calls {
            reader.mcps.insert(format!("{}::{}", m.server, m.tool), m.clone());
        }
        reader
    }

    /// Persist the reader's cumulative state for the next process.
    pub fn to_state(&self) -> TranscriptSegment {
        let mut agents: Vec<AgentRecord> = self.agents.values().cloned().collect();
        agents.sort_by(|a, b| a.name.cmp(&b.name));
        let mut token_timeline = self.token_timeline.clone();
        cap_timeline(&mut token_timeline);
        TranscriptSegment {
            path: self.path.to_string_lossy().into_owned(),
            last_pos: self.last_pos,
            agents,
            skill_calls: self.skills.values().cloned().collect(),
            mcp_calls: self.mcps.values().cloned().collect(),
            tool_counts: self.tool_counts.clone(),
            total_tokens: self.total_tokens.clone(),
            token_timeline,
            timestamps_reliable: self.timestamps_reliable,
            last_event_secs: self.last_event_secs,
        }
    }

    /// Build the cumulative summary seen by widgets (replace semantics:
    /// each widget stores its own copy of the full state).
    fn cumulative_summary(&self) -> TranscriptSummary {
        TranscriptSummary {
            agents: self.agents.values().cloned().collect(),
            tool_counts: self.tool_counts.clone(),
            skill_calls: self.skills.values().cloned().collect(),
            mcp_calls: self.mcps.values().cloned().collect(),
            token_timeline: self.token_timeline.clone(),
            total_tokens: self.total_tokens.clone(),
            timestamps_reliable: self.timestamps_reliable,
            last_event_secs: self.last_event_secs,
        }
    }

    /// Parse all new entries since last read. Returns the cumulative
    /// summary (totals, not increments).
    pub fn read_updates(&mut self) -> TranscriptSummary {
        let file = match fs::File::open(&self.path) {
            Ok(f) => f,
            Err(_) => return self.cumulative_summary(),
        };

        let file_len = match file.metadata() {
            Ok(m) => m.len(),
            Err(_) => return self.cumulative_summary(),
        };

        // 文件被截断（如会话重启重写）→ 丢弃累计状态并从 0 重读
        if self.last_pos > file_len {
            self.last_pos = 0;
            self.agents.clear();
            self.skills.clear();
            self.mcps.clear();
            self.tool_counts.clear();
            self.total_tokens = TokenTotal::default();
            self.token_timeline.clear();
            self.active_recent = None;
            self.timestamps_reliable = false;
            self.last_event_secs = None;
        }
        if file_len <= self.last_pos {
            return self.cumulative_summary(); // No new data
        }

        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(self.last_pos)).is_err() {
            return self.cumulative_summary();
        }

        // 会话起点（偏移 0 且无累计状态）判定时间轴可靠性：首条事件带
        // 有效 ISO8601 时间戳即可靠。从 state 恢复的会话沿用持久化标志。
        if self.last_pos == 0 && self.agents.is_empty() {
            self.timestamps_reliable = first_line_has_ts(&mut reader);
        }

        let mut current_secs = self.last_event_secs.unwrap_or(0);

        let mut line = String::new();
        while let Ok(bytes) = reader.read_line(&mut line) {
            if bytes == 0 {
                break;
            }

            if let Ok(entry) = serde_json::from_str::<TranscriptEntry>(line.trim()) {
                // 可靠会话：真实 ts 单调推进（不回退）；缺失行沿用最新
                // 已知 ts（连续缺失共享同一 ts）。不可靠会话：行号递增。
                if self.timestamps_reliable {
                    if let Some(real) = entry_ts(&entry) {
                        current_secs = current_secs.max(real);
                    }
                } else {
                    current_secs = current_secs.saturating_add(1);
                }
                self.last_event_secs = Some(current_secs);

                match entry {
                    TranscriptEntry::ToolUse(tool) => {
                        let name = tool.name.clone();
                        *self.tool_counts.entry(name.clone()).or_default() += 1;

                        // Detect MCP calls (mcp__server__tool format)
                        if name.starts_with("mcp__") {
                            let parts: Vec<&str> = name.splitn(3, "__").collect();
                            if parts.len() >= 2 {
                                let server = parts[1].to_string();
                                let tool_name = parts.get(2).map(|s| s.to_string()).unwrap_or_default();
                                let key = format!("{}::{}", server, tool_name);
                                let entry = self.mcps.entry(key).or_insert(McpCall {
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
                                let entry = self.skills.entry(skill_name.to_string()).or_insert(SkillCall {
                                    name: skill_name.to_string(),
                                    call_count: 0,
                                    last_call_secs: current_secs,
                                    is_active: true,
                                });
                                entry.call_count += 1;
                                entry.last_call_secs = current_secs;
                            }
                        }

                        // 工具调用归属最近激活的 subagent（近似：平铺
                        // JSONL 无 agent 关联，start/stop 切换指针）
                        if let Some(active) = self.active_recent.clone() {
                            if let Some(agent) = self.agents.get_mut(&active) {
                                agent.last_tool_call_secs = Some(current_secs);
                                agent.tool_calls += 1;
                            }
                        }
                    }
                    TranscriptEntry::SubagentStart(sub) => {
                        self.active_recent = Some(sub.name.clone());
                        self.agents.entry(sub.name.clone()).or_insert(AgentRecord {
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
                    TranscriptEntry::SubagentStop { name, .. } => {
                        if self.active_recent.as_deref() == Some(&name) {
                            self.active_recent = None;
                        }
                        if let Some(agent) = self.agents.get_mut(&name) {
                            agent.is_active = false;
                            agent.end_time_secs = Some(current_secs);
                        }
                    }
                    TranscriptEntry::AssistantEntry(assistant) => {
                        if let Some(msg) = assistant.message {
                            if let Some(usage) = msg.usage {
                                self.total_tokens.input += usage.input_tokens;
                                self.total_tokens.output += usage.output_tokens;
                                self.total_tokens.cache_created +=
                                    usage.cache_creation_input_tokens.unwrap_or(0);
                                self.total_tokens.cache_read +=
                                    usage.cache_read_input_tokens.unwrap_or(0);
                            }
                        }
                        // 60s epoch 对齐桶（跨进程稳定：进程 B 恢复后新行
                        // 落入既有桶即合并，新桶才 push）
                        let bucket = (current_secs / 60) * 60;
                        let snapshot = TokenSnapshot {
                            timestamp_secs: bucket,
                            input_tokens: self.total_tokens.input,
                            output_tokens: self.total_tokens.output,
                            total_tokens: self.total_tokens.input + self.total_tokens.output,
                        };
                        match self.token_timeline.last_mut() {
                            Some(last) if last.timestamp_secs == bucket => *last = snapshot,
                            _ => self.token_timeline.push(snapshot),
                        }
                        cap_timeline(&mut self.token_timeline);
                    }
                    _ => {}
                }
            }

            line.clear();
        }

        // Move position forward to the actually consumed offset
        self.last_pos = reader.stream_position().unwrap_or(file_len);

        self.cumulative_summary()
    }
}

/// 读当前偏移处的首条事件，判定是否带有效 ISO8601 时间戳；随后把
/// 读取位置回退到文件起点（会话起点判定用，偏移必为 0）。
fn first_line_has_ts(reader: &mut BufReader<fs::File>) -> bool {
    let mut first = String::new();
    if reader.read_line(&mut first).unwrap_or(0) == 0 {
        return false; // 空文件
    }
    let has_ts = serde_json::from_str::<TranscriptEntry>(first.trim())
        .ok()
        .and_then(|e| entry_ts(&e))
        .is_some();
    let _ = reader.seek(SeekFrom::Start(0));
    has_ts
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
        if !self.timestamps_reliable || self.token_timeline.len() < 2 {
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

/// Cross-process persisted transcript state (state.json `transcript`
/// segment). Replaces the placeholder from Task 1.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TranscriptSegment {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub last_pos: u64,
    #[serde(default)]
    pub agents: Vec<AgentRecord>,
    #[serde(default)]
    pub skill_calls: Vec<SkillCall>,
    #[serde(default)]
    pub mcp_calls: Vec<McpCall>,
    #[serde(default)]
    pub tool_counts: HashMap<String, usize>,
    #[serde(default)]
    pub total_tokens: TokenTotal,
    #[serde(default)]
    pub token_timeline: Vec<TokenSnapshot>,
    #[serde(default)]
    pub timestamps_reliable: bool,
    #[serde(default)]
    pub last_event_secs: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/transcript/agents.jsonl")
    }

    fn tmp_copy(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("hud-transcript-{}-{}", std::process::id(), name));
        fs::copy(fixture(), &p).unwrap();
        p
    }

    fn ts_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/transcript/timestamps.jsonl")
    }

    fn no_ts_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/transcript/no_ts.jsonl")
    }

    #[test]
    fn real_timestamps_drive_time_axis() {
        let mut reader = TranscriptReader::new(ts_fixture());
        let summary = reader.read_updates();
        assert!(summary.timestamps_reliable);
        assert_eq!(summary.last_event_secs, parse_iso_ts("2026-07-31T10:04:00Z"));
        let alpha = &summary.agents[0];
        assert_eq!(alpha.name, "alpha");
        assert_eq!(alpha.start_time_secs, parse_iso_ts("2026-07-31T10:01:00Z").unwrap());
        assert_eq!(alpha.last_tool_call_secs, Some(parse_iso_ts("2026-07-31T10:02:00Z").unwrap()));
        assert_eq!(alpha.end_time_secs, parse_iso_ts("2026-07-31T10:02:30Z"));
        assert!(!alpha.is_active);
        assert_eq!(alpha.tool_calls, 2);
    }

    #[test]
    fn missing_first_ts_marks_unreliable() {
        let mut reader = TranscriptReader::new(no_ts_fixture());
        let summary = reader.read_updates();
        assert!(!summary.timestamps_reliable);
        // 降级路径：start = 行号（1），end = 行号（3）
        assert_eq!(summary.agents[0].start_time_secs, 1);
        assert_eq!(summary.agents[0].end_time_secs, Some(3));
    }

    #[test]
    fn state_restore_keeps_reliability_flag() {
        let mut a = TranscriptReader::new(ts_fixture());
        let first = a.read_updates();
        assert!(first.timestamps_reliable);
        let seg = a.to_state();
        assert!(seg.timestamps_reliable);
        let mut b = TranscriptReader::from_state(&seg);
        let second = b.read_updates();
        assert!(second.timestamps_reliable);
        assert_eq!(second.last_event_secs, first.last_event_secs);

        let mut c = TranscriptReader::new(no_ts_fixture());
        assert!(!c.read_updates().timestamps_reliable);
        let seg2 = c.to_state();
        let mut d = TranscriptReader::from_state(&seg2);
        assert!(!d.read_updates().timestamps_reliable);
    }

    #[test]
    fn timeline_caps_at_360_buckets() {
        let mut timeline: Vec<TokenSnapshot> = (0..400u64)
            .map(|i| TokenSnapshot {
                timestamp_secs: i * 60,
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: i,
            })
            .collect();
        cap_timeline(&mut timeline);
        assert_eq!(timeline.len(), 360);
        assert_eq!(timeline[0].timestamp_secs, 40 * 60);
        assert_eq!(timeline[359].timestamp_secs, 399 * 60);
    }

    #[test]
    fn timeline_cap_keeps_prediction_window() {
        let mut reader = TranscriptReader::new(PathBuf::new());
        for i in 0..400u64 {
            reader.token_timeline.push(TokenSnapshot {
                timestamp_secs: i * 60,
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: i * 10,
            });
        }
        reader.timestamps_reliable = true;
        cap_timeline(&mut reader.token_timeline);
        let summary = reader.cumulative_summary();
        assert!(summary.compaction_prediction(50.0, 200_000).is_some());
    }

    #[test]
    fn epoch_buckets_merge_across_processes() {
        let mut p = std::env::temp_dir();
        p.push(format!("hud-transcript-{}-buckets.jsonl", std::process::id()));
        fs::copy(ts_fixture(), &p).unwrap();
        let mut a = TranscriptReader::new(p.clone());
        let first = a.read_updates();
        let seg = a.to_state();
        // 两桶：10:03:00 与 10:04:00（epoch 对齐）
        assert_eq!(first.token_timeline.len(), 2);
        let b0 = (parse_iso_ts("2026-07-31T10:03:00Z").unwrap() / 60) * 60;
        let b1 = (parse_iso_ts("2026-07-31T10:04:00Z").unwrap() / 60) * 60;
        assert_eq!(first.token_timeline[0].timestamp_secs, b0);
        assert_eq!(first.token_timeline[1].timestamp_secs, b1);

        // 进程 B 恢复后追加同分钟新行（10:04:30，晚于最新事件）→ 合并进
        // 既有 10:04:00 桶，不新 push（单调推进不会回退到旧分钟）
        let mut content = fs::read_to_string(&p).unwrap();
        content.push_str(
            "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":50,\"output_tokens\":10}},\"timestamp\":\"2026-07-31T10:04:30Z\"}\n",
        );
        fs::write(&p, content).unwrap();
        let mut b = TranscriptReader::from_state(&seg);
        let merged = b.read_updates();
        assert_eq!(merged.token_timeline.len(), 2);
        assert_eq!(merged.token_timeline[1].total_tokens, 300 + 130 + 60);
        fs::remove_file(&p).unwrap();
    }

    #[test]
    fn stalled_agents_requires_recent_tool_call() {
        let mut summary = TranscriptSummary::default();
        summary.agents.push(AgentRecord {
            name: "stalled".into(),
            is_active: true,
            last_tool_call_secs: Some(100),
            ..Default::default()
        });
        summary.agents.push(AgentRecord {
            name: "idle-not-active".into(),
            is_active: false,
            last_tool_call_secs: Some(100),
            ..Default::default()
        });
        summary.agents.push(AgentRecord {
            name: "no-call".into(),
            is_active: true,
            last_tool_call_secs: None,
            ..Default::default()
        });
        let stalled = summary.stalled_agents(30, 200);
        assert_eq!(stalled.len(), 1);
        assert_eq!(stalled[0].name, "stalled");
        assert!(summary.stalled_agents(30, 120).is_empty());
    }

    #[test]
    fn compaction_prediction_gated_on_reliability() {
        let mut summary = TranscriptSummary::default();
        summary.timestamps_reliable = true;
        summary.token_timeline.push(TokenSnapshot {
            timestamp_secs: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 1000,
        });
        summary.token_timeline.push(TokenSnapshot {
            timestamp_secs: 600,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 10000,
        });
        let minutes = summary.compaction_prediction(50.0, 200000);
        assert!(minutes.is_some());
        // 不可靠时间轴不显示伪精确
        summary.timestamps_reliable = false;
        assert!(summary.compaction_prediction(50.0, 200000).is_none());
        // 窗口参数真实生效
        summary.timestamps_reliable = true;
        let w200 = summary.compaction_prediction(50.0, 200000).unwrap();
        let w400 = summary.compaction_prediction(50.0, 400000).unwrap();
        assert!(w400 > w200);
    }

    #[test]
    fn read_updates_returns_cumulative_summary() {
        let mut reader = TranscriptReader::new(fixture());
        let first = reader.read_updates();
        let second = reader.read_updates(); // 无新行
        assert_eq!(first.tool_counts, second.tool_counts);
        assert_eq!(first.total_tokens.input, second.total_tokens.input);
        assert!(!first.tool_counts.is_empty());
    }

    #[test]
    fn cross_process_accumulation_via_state() {
        let path = fixture();
        let mut a = TranscriptReader::new(path.clone());
        let first = a.read_updates();
        let total_before = first.total_tokens.input;
        let seg = a.to_state();
        assert_eq!(seg.path, path.to_string_lossy());
        assert!(seg.last_pos > 0);

        // 进程 B 从 state 恢复，无新数据 → 累计结果完全一致（不重复计数）
        let mut b = TranscriptReader::from_state(&seg);
        let second = b.read_updates();
        assert_eq!(second.total_tokens.input, total_before);
        assert_eq!(second.agents.len(), first.agents.len());
        assert_eq!(b.to_state().last_pos, seg.last_pos);
    }

    #[test]
    fn truncated_file_resets_cumulative_state() {
        let p = tmp_copy("trunc.jsonl");
        let mut reader = TranscriptReader::new(p.clone());
        let before = reader.read_updates();
        assert!(before.total_tokens.input > 0);
        assert!(reader.to_state().last_pos > 100);

        // 文件被截断（会话重启）→ 重置并从 0 重读
        let data = fs::read(&p).unwrap();
        fs::write(&p, &data[..100]).unwrap();
        let after = reader.read_updates();
        assert!(after.total_tokens.input <= before.total_tokens.input);
        assert_eq!(reader.to_state().last_pos, 100);
        fs::remove_file(&p).unwrap();
    }
}
