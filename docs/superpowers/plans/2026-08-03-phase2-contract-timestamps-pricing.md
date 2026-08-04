# Phase 2 实施计划：字段契约 + 真实时间戳 + 成本正确性

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完成第二期三个任务——③ 字段契约（双命名 stdin + `--dump` + doctor 探针）、④ 真实时间戳（ISO8601 时间轴 + 降级 + epoch 分桶）、⑭ 成本正确性（`currency_symbol` 统一 + `[pricing]` 重算）——每任务一个 commit。

**Architecture:** 三个任务不改 state.json 5 段共享层，各自强化输入契约与输出真实性：③ 在 session.rs 做输入层兼容（alias + untagged 双形态），④ 在 transcript.rs 把 timestamp 从"声明未用"变成主时间轴（`timestamps_reliable` 贯穿展示层），⑭ 新增 `core/pricing.rs` 纯函数三态成本流，经 WidgetConfig 注入两条渲染管线（widget 签名零改动）。

**Tech Stack:** Rust 2021 / serde(untagged, alias, deserialize_with) / chrono 0.4.45（`parse_from_rfc3339` + `and_utc`，无默认 feature）/ clap / toml / Python 黑盒套件（scripts/test_hud.py）。

**执行约定（用户既定，高于技能默认）：**
- INLINE 执行：我在会话内直接改代码、跑测试，每任务完成后请用户过目，用户回复"继续"进入下一任务。
- **禁止自动 git add/commit/push**：每个任务末尾只给出提交命令清单，由用户手动执行。
- cargo 不在 PATH：`export PATH="$HOME/.cargo/bin:$PATH"`。
- 禁止 `cargo fmt`（本仓库有意不遵循 rustfmt）。
- 黑盒套件：`python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`（相对路径在子进程会失败）。

---

## Task 1: ③ 字段契约（session.rs + `--dump` + doctor 探针 + 夹具 + P2-01..03）

**Files:**
- Modify: `src/core/session.rs`（alias + RateLimits 自定义反序列化 + 单测）
- Modify: `src/main.rs:26-53,107-157`（`Render { dump }` 变体 + 分发）
- Modify: `src/compact.rs`（新增 `dump_stdin`）
- Modify: `src/doctor.rs`（契约探针信息项）
- Create: `fixtures/json/camel_contract.json`
- Modify: `scripts/hudlib/cases.py`（P2-01/02/03 + CASES 计数）

### Step 1: session.rs — `subagentStatusLine` alias + RateLimits 双形态

`src/core/session.rs`：

（a）`SessionData` 两个字段改注解：

```rust
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub rate_limits: RateLimits,
    #[serde(default, alias = "subagentStatusLine")]
    pub subagent_status_line: Option<SubagentStatusLine>,
```

（b）`RateLimits` 由派生 Deserialize 改为自定义 impl（untagged 双形态，结构体形状不变 → state.rs 零改动）：

```rust
#[derive(Debug, Default, Clone)]
pub struct RateLimits {
    pub five_hour: RateLimitBucket,
    pub seven_day: RateLimitBucket,
}

/// 双形态解析：嵌套对象（Claude Code 现行 `five_hour`/`seven_day`）
/// 与扁平 `five_hour_pct`/`seven_day_pct`（state.json 段命名）都接受。
impl<'de> Deserialize<'de> for RateLimits {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RateLimitsIn {
            Nested {
                #[serde(default)]
                five_hour: RateLimitBucket,
                #[serde(default)]
                seven_day: RateLimitBucket,
            },
            Flat {
                #[serde(default)]
                five_hour_pct: f64,
                #[serde(default)]
                seven_day_pct: f64,
            },
        }
        match RateLimitsIn::deserialize(deserializer)? {
            RateLimitsIn::Nested { five_hour, seven_day } => {
                Ok(RateLimits { five_hour, seven_day })
            }
            RateLimitsIn::Flat { five_hour_pct, seven_day_pct } => Ok(RateLimits {
                five_hour: RateLimitBucket {
                    used_percentage: five_hour_pct,
                },
                seven_day: RateLimitBucket {
                    used_percentage: seven_day_pct,
                },
            }),
        }
    }
}
```

`RateLimitBucket` 与 `SubagentStatusLine` 等其余代码不动。

### Step 2: session.rs — 新增测试模块（文件末尾）

```rust
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
        assert_eq!(agents.agents[0].name, "a");
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
```

### Step 3: main.rs — `Render { dump }` 变体

`src/main.rs`：

（a）子命令（替换原 `Render,` 行）：

```rust
    /// Compact mode: read stdin JSON, output ANSI status line
    Render {
        /// Debug: print stdin JSON with recognized/unknown top-level key classification
        #[arg(long)]
        dump: bool,
    },
```

（b）分发（替换原 `Commands::Render => ...` 分支）：

```rust
        Commands::Render { dump } => {
            if dump {
                match compact::dump_stdin() {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        let state_path = AppConfig::state_path().unwrap_or_default();
                        StateFile::write_last_error(&state_path, &e);
                        println!("{}", compact::hud_err_marker(&e));
                        Err(e)
                    }
                }
            } else {
                match compact::render(&registry, &config, &theme) {
                    Ok(output) => {
                        print!("{}", output);
                        Ok(())
                    }
                    Err(e) => {
                        let state_path = AppConfig::state_path().unwrap_or_default();
                        StateFile::write_last_error(&state_path, &e);
                        println!("{}", compact::hud_err_marker(&e));
                        Err(e)
                    }
                }
            }
        }
```

### Step 4: compact.rs — `dump_stdin`

追加到 `read_stdin` 之后：

```rust
/// 调试输出：原始 stdin JSON + 顶层键分类（recognized = SessionData
/// 已知字段含 camelCase alias / unknown = 其余）。解析失败走 render
/// 的错误路径（[hud err] + last_error，行为与 render 一致）。
pub fn dump_stdin() -> Result<(), String> {
    let stdin_data = read_stdin()?;
    let value: serde_json::Value = serde_json::from_str(&stdin_data)
        .map_err(|e| format!("parse stdin JSON: {}", e))?;
    let _ = SessionData::from_stdin_json(&stdin_data)
        .map_err(|e| format!("parse stdin JSON: {}", e))?;

    let recognized = [
        "model", "context_window", "cost", "rate_limits",
        "transcript_path", "subagent_status_line", "subagentStatusLine",
    ];
    let mut unknown: Vec<String> = Vec::new();
    if let Some(obj) = value.as_object() {
        for key in obj.keys() {
            if !recognized.contains(&key.as_str()) {
                unknown.push(key.clone());
            }
        }
    }
    unknown.sort();
    println!("recognized: {}", recognized.join(", "));
    println!(
        "unknown: {}",
        if unknown.is_empty() {
            "(none)".to_string()
        } else {
            unknown.join(", ")
        }
    );
    println!("--- raw stdin ---");
    println!(
        "{}",
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| stdin_data)
    );
    Ok(())
}
```

### Step 5: doctor.rs — 契约探针（信息项，失败不计 failure）

（a）新增函数（放在 `check` 函数之前）：

```rust
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
```

（b）在 `run` 中 sample render 检查之后、`if failures == 0` 之前插入一行调用：

```rust
    contract_probe();
```

### Step 6: 构建 + 单测

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test`
Expected: 通过（新增 3 个 session 测试；既有 38 个测试不受影响——RateLimits 形状未变）。

### Step 7: 夹具 + 黑盒用例

（a）创建 `fixtures/json/camel_contract.json`：

```json
{
  "model": {"id": "deepseek-v4-flash", "display_name": "deepseek-v4-flash"},
  "context_window": {"context_window_size": 200000, "used_percentage": 30,
                     "total_input_tokens": 6800, "total_output_tokens": 5000,
                     "current_usage": {"input_tokens": 6800, "output_tokens": 5000,
                                       "cache_creation_input_tokens": 0,
                                       "cache_read_input_tokens": 100}},
  "cost": {"total_cost_usd": 0.034, "total_duration_ms": 12000},
  "rate_limits": {"five_hour_pct": 12, "seven_day_pct": 0},
  "subagentStatusLine": {"agents": [{"name": "probe", "model": "deepseek-v4-flash",
                                     "task": "search", "elapsed_secs": 5,
                                     "is_active": true}]}
}
```

（b）`scripts/hudlib/cases.py`：在 `P1` 列表之后、`CASES = ...` 之前追加：

```python
# ---------------------------------------------------------------------------
# P2: 第二期（任务③④⑭）
# ---------------------------------------------------------------------------
P2 = [
    render_case("P2-01", "camelCase 双命名 + 扁平 rate_limits", "P2",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash", "1/1 agents"],
                 "stderr_empty": True},
                stdin_file="json/camel_contract.json",
                config=DEFAULT_CONFIG,
                note="任务③：subagentStatusLine alias + five_hour_pct 扁平形态解析"),
    render_case("P2-02", "render --dump 键分类", "P2",
                {"exit": 0, "stdout_contains": ["recognized", "unknown",
                                                "session_id", "exceeds_200k_tokens"]},
                args=["render", "--dump"], stdin_file="json/full.json",
                note="任务③：full.json 的 cwd/workspace/version/exceeds_200k_tokens/session_id 归入 unknown"),
    render_case("P2-03", "doctor 契约探针", "P2",
                {"exit": 0, "stdout_contains": ["contract probe"]},
                args=["doctor"], config=DEFAULT_CONFIG,
                note="任务③：探针为信息项，双命名样例解析失败不红"),
]
```

并把计数行改为：

```python
CASES = D1 + D2 + D3 + D4 + D5 + D6 + D7 + D8 + P1 + P2
assert len(CASES) == 99, f"expected 99 cases, got {len(CASES)}"
```

### Step 8: 构建 + 全量黑盒

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build && python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
Expected: 99/99 通过（原 96 个不回归 + P2-01..03）。D3-04 等既有用例不应受影响（RateLimits 形状未变）。

### Step 9: Commit（用户执行）

```bash
git add src/core/session.rs src/main.rs src/compact.rs src/doctor.rs \
        fixtures/json/camel_contract.json scripts/hudlib/cases.py
git commit -m "fix: dual-naming contract — subagentStatusLine alias + flat rate_limits + render --dump + doctor contract probe"
```

---

## Task 2: ④ 真实时间戳（transcript.rs 时间轴 + 下游接线 + P2-04/10）

**Files:**
- Modify: `src/core/transcript.rs`（时间轴核心）
- Create: `fixtures/transcript/timestamps.jsonl`、`fixtures/transcript/no_ts.jsonl`
- Modify: `src/widgets/agent_detail.rs`（elapsed 真实化 + `≈` 降级 + is_stalled 修复）
- Modify: `src/widgets/alerts.rs`（stalled 真实触发 + 真实窗口 + 可靠门）
- Modify: `scripts/hudlib/assertions.py`（`_dig` 支持列表索引）
- Modify: `scripts/hudlib/cases.py`（P2-04/10 + CASES 计数）

### Step 1: transcript.rs — 时间戳解析函数与 entry 字段

（a）入口 struct 补 timestamp 字段（`ToolUseEntry` 已有）：

```rust
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
```

枚举变体改：

```rust
    #[serde(rename = "subagent_stop")]
    SubagentStop {
        name: String,
        #[serde(default)]
        timestamp: Option<String>,
    },
```

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct AssistantEntry {
    #[serde(default)]
    pub message: Option<MessageContent>,
    #[serde(default)]
    pub timestamp: Option<String>,
}
```

（b）解析函数（放在 `TranscriptEntry` 定义之后）：

```rust
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
```

（c）`AgentRecord` 加 `#[derive(Default)]`（测试构造便利）：

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentRecord {
```

### Step 2: transcript.rs — TranscriptSummary / TranscriptSegment / Reader 字段

（a）`TranscriptSummary` 追加两个字段（`#[derive(Debug, Clone, Default)]` 不变）：

```rust
    /// 时间轴是否可靠：首条事件带有效 ISO8601 时间戳的会话才可靠；
    /// 不可靠会话所有下游走估算路径（≈ 标注）。
    pub timestamps_reliable: bool,
    /// 最新事件时间戳（可靠=真实 epoch；不可靠=行号估算）。
    pub last_event_secs: Option<u64>,
```

（b）`TranscriptReader` 结构体：删 `base_time_secs`，加三个字段：

```rust
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
```

`new()` 初始化补：

```rust
            active_recent: None,
            timestamps_reliable: false,
            last_event_secs: None,
```

`from_state`：删除 `reader.base_time_secs = Some(0);`，补：

```rust
        reader.active_recent = None;
        reader.timestamps_reliable = seg.timestamps_reliable;
        reader.last_event_secs = seg.last_event_secs;
```

`to_state`：agents 排序后收集（确定性持久化，P2-04 可精确断言 `agents.0`）：

```rust
    pub fn to_state(&self) -> TranscriptSegment {
        let mut agents: Vec<AgentRecord> = self.agents.values().cloned().collect();
        agents.sort_by(|a, b| a.name.cmp(&b.name));
        TranscriptSegment {
            path: self.path.to_string_lossy().into_owned(),
            last_pos: self.last_pos,
            agents,
            skill_calls: self.skills.values().cloned().collect(),
            mcp_calls: self.mcps.values().cloned().collect(),
            tool_counts: self.tool_counts.clone(),
            total_tokens: self.total_tokens.clone(),
            token_timeline: self.token_timeline.clone(),
            timestamps_reliable: self.timestamps_reliable,
            last_event_secs: self.last_event_secs,
        }
    }
```

`cumulative_summary` 补：

```rust
            timestamps_reliable: self.timestamps_reliable,
            last_event_secs: self.last_event_secs,
```

`TranscriptSegment` 追加两字段：

```rust
    #[serde(default)]
    pub timestamps_reliable: bool,
    #[serde(default)]
    pub last_event_secs: Option<u64>,
```

### Step 3: transcript.rs — read_updates 时间轴重写

整个 `read_updates` 方法体替换（`while let Ok(bytes) ...` 段之前的部分保留到 seek）：

```rust
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
```

新增辅助函数（放在 `impl TranscriptReader` 之外）：

```rust
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
```

（删除原 `// Set base time from first entry timestamp if not set ...` 块与 `base_time_secs` 相关代码；`current_secs` 初始化由 `token_timeline.last()` 改为 `last_event_secs`。）

### Step 4: transcript.rs — compaction_prediction 可靠门 + 真实窗口

`TranscriptSummary::compaction_prediction` 开头加门：

```rust
    pub fn compaction_prediction(&self, used_pct: f64, window_size: u64) -> Option<u64> {
        if !self.timestamps_reliable || self.token_timeline.len() < 2 {
            return None;
        }
```

（`window_size` 参数已存在，调用方接线在 Step 5。）

### Step 5: 夹具

（a）创建 `fixtures/transcript/timestamps.jsonl`（全行固定 ISO8601，可精确断言；assistant 的 usage 供 ⑭ 重算）：

```
{"type":"subagent_start","name":"alpha","model":"deepseek-v4-flash","task":"search","timestamp":"2026-07-31T10:01:00Z"}
{"type":"tool_use","name":"Bash","input":{},"timestamp":"2026-07-31T10:01:30Z"}
{"type":"tool_use","name":"Read","input":{},"timestamp":"2026-07-31T10:02:00Z"}
{"type":"subagent_stop","name":"alpha","timestamp":"2026-07-31T10:02:30Z"}
{"type":"assistant","message":{"usage":{"input_tokens":100,"output_tokens":50}},"timestamp":"2026-07-31T10:03:00Z"}
{"type":"assistant","message":{"usage":{"input_tokens":200,"output_tokens":80}},"timestamp":"2026-07-31T10:04:00Z"}
```

（b）创建 `fixtures/transcript/no_ts.jsonl`（无 timestamp，降级路径）：

```
{"type":"subagent_start","name":"alpha","model":"deepseek-v4-flash","task":"search"}
{"type":"tool_use","name":"Bash","input":{}}
{"type":"subagent_stop","name":"alpha"}
```

### Step 6: transcript.rs — 单元测试

在既有 `mod tests` 内追加：

```rust
    fn tmp_copy(name: &str) -> PathBuf { ... } // 已有，新增两个 fixture 路径

    #[test]
    fn real_timestamps_drive_time_axis() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/transcript/timestamps.jsonl");
        let mut reader = TranscriptReader::new(path);
        let summary = reader.read_updates();
        assert!(summary.timestamps_reliable);
        assert_eq!(summary.last_event_secs, parse_iso_ts("2026-07-31T10:04:00Z"));
        let alpha = &summary.agents[0];
        assert_eq!(alpha.name, "alpha");
        assert_eq!(alpha.start_time_secs, parse_iso_ts("2026-07-31T10:01:00Z").unwrap());
        assert_eq!(alpha.last_tool_call_secs, parse_iso_ts("2026-07-31T10:02:00Z").unwrap());
        assert_eq!(alpha.end_time_secs, parse_iso_ts("2026-07-31T10:02:30Z"));
        assert!(!alpha.is_active);
        assert_eq!(alpha.tool_calls, 2);
    }

    #[test]
    fn missing_first_ts_marks_unreliable() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/transcript/no_ts.jsonl");
        let mut reader = TranscriptReader::new(path);
        let summary = reader.read_updates();
        assert!(!summary.timestamps_reliable);
        // 降级路径：start = 行号（1），end = 行号（3）
        assert_eq!(summary.agents[0].start_time_secs, 1);
        assert_eq!(summary.agents[0].end_time_secs, Some(3));
    }

    #[test]
    fn state_restore_keeps_reliability_flag() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/transcript/timestamps.jsonl");
        let mut a = TranscriptReader::new(path.clone());
        let first = a.read_updates();
        assert!(first.timestamps_reliable);
        let seg = a.to_state();
        assert!(seg.timestamps_reliable);
        let mut b = TranscriptReader::from_state(&seg);
        let second = b.read_updates();
        assert!(second.timestamps_reliable);
        assert_eq!(second.last_event_secs, first.last_event_secs);

        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/transcript/no_ts.jsonl");
        let mut c = TranscriptReader::new(p);
        assert!(!c.read_updates().timestamps_reliable);
        let seg2 = c.to_state();
        let mut d = TranscriptReader::from_state(&seg2);
        assert!(!d.read_updates().timestamps_reliable);
    }

    #[test]
    fn epoch_buckets_merge_across_processes() {
        let p = tmp_copy("buckets.jsonl");
        let mut a = TranscriptReader::new(p.clone());
        let first = a.read_updates();
        let seg = a.to_state();
        // 两桶：10:03:00 与 10:04:00（epoch 对齐）
        assert_eq!(first.token_timeline.len(), 2);
        let b0 = (parse_iso_ts("2026-07-31T10:03:00Z").unwrap() / 60) * 60;
        let b1 = (parse_iso_ts("2026-07-31T10:04:00Z").unwrap() / 60) * 60;
        assert_eq!(first.token_timeline[0].timestamp_secs, b0);
        assert_eq!(first.token_timeline[1].timestamp_secs, b1);

        // 进程 B 恢复后追加同一桶内的新行 → 合并进既有桶，不新 push
        let mut content = fs::read_to_string(&p).unwrap();
        content.push_str(
            "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":50,\"output_tokens\":10}},\"timestamp\":\"2026-07-31T10:03:30Z\"}\n",
        );
        fs::write(&p, content).unwrap();
        let mut b = TranscriptReader::from_state(&seg);
        let merged = b.read_updates();
        assert_eq!(merged.token_timeline.len(), 2);
        assert_eq!(merged.token_timeline[0].total_tokens, 300 + 130 + 60);
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
```

注意：`tmp_copy` 现有实现从 `fixtures/transcript/agents.jsonl` 复制；新增测试内联写文件的场景用 `fs::write`（epoch_buckets 测试直接复制 timestamps.jsonl 内容的写法不可用，需改为：先 `fs::copy` timestamps.jsonl 到 tmp 再追加。具体写法：把 `tmp_copy` 泛化为 `tmp_copy_from(src_name, name)`，或在该测试内直接用 `fs::copy(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/transcript/timestamps.jsonl"), &p)` 后追加——以实际实现为准，断言不变）。

### Step 7: agent_detail.rs — elapsed 真实化 + `≈` 降级 + is_stalled 修复

（a）新增辅助（放在 `impl AgentDetail` 之前）：

```rust
/// 卡顿判定：真实触发需 now − last_tool_call > threshold；不可靠
/// 时间轴不猜测（返回 false，避免行号代秒触发假告警）。
fn is_stalled(
    agent: &crate::core::transcript::AgentRecord,
    summary: &TranscriptSummary,
    now_secs: u64,
    stall_secs: u64,
) -> bool {
    summary.timestamps_reliable
        && agent
            .last_tool_call_secs
            .map(|t| now_secs.saturating_sub(t) > stall_secs)
            .unwrap_or(false)
}

fn format_dur(secs: u64) -> String {
    if secs >= 60 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}s", secs)
    }
}
```

（b）`render_compact` 的循环体改为：

```rust
                for agent in &summary.agents {
                    if !agent.is_active {
                        continue;
                    }
                    let now = crate::core::state::now_secs();
                    let is_stalled = is_stalled(agent, summary, now, stall_secs);
                    let status = if is_stalled {
                        ansi::ansi_fg("◐", &theme.danger)
                    } else {
                        ansi::ansi_fg("◐", &theme.success)
                    };
                    let name = ansi::ansi_fg(&agent.name, &theme.accent);
                    let task =
                        ansi::ansi_fg(&ansi::truncate(&agent.task_description, 40), &theme.muted);
                    let elapsed = summary
                        .last_event_secs
                        .map_or(0, |e| e.saturating_sub(agent.start_time_secs));
                    let elapsed_str = if summary.timestamps_reliable {
                        format_dur(elapsed)
                    } else {
                        format!("≈{}", format_dur(elapsed))
                    };
                    let time = ansi::ansi_fg(&elapsed_str, &theme.muted);
                    parts.push(format!("{} {} {} {}", status, name, task, time));
                }
```

（删除旧的 `is_stalled` 内联计算与 `let elapsed = agent.start_time_secs;`。）

（c）`render_dashboard`：`_config` 改名 `config`，循环内 stall 判定改为：

```rust
                for agent in &summary.agents {
                    let now = crate::core::state::now_secs();
                    let is_stalled = is_stalled(agent, summary, now, config.get_u64("stall_threshold_sec", 30));
```

（d）单元测试（`#[cfg(test)] mod tests` 追加）：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn summary(agents: Vec<AgentRecord>) -> TranscriptSummary {
        let mut s = TranscriptSummary::default();
        s.agents = agents;
        s
    }

    #[test]
    fn unreliable_session_elapsed_shows_approx_marker() {
        let mut s = summary(vec![AgentRecord {
            name: "a".into(),
            is_active: true,
            start_time_secs: 3,
            ..Default::default()
        }]);
        s.timestamps_reliable = false;
        let w = AgentDetail::new();
        w.update_transcript(&s);
        let out = w.render_compact(
            &SessionData::default(),
            &Theme::default(),
            &WidgetConfig::default(),
        );
        assert!(out.contains("≈"), "unreliable elapsed must be marked: {}", out);
    }

    #[test]
    fn reliable_session_elapsed_is_real_diff() {
        let mut s = summary(vec![AgentRecord {
            name: "a".into(),
            is_active: true,
            start_time_secs: 100,
            ..Default::default()
        }]);
        s.timestamps_reliable = true;
        s.last_event_secs = Some(160);
        let w = AgentDetail::new();
        w.update_transcript(&s);
        let out = w.render_compact(
            &SessionData::default(),
            &Theme::default(),
            &WidgetConfig::default(),
        );
        assert!(out.contains("60s"), "elapsed must be the real diff: {}", out);
        assert!(!out.contains("≈"), "reliable session must not be marked: {}", out);
    }

    #[test]
    fn is_stalled_requires_reliable_timeline_and_now() {
        let mut s = TranscriptSummary::default();
        s.timestamps_reliable = true;
        let agent = AgentRecord {
            name: "a".into(),
            is_active: true,
            last_tool_call_secs: Some(100),
            ..Default::default()
        };
        assert!(is_stalled(&agent, &s, 200, 30));
        assert!(!is_stalled(&agent, &s, 120, 30));
        s.timestamps_reliable = false;
        assert!(!is_stalled(&agent, &s, 200, 30));
    }
}
```

### Step 8: alerts.rs — stalled 真实触发 + 真实窗口 + 可靠门

（a）文件头 `use` 补：

```rust
use crate::core::state;
```

（b）`render_compact` 的 summary 块改为：

```rust
        if let Ok(ref guard) = self.summary.lock() {
            if let Some(ref summary) = **guard {
                // 卡顿检测只在时间轴可靠时真实触发；不可靠会话不猜测
                if summary.timestamps_reliable {
                    let stalled = summary.stalled_agents(30, state::now_secs());
                    if !stalled.is_empty() {
                        alerts.push(ansi::ansi_fg(&format!("⚠ {} stalled", stalled.len()), &theme.danger));
                    }
                }
                if let Some(minutes) = summary.compaction_prediction(pct, data.context_window.context_window_size) {
                    if minutes < 10 {
                        alerts.push(ansi::ansi_fg(&format!("compact ~{}m", minutes), &theme.warning));
                    }
                }
            }
        }
```

（c）`render_dashboard` 的 summary 块同样加可靠门并传 `state::now_secs()`：

```rust
        if let Ok(ref guard) = self.summary.lock() {
            if let Some(ref summary) = **guard {
                if summary.timestamps_reliable {
                    for agent in summary.stalled_agents(30, state::now_secs()) {
                        lines.push(Line::from(Span::styled(
                            format!("⚠ Agent '{}' stalled >30s", agent.name),
                            Style::default().fg(ansi::parse_ratatui_color(&theme.danger)))));
                    }
                }
            }
        }
```

### Step 9: 构建 + 单测

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test`
Expected: 全部通过。注意 `read_updates_returns_cumulative_summary` / `cross_process_accumulation_via_state` / `truncated_file_resets_cumulative_state` 三个既有测试不应受影响（agents.jsonl 首行带 ts → 现在为可靠会话；断言只涉及 tool_counts/total_tokens/last_pos）。

### Step 10: 黑盒 — assertions.py `_dig` 支持列表索引

`scripts/hudlib/assertions.py` 的 `_dig` 改为：

```python
def _dig(node, path):
    """Dig a dot path; _MISSING when any segment is absent.
    Integer segments index into lists (e.g. transcript.agents.0.name)."""
    for part in path.split("."):
        if part.isdigit() and isinstance(node, list):
            i = int(part)
            if i < len(node):
                node = node[i]
                continue
            return _MISSING
        if isinstance(node, dict) and part in node:
            node = node[part]
        else:
            return _MISSING
    return node
```

### Step 11: 黑盒 — P2-04 / P2-10

`scripts/hudlib/cases.py`：文件头补 import，P2 列表末尾追加两个用例，计数改 101：

```python
from datetime import datetime, timezone
```

```python
TS_ALPHA_START = int(datetime(2026, 7, 31, 10, 1, 0, tzinfo=timezone.utc).timestamp())
TS_TOOL_USE = int(datetime(2026, 7, 31, 10, 2, 0, tzinfo=timezone.utc).timestamp())
```

```python
    render_case("P2-04", "timestamps 真实时间轴落盘", "P2",
                {"exit": 0, "stderr_empty": True,
                 "state_json": {"equals": {
                     "transcript.timestamps_reliable": True,
                     "transcript.agents.0.start_time_secs": TS_ALPHA_START,
                     "transcript.agents.0.last_tool_call_secs": TS_TOOL_USE,
                     "transcript.last_pos": "<FIXTURE_SIZE>"}}},
                stdin=j(full_dict()), config=DEFAULT_CONFIG,
                transcript_copy="timestamps.jsonl",
                note="任务④：真实 ISO8601 → epoch 精确落盘；agents 按名排序保证 agents.0 确定性"),
    render_case("P2-10", "no_ts 降级端到端", "P2",
                {"exit": 0, "stderr_empty": True,
                 "state_json": {"equals": {
                     "transcript.timestamps_reliable": False,
                     "transcript.last_pos": "<FIXTURE_SIZE>"}}},
                stdin=j(full_dict()), config=DEFAULT_CONFIG,
                transcript_copy="no_ts.jsonl",
                note="任务④：首条无 timestamp → 降级路径，标志持久化为 false"),
]
```

```python
CASES = D1 + D2 + D3 + D4 + D5 + D6 + D7 + D8 + P1 + P2
assert len(CASES) == 101, f"expected 101 cases, got {len(CASES)}"
```

### Step 12: 构建 + 全量黑盒

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build && python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
Expected: 101/101 通过。注意 P2-04 依赖 Step 10 的 `_dig` 列表索引；P1-01 等既有用例的 `equals` 路径不含列表段，不受影响。

### Step 13: Commit（用户执行）

```bash
git add src/core/transcript.rs src/widgets/agent_detail.rs src/widgets/alerts.rs \
        fixtures/transcript/timestamps.jsonl fixtures/transcript/no_ts.jsonl \
        scripts/hudlib/assertions.py scripts/hudlib/cases.py
git commit -m "feat: real timestamps — ISO8601 time axis + reliability degradation + epoch buckets + real stalled/compaction"
```

---

## Task 3: ⑭ 成本正确性（currency_symbol + pricing.rs + 注入 + context_bar + P2-05..09）

**Files:**
- Create: `src/core/pricing.rs`（PriceEntry + PricingTable + effective_cost + inject_cost + 单测）
- Modify: `src/core/config.rs`（`currency_symbol` + `pricing` 段 + 单测）
- Modify: `src/core/mod.rs`（注册 pricing 模块）
- Modify: `src/compact.rs`（render_with_data 签名 + 注入 + notify 接线）
- Modify: `src/dashboard.rs`（summary 提升 + draw_dashboard 注入 + notify 接线）
- Modify: `src/doctor.rs`（sample_render 调用点 + [pricing] 校验）
- Modify: `src/alert.rs`（send_notifications 签名 + cost_threshold 调用）
- Modify: `src/notify.rs`（cost_threshold 币种参数）
- Modify: `src/widgets/cost_display.rs`（effective_cost 优先 + 符号注入）
- Modify: `src/widgets/alerts.rs`（成本行符号 + ≈ 标注）
- Modify: `src/widgets/context_bar.rs`（`12.3k/45.6k tok`）
- Modify: `scripts/hudlib/cases.py`（P2-05..09 + CASES 计数）

### Step 1: config.rs — `currency_symbol` + `[pricing]`

（a）`AppConfig` 追加两字段（放在 `alerts` 之后）：

```rust
    #[serde(default = "default_currency_symbol")]
    pub currency_symbol: String,

    #[serde(default)]
    pub pricing: HashMap<String, crate::core::pricing::PriceEntry>,
```

（b）默认函数与 Default impl：

```rust
fn default_currency_symbol() -> String {
    "$".into()
}
```

`impl Default for AppConfig` 追加：

```rust
            currency_symbol: "$".into(),
            pricing: HashMap::new(),
```

（c）单测追加：

```rust
    #[test]
    fn currency_symbol_and_pricing_defaults() {
        let c = AppConfig::default();
        assert_eq!(c.currency_symbol, "$");
        assert!(c.pricing.is_empty());
    }

    #[test]
    fn pricing_table_parses_with_field_defaults() {
        let toml_str = r#"
            currency_symbol = "¥"
            [pricing]
            "m1" = { input = 1e-6, output = 2e-6 }
        "#;
        let cfg: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.currency_symbol, "¥");
        let p = cfg.pricing.get("m1").expect("model price parsed");
        assert_eq!(p.input, 1e-6);
        assert_eq!(p.output, 2e-6);
        assert_eq!(p.cache_read, 0.0); // 缺省按 0
        assert_eq!(p.cache_creation, 0.0);
    }
```

（`use super::super::pricing::PriceEntry;` 或全路径 `crate::core::pricing::PriceEntry`，以 config.rs 现有 `use` 风格为准——建议全路径。）

### Step 2: pricing.rs — 新建（`src/core/pricing.rs`）

```rust
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::config::AppConfig;
use super::session::SessionData;
use super::transcript::{TranscriptSummary, TokenTotal};
use super::widget::WidgetConfig;

/// 模型单价（USD/token）。字段可缺省：缺省按 0 计，重算值偏小并带 ≈ 标注
/// （诚实降级，spec §6 错误矩阵）。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PriceEntry {
    #[serde(default)]
    pub input: f64,
    #[serde(default)]
    pub output: f64,
    #[serde(default)]
    pub cache_read: f64,
    #[serde(default)]
    pub cache_creation: f64,
}

pub type PricingTable = HashMap<String, PriceEntry>;

/// 三态成本计算（spec §2.1）：
/// - [pricing] 命中 + transcript 有累计 token → 按单价重算（≈ 标注）
/// - 未命中 → 透传 data.cost.total_cost_usd（官方价含 cache）
/// - 命中但无 transcript/token → 透传（无数据可算，不算估算）
pub fn effective_cost(
    data: &SessionData,
    summary: &TranscriptSummary,
    pricing: &PricingTable,
) -> (f64, bool) {
    if let Some(price) = pricing.get(&data.model.id) {
        let t = &summary.total_tokens;
        let has_tokens =
            t.input > 0 || t.output > 0 || t.cache_created > 0 || t.cache_read > 0;
        if has_tokens {
            let cost = price.input * t.input as f64
                + price.output * t.output as f64
                + price.cache_read * t.cache_read as f64
                + price.cache_creation * t.cache_created as f64;
            return (cost, true);
        }
    }
    (data.cost.total_cost_usd, false)
}

/// 把 effective cost / 估算标记 / 币种注入 WidgetConfig。
/// compact.rs 与 dashboard.rs 两条管线共用（widget 签名零改动）。
pub fn inject_cost(
    data: &SessionData,
    summary: Option<&TranscriptSummary>,
    config: &AppConfig,
    widget_config: &mut WidgetConfig,
) {
    if let Some(summary) = summary {
        let (cost, estimated) = effective_cost(data, summary, &config.pricing);
        widget_config
            .values
            .insert("effective_cost".into(), cost.to_string());
        widget_config
            .values
            .insert("cost_estimated".into(), estimated.to_string());
    }
    widget_config
        .values
        .insert("currency_symbol".into(), config.currency_symbol.clone());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(model: &str, official_cost: f64) -> SessionData {
        let json = format!(
            r#"{{"model":{{"id":"{model}","display_name":"{model}"}},
                "context_window":{{"used_percentage":1,"total_input_tokens":1,
                "context_window_size":200000}},
                "cost":{{"total_cost_usd":{official_cost},"total_duration_ms":1}}}}"#
        );
        SessionData::from_stdin_json(&json).unwrap()
    }

    fn summary_with_tokens(input: u64, output: u64, cache_read: u64, cache_created: u64) -> TranscriptSummary {
        let mut s = TranscriptSummary::default();
        s.total_tokens = TokenTotal {
            input,
            output,
            cache_created,
            cache_read,
        };
        s
    }

    #[test]
    fn hit_with_tokens_recomputes_and_marks_estimated() {
        let data = session("m1", 9.99);
        let summary = summary_with_tokens(1_000_000, 500_000, 100_000, 10_000);
        let mut pricing = PricingTable::new();
        pricing.insert(
            "m1".into(),
            PriceEntry {
                input: 1e-6,
                output: 2e-6,
                cache_read: 0.5e-6,
                cache_creation: 2.5e-6,
            },
        );
        let (cost, estimated) = effective_cost(&data, &summary, &pricing);
        // 1.0 + 1.0 + 0.05 + 0.025
        assert!((cost - 2.075).abs() < 1e-9);
        assert!(estimated);
    }

    #[test]
    fn miss_passthroughs_official_cost() {
        let data = session("m2", 0.034);
        let summary = summary_with_tokens(100, 100, 0, 0);
        let pricing = PricingTable::new();
        let (cost, estimated) = effective_cost(&data, &summary, &pricing);
        assert_eq!(cost, 0.034);
        assert!(!estimated);
    }

    #[test]
    fn hit_without_tokens_passthroughs() {
        let data = session("m1", 0.034);
        let summary = TranscriptSummary::default(); // 零 token
        let mut pricing = PricingTable::new();
        pricing.insert("m1".into(), PriceEntry { input: 1e-6, ..Default::default() });
        let (cost, estimated) = effective_cost(&data, &summary, &pricing);
        assert_eq!(cost, 0.034);
        assert!(!estimated);
    }

    #[test]
    fn partial_prices_count_missing_as_zero() {
        let data = session("m1", 9.99);
        let summary = summary_with_tokens(1000, 0, 0, 0);
        let mut pricing = PricingTable::new();
        pricing.insert("m1".into(), PriceEntry { input: 1e-3, ..Default::default() });
        let (cost, estimated) = effective_cost(&data, &summary, &pricing);
        assert!((cost - 1.0).abs() < 1e-12);
        assert!(estimated); // 部分单价缺失 → 值偏小但仍标 ≈（诚实）
    }

    #[test]
    fn inject_cost_adds_keys() {
        let data = session("m1", 0.5);
        let summary = summary_with_tokens(1000, 0, 0, 0);
        let mut pricing = PricingTable::new();
        pricing.insert("m1".into(), PriceEntry { input: 1e-3, ..Default::default() });
        let mut config = AppConfig::default();
        config.currency_symbol = "¥".into();
        config.pricing = pricing;
        let mut wc = WidgetConfig::default();
        inject_cost(&data, Some(&summary), &config, &mut wc);
        assert_eq!(wc.get_str("currency_symbol", ""), "¥");
        assert_eq!(wc.get_f64("effective_cost", -1.0), 1.0);
        assert!(wc.get_bool("cost_estimated", false));
        // 无 summary → 只注入币种，不注入成本键
        let mut wc2 = WidgetConfig::default();
        inject_cost(&data, None, &config, &mut wc2);
        assert_eq!(wc2.get_str("currency_symbol", ""), "¥");
        assert_eq!(wc2.get_f64("effective_cost", -1.0), -1.0);
    }
}
```

### Step 3: core/mod.rs — 注册模块

在既有 `pub mod ...` 行附近加：

```rust
pub mod pricing;
```

### Step 4: compact.rs — 管线注入 + notify 接线

（a）`use` 补：

```rust
use crate::core::pricing;
use crate::core::transcript::TranscriptSummary;
```

（b）`run_pipeline` 中 `render_with_data` 调用（第 49 行）与 notify 接线：

```rust
    let output = render_with_data(data, registry, config, theme, Some(&summary))?;

    // ⑦ 越阈告警：render 是跨进程冷却权威（加载 → 判定 → 回写 state.alerts）
    let now = state::now_secs();
    let mut cooldown = alert::AlertCooldown::from_state(&state.alerts);
    let fired = alert::check_alerts(&data, &config.alerts, &mut cooldown, now);
    let (effective_cost, _) = pricing::effective_cost(data, &summary, &config.pricing);
    alert::send_notifications(
        &fired,
        &data,
        &config.alerts,
        &config.currency_symbol,
        effective_cost,
    );
```

（c）`render_with_data` 签名改为：

```rust
pub fn render_with_data(
    data: &SessionData,
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
    summary: Option<&TranscriptSummary>,
) -> Result<String, String> {
```

循环内 widget_config 构造处改为：

```rust
                let w = registry.get(id)?;
                let mut widget_config = config.widget_config(id);
                pricing::inject_cost(data, summary, config, &mut widget_config);
                let rendered = w.render_compact(data, theme, &widget_config);
```

### Step 5: doctor.rs — sample_render 调用点 + [pricing] 校验

（a）`sample_render` 调用改：

```rust
    compact::render_with_data(&data, registry, config, theme, None)
```

（b）新增校验函数（放在 `contract_probe` 之后）：

```rust
/// ⑭ [pricing] 校验：负单价为 failure（含模型名定位）；否则信息项。
fn pricing_check(config: &AppConfig, failures: &mut usize) {
    if config.pricing.is_empty() {
        println!("  [..] pricing: no [pricing] table (cost shown from official data)");
        return;
    }
    let bad: Vec<&String> = config
        .pricing
        .iter()
        .filter(|(_, p)| {
            p.input < 0.0 || p.output < 0.0 || p.cache_read < 0.0 || p.cache_creation < 0.0
        })
        .map(|(m, _)| m)
        .collect();
    if bad.is_empty() {
        println!(
            "  [ok] pricing: {} model(s) configured, prices non-negative",
            config.pricing.len()
        );
    } else {
        println!(
            "  [!!] pricing: negative price for model(s): {}",
            bad.join(", ")
        );
        *failures += 1;
    }
}
```

（c）`run` 中在 `contract_probe();` 之后加：

```rust
    pricing_check(config, &mut failures);
```

### Step 6: alert.rs + notify.rs — send_notifications 签名

（a）`alert.rs`：

```rust
pub fn send_notifications(
    fired: &[AlertKind],
    data: &SessionData,
    cfg: &AlertsConfig,
    symbol: &str,
    effective_cost: f64,
) {
    for kind in fired {
        match kind {
            AlertKind::ContextCritical => {
                crate::notify::context_critical(data.context_window.used_percentage)
            }
            AlertKind::CostThreshold => {
                crate::notify::cost_threshold(effective_cost, cfg.cost_threshold_usd, symbol)
            }
            AlertKind::RateLimit => {
                crate::notify::rate_limit_warning(data.rate_limits.five_hour.used_percentage)
            }
        }
    }
}
```

（b）`notify.rs`：

```rust
/// Convenience: cost threshold exceeded (symbol from config.currency_symbol).
pub fn cost_threshold(cost: f64, threshold: f64, symbol: &str) {
    send(
        "Cost Warning",
        &format!(
            "Session cost {}{:.2} exceeded threshold {}{:.2}.",
            symbol, cost, symbol, threshold
        ),
    );
}
```

### Step 7: cost_display.rs — effective_cost 优先

`render_compact` 改为：

```rust
    fn render_compact(&self, data: &SessionData, theme: &Theme, config: &WidgetConfig) -> String {
        let symbol = config.get_str("currency_symbol", "$");
        let cost = config.get_f64("effective_cost", data.cost.total_cost_usd);
        let estimated = config.get_bool("cost_estimated", false);
        let warn = config.get_f64("warn_threshold_usd", 10.0);
        let color = if cost >= warn { &theme.warning } else { &theme.success };
        let prefix = if estimated { "≈" } else { "" };
        format!(
            "{}{}{:.2}{}",
            ansi::ansi_fg(&format!("{}{}", prefix, symbol), color),
            cost,
            ansi::ansi_reset()
        )
    }
```

### Step 8: alerts.rs — 成本行符号 + ≈ 标注

`render_compact` 的成本告警段改为：

```rust
        let cost = config.get_f64("effective_cost", data.cost.total_cost_usd);
        let symbol = config.get_str("currency_symbol", "$");
        let estimated = config.get_bool("cost_estimated", false);
        if cost >= cost_warn {
            let prefix = if estimated { "≈" } else { "" };
            alerts.push(ansi::ansi_fg(&format!("{}{}{:.2}", prefix, symbol, cost), &theme.warning));
        }
```

（alerts.rs 需要 `use crate::core::state;` 已在 Task 2 Step 8 加入。）

### Step 9: context_bar.rs — tokens in/out

`render_compact` 改为（k 缩写 helper 放在 impl 外）：

```rust
    fn render_compact(&self, data: &SessionData, theme: &Theme, config: &WidgetConfig) -> String {
        let pct = data.context_window.used_percentage;
        let bar_width = config.get_u64("bar_width", theme.bar_width as u64) as usize;
        let filled = ((pct / 100.0) * (bar_width as f64)).round() as usize;
        let filled = filled.min(bar_width);
        let empty = bar_width - filled;
        let warn = config.get_f64("warn_threshold", 80.0);
        let critical = config.get_f64("critical_threshold", 95.0);
        let color = if pct >= critical { &theme.danger } else if pct >= warn { &theme.warning } else { &theme.success };
        let filled_str = theme.bar_filled.to_string().repeat(filled);
        let empty_str = theme.bar_empty.to_string().repeat(empty);
        format!("ctx {}{}{} {:.0}% {}/{} tok",
            ansi::ansi_fg(&filled_str, color),
            ansi::ansi_fg(&empty_str, &theme.border),
            ansi::ansi_reset(),
            pct,
            format_k(data.context_window.total_input_tokens),
            format_k(data.context_window.total_output_tokens))
    }
```

```rust
/// k 缩写：≥1000 时 x.xk（12.3k），否则原样。
fn format_k(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}
```

### Step 10: dashboard.rs — summary 提升 + 注入 + notify 接线

（a）`use` 补：

```rust
use crate::core::pricing;
use crate::core::transcript::TranscriptSummary;
```

（b）`run_loop`：在 `let history = ...` 后声明：

```rust
    let mut summary: Option<TranscriptSummary> = None;
```

transcript 读取段改为：

```rust
        // Read transcript updates and push to all widgets
        if let Some(ref mut reader) = transcript_reader {
            let s = reader.read_updates();
            for widget in &registry.widgets {
                widget.update_transcript(&s);
            }
            summary = Some(s);
        }
```

notify 段改为：

```rust
        // Check for notification triggers
        let fired = alert::check_alerts(&data, &config.alerts, &mut cooldown, state::now_secs());
        let effective_cost = pricing::effective_cost(
            &data,
            summary.as_ref().unwrap_or(&TranscriptSummary::default()),
            &config.pricing,
        )
        .0;
        alert::send_notifications(
            &fired,
            &data,
            &config.alerts,
            &config.currency_symbol,
            effective_cost,
        );

        terminal
            .draw(|frame| {
                draw_dashboard(frame, registry, &data, theme, config, summary.as_ref());
            })
            .map_err(|e| format!("draw: {}", e))?;
```

（c）`draw_dashboard` 签名与注入：

```rust
fn draw_dashboard(
    frame: &mut Frame,
    registry: &WidgetRegistry,
    data: &SessionData,
    theme: &Theme,
    config: &AppConfig,
    summary: Option<&TranscriptSummary>,
) {
```

循环内：

```rust
        if let Some(widget) = registry.get(widget_id) {
            let mut widget_config = config.widget_config(widget_id);
            pricing::inject_cost(data, summary, config, &mut widget_config);
            widget.render_dashboard(data, *panel_area, frame, theme, &widget_config);
        }
```

### Step 11: 构建 + 单测

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test`
Expected: 全部通过（新增 pricing 5 个 + config 2 个测试；`render_with_data` 两个调用点已改）。

### Step 12: 黑盒 — P2-05..09

`scripts/hudlib/cases.py` P2 列表末尾追加：

```python
    render_case("P2-05", "[pricing] 命中重算 ≈$", "P2",
                {"exit": 0, "stdout_contains": ["≈$0.56"], "stderr_empty": True},
                stdin=j(full_dict()), config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"cost_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"
                    "[pricing]\n"
                    "\"deepseek-v4-flash\" = { input = 0.001, output = 0.002 }\n"),
                transcript_copy="timestamps.jsonl",
                note="任务⑭：timestamps.jsonl 累计 input=300 output=130 → 0.3+0.26=0.56，≈ 标注"),
    render_case("P2-06", "无 [pricing] 透传官方价", "P2",
                {"exit": 0, "stdout_contains": ["$0.03"],
                 "stdout_not_contains": ["≈"]},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                note="任务⑭：未命中 → 透传 data.cost.total_cost_usd，无 ≈"),
    render_case("P2-07", "currency_symbol 全局生效", "P2",
                {"exit": 0, "stdout_contains": ["¥0.03"]},
                stdin_file="json/full.json", config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"cost_display\"]\n"
                    "currency_symbol = \"¥\"\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="任务⑭：顶层 currency_symbol 注入 cost_display（compact 路径代表四处接线）"),
    render_case("P2-08", "context_bar tokens k 缩写", "P2",
                {"exit": 0, "stdout_contains": ["6.8k/5.0k tok"]},
                stdin_file="json/full.json", config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"context_bar\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="任务⑭：in 6800 → 6.8k / out 5000 → 5.0k"),
    render_case("P2-09", "doctor 负单价校验", "P2",
                {"exit": 1, "stdout_contains": ["[!!]", "neg-model"]},
                args=["doctor"], config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"model_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"
                    "[pricing]\n"
                    "\"neg-model\" = { input = -0.000001 }\n"),
                note="任务⑭：负单价 → [!!] failure 含模型名定位"),
]
```

```python
CASES = D1 + D2 + D3 + D4 + D5 + D6 + D7 + D8 + P1 + P2
assert len(CASES) == 106, f"expected 106 cases, got {len(CASES)}"
```

注意 P2-09 依赖其之前用例（P2-05..08）留下的 state.json 无 last_error（成功 render 已清除），否则 doctor 会多计 1 个 failure——exit 仍为 1，但断言只查 `[!!]` 与模型名，不受影响。

### Step 13: 构建 + 全量黑盒

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build && python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
Expected: 106/106 通过。既有用例注意点：
- D3-04：断言仅 `0.03` 数字部分，`[widgets.cost_display] currency_symbol` 键被顶层注入覆盖（`$` 默认）——用例不回归。
- D3-05：cost_display warn 阈值逻辑不变。
- D8-02/03：agents.jsonl 现在为可靠会话，但用例只断言 exit/stderr——不回归。

### Step 14: Commit（用户执行）

```bash
git add src/core/pricing.rs src/core/config.rs src/core/mod.rs src/compact.rs \
        src/dashboard.rs src/doctor.rs src/alert.rs src/notify.rs \
        src/widgets/cost_display.rs src/widgets/alerts.rs src/widgets/context_bar.rs \
        scripts/hudlib/cases.py
git commit -m "feat: cost correctness — currency_symbol unification + [pricing] recompute + context_bar tokens + doctor pricing check"
```

---

## Task 4: 全量验证 + COMPLETE.md 状态回写

**Files:**
- Modify: `COMPLETE.md`（第 20/21 章）

### Step 1: 全量验证

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test && python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")" && claude-hud doctor`
Expected: cargo test 全绿；黑盒 106/106；doctor 8 项 + 契约探针 + pricing 信息项全绿。

### Step 2: COMPLETE.md 更新

- 第 20 章「完整实现」追加一行 Phase 2 要点：
  `· 输入契约（subagentStatusLine/扁平 rate_limits 双形态 + render --dump 键分类 + doctor 契约探针）`
  `· 真实时间轴（ISO8601 主时间轴 + timestamps_reliable 降级 + epoch 60s 分桶 + 真实卡顿/压缩预测）`
  `· 成本正确性（currency_symbol 全局 + [pricing] 三态重算 + context_bar tokens + doctor 负单价校验）`
- 第 21 章 roadmap 追加 `| Phase 2 契约与真实性 | 双命名契约 + 真实时间戳 + 成本正确性 + 黑盒用例 106 例 | ✅ |`
- 页脚更新时间戳（保持既有格式）。

### Step 3: Commit（用户执行）

```bash
git add COMPLETE.md
git commit -m "docs: COMPLETE.md Phase 2 status — contract + timestamps + pricing done"
```

---

## 自检（spec 覆盖对照）

| 规格 §9 验收项 | 落点 |
|---|---|
| camelCase 与 snake_case 双命名渲染 | Task 1 Step 1/7（P2-01） |
| `--dump` 分类 + doctor 探针 | Task 1 Step 3-5（P2-02/03） |
| 真实 timestamp：elapsed/卡顿/压缩精确 | Task 2 Step 3/6（单测） + Step 11（P2-04） |
| 无 timestamp：`≈` 估算、无伪精确 | Task 2 Step 7/8 + Step 11（P2-10） |
| stalled 真实触发 | Task 2 Step 6 单测 + Step 8 接线 |
| compaction 真实窗口 + 不可靠返回 None | Task 2 Step 4 |
| 默认 `$` + 四处接线 | Task 3 Step 1/4/6/10 + P2-07 |
| `[pricing]` 命中重算带 ≈ / 未命中透传 | Task 3 Step 2/4 + P2-05/06 |
| 坏单价 doctor 报错定位模型 | Task 3 Step 5 + P2-09 |
| 存量 config 无新键行为一致 | Task 3 Step 1（serde default） |
| context_bar tokens in/out | Task 3 Step 9 + P2-08 |
| cargo test 全绿 + 黑盒 106 | Task 3 Step 13 / Task 4 Step 1 |
| COMPLETE.md 回写 | Task 4 Step 2 |
