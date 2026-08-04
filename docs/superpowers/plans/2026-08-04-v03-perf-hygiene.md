# v0.3 性能与卫生批次实施计划（W1-W5）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 17 个构建 warning 清零 + serve SQLite 缓存 + token_timeline 上限 + 结账 double-billing 修复 + 状态栏预算进度显示；单测 112 → ~123，黑盒 138 → 140。

**Architecture:** 全部为局部改动，无架构调整。W1 按"不留死代码"原则删除未接线代码（动画原语 frame 制已被 ②⑧ 判死刑，未来 v0.4 按时间相位重建——见 spec §未来规划）；W2 加 30s TTL 静态缓存；W3 裁剪无界 Vec；W4 结账去重复用 `[alerts].cooldown_minutes`；W5 复用现成注入点。

**Tech Stack:** Rust（serde/tiny_http/rusqlite）、Python 黑盒套件。

**前置约定：**
- 本批次**不自动 git commit**（用户全局规则）；批次末统一询问用户是否代提交。
- 所有 cargo 命令加前缀：`export PATH="$HOME/.cargo/bin:$PATH" &&`。
- 黑盒套件：`python scripts/test_hud.py`（`--case` 可单跑）。
- 每个改源码的任务以 `cargo test` 全绿 + `cargo check` 无新增 warning 为通过标准。

---

### Task 1: W3 token_timeline 上限（360 桶 = 6h）

**Files:**
- Modify: `src/core/transcript.rs`（push 处 :448-451、`to_state()`、impl 区）

- [ ] **Step 1: 写失败测试**（transcript.rs 测试模块，追加）

```rust
#[test]
fn timeline_caps_at_360_buckets() {
    let mut reader = TranscriptReader::new(PathBuf::new());
    for i in 0..400u64 {
        reader.token_timeline.push(TokenSnapshot {
            timestamp_secs: i * 60,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: i,
        });
    }
    reader.cap_timeline();
    assert_eq!(reader.token_timeline.len(), 360);
    assert_eq!(reader.token_timeline[0].timestamp_secs, 40 * 60);
    assert_eq!(reader.token_timeline[359].timestamp_secs, 399 * 60);
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
    reader.cap_timeline();
    let summary = reader.cumulative_summary();
    // 6h 窗口内速率仍可算（首尾桶被保留）
    let mins = summary.compaction_prediction(50.0, 200_000);
    assert!(mins.is_some());
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test timeline_cap` → Expected: FAIL（`cap_timeline` 不存在 / `token_timeline` 字段访问限制视实况调整）

- [ ] **Step 3: 实现**（transcript.rs）

常量（TranscriptReader impl 上方）：
```rust
/// 时间线分桶上限：360 桶 × 60s = 6h 滚动窗口（压缩预测只读首尾桶，足够）。
const MAX_TIMELINE_BUCKETS: usize = 360;
```

impl TranscriptReader 内新增：
```rust
/// 裁剪时间线到最近 6h（push 后与 to_state 序列化前调用，恢复旧状态立即封顶）。
fn cap_timeline(&mut self) {
    let overflow = self.token_timeline.len().saturating_sub(MAX_TIMELINE_BUCKETS);
    if overflow > 0 {
        self.token_timeline.drain(0..overflow);
    }
}
```

调用点 1：`read_updates` 的 AssistantEntry 臂末尾（:451 push match 之后、`_ => {}` 之前）追加 `self.cap_timeline();`
调用点 2：`to_state()` 构造 `TranscriptSegment` 之前调用（在 from_state 恢复旧大文件后下一轮 to_state 立即封顶）。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test timeline_cap` → Expected: PASS；再跑 `cargo test` 全绿；`cargo check` 无新增 warning

---

### Task 2: W4 结账 double-billing 修复（同 path 冷却期内最多结账一次）

**Files:**
- Modify: `src/core/state.rs`（StateFile 增字段）、`src/compact.rs`（should_checkout + 结账块）

- [ ] **Step 1: 写失败测试**（compact.rs 测试模块，扩展现有 `should_checkout_four_states` 旁追加）

```rust
#[test]
fn checkout_skips_rebilling_same_path_within_cooldown() {
    // 振荡 A→B→A→B：第三次起 prev 路径已在冷却期内结账 → 跳过
    assert!(!should_checkout(100, Some("/a"), Some("/b"), "/a", 1000, 1200, 600));
    // 不同路径正常结账
    assert!(should_checkout(100, Some("/a"), Some("/b"), "/c", 1000, 1200, 600));
    // 从未结账（ts=0）不挡
    assert!(should_checkout(100, Some("/a"), Some("/b"), "/a", 0, 1200, 600));
    // 冷却过期（600s 窗口外）放行
    assert!(should_checkout(100, Some("/a"), Some("/b"), "/a", 1000, 1700, 600));
    // 边界：恰好 600s 视为过期（< 判定）
    assert!(should_checkout(100, Some("/a"), Some("/b"), "/a", 1000, 1600, 600));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test checkout` → Expected: FAIL（签名不匹配）

- [ ] **Step 3a: state.rs 增字段**（StateFile 结构，budget_tier 之后）

```rust
/// 结账去重（⑨+）：同一 transcript path 在冷却期内最多结账一次
/// （path 抖动 A→B→A→B 时防同一会话 double-billing）。
#[serde(default)]
pub last_checkout_path: String,
#[serde(default)]
pub last_checkout_ts: u64,
```

- [ ] **Step 3b: compact.rs 改 should_checkout 签名与逻辑**

```rust
pub fn should_checkout(
    prev_ts: u64,
    prev_path: Option<&str>,
    cur_path: Option<&str>,
    last_billed_path: &str,
    last_billed_ts: u64,
    now: u64,
    cooldown_secs: u64,
) -> bool {
    prev_ts != 0
        && !prev_path.map(|p| p.is_empty()).unwrap_or(true)
        && prev_path != cur_path
        && !(last_billed_ts != 0
            && prev_path == Some(last_billed_path)
            && now.saturating_sub(last_billed_ts) < cooldown_secs)
}
```

- [ ] **Step 3c: compact.rs 结账块更新**（:143-159，`now` 变量在 :111 已存在）

```rust
if should_checkout(
    state.snapshot.timestamp_secs,
    state.snapshot.transcript_path.as_deref(),
    data.transcript_path.as_deref(),
    &state.last_checkout_path,
    state.last_checkout_ts,
    now,
    config.alerts.cooldown_minutes * 60,
) {
    match HistoryStore::open() {
        Ok(h) => {
            let last = state.snapshot.to_session();
            if let Err(e) = h
                .record_session(&last, state.snapshot.agent_count, &config.active_mod)
            {
                eprintln!("[claude-hud] warning: session checkout failed: {}", e);
            }
        }
        Err(e) => eprintln!("[claude-hud] warning: cannot open history db: {}", e),
    }
    state.last_checkout_path = state.snapshot.transcript_path.clone().unwrap_or_default();
    state.last_checkout_ts = now;
}
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test checkout` → PASS；`cargo test` 全绿；`cargo check` 无新增 warning

---

### Task 3: W2 serve 历史缓存（30s TTL）

**Files:**
- Modify: `src/serve.rs`

- [ ] **Step 1: 写失败测试**（serve.rs 测试模块，文件末尾新增）

```rust
#[cfg(test)]
mod tests {
    use super::ttl_fresh;
    use std::time::{Duration, Instant};

    #[test]
    fn ttl_fresh_boundary() {
        let t0 = Instant::now();
        assert!(ttl_fresh(t0, t0 + Duration::from_secs(29), Duration::from_secs(30)));
        assert!(!ttl_fresh(t0, t0 + Duration::from_secs(30), Duration::from_secs(30)));
        assert!(!ttl_fresh(t0, t0 + Duration::from_secs(301), Duration::from_secs(300)));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test ttl_fresh` → Expected: FAIL（ttl_fresh 不存在）

- [ ] **Step 3: 实现**（serve.rs）

```rust
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// ⑨+㉑ 历史聚合缓存：前端 2s 轮询 /api/data，weekly/trend 是分钟级统计，
/// 每请求重开 SQLite 纯属空转。30s TTL 内命中缓存不重查。
const HISTORY_TTL: Duration = Duration::from_secs(30);
static HISTORY_CACHE: Mutex<Option<(Instant, String, String)>> = Mutex::new(None);
// (fetched_at, weekly_json, trend_json)

fn ttl_fresh(fetched_at: Instant, now: Instant, ttl: Duration) -> bool {
    now.duration_since(fetched_at) < ttl
}

fn cached_history() -> (String, String) {
    let mut guard = HISTORY_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    if let Some((at, weekly, trend)) = guard.as_ref() {
        if ttl_fresh(*at, now, HISTORY_TTL) {
            return (weekly.clone(), trend.clone());
        }
    }
    let weekly = weekly_json_inner();
    let trend = trend_json_inner();
    *guard = Some((now, weekly.clone(), trend.clone()));
    (weekly, trend)
}
```

- [ ] **Step 4: 接线**

将现有 `weekly_json` 重命名为 `weekly_json_inner`、`trend_json` 重命名为 `trend_json_inner`（保留 ⑨/㉑ doc 注释）；`build_api_json` 中改为：
```rust
let (weekly, trend) = cached_history();
```
并把 format! 里的 `weekly_json(),` / `trend_json(),` 替换为 `weekly,` / `trend,`。

- [ ] **Step 5: 运行确认通过**

Run: `cargo test ttl_fresh` → PASS；`cargo test` 全绿；`cargo check` 无新增 warning

---

### Task 4: W5 状态栏预算进度显示

**Files:**
- Modify: `src/core/pricing.rs`（inject_cost_realtime 尾部）、`src/widgets/cost_display.rs`（render_compact + 测试）

- [ ] **Step 1: 写失败测试**（cost_display.rs 测试模块，format_tokens 测试旁追加）

```rust
use crate::core::session::SessionData;
use crate::core::widget::WidgetConfig;
use crate::core::theme::Theme;

fn session_data() -> SessionData {
    SessionData::from_stdin_json(
        r#"{"model":{"id":"m","display_name":"M"},
            "context_window":{"total_input_tokens":1000,"total_output_tokens":2000},
            "cost":{"total_cost_usd":0.0}}"#,
    )
    .unwrap()
}

fn cfg(extra: &[(&str, &str)]) -> WidgetConfig {
    WidgetConfig {
        values: extra.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
    }
}

#[test]
fn budget_pct_shown_when_configured() {
    let out = CostDisplay.render_compact(
        &session_data(), &Theme::default(), &cfg(&[
            ("effective_cost", "3.1"),
            ("cost_estimated", "true"),
            ("budget_cap_usd", "5.0"),
        ]));
    assert!(out.contains("· 62%"));
    assert!(out.contains("≈$3.10"));
}

#[test]
fn budget_pct_hidden_when_cap_zero() {
    let out = CostDisplay.render_compact(
        &session_data(), &Theme::default(), &cfg(&[
            ("effective_cost", "3.1"),
            ("cost_estimated", "true"),
            ("budget_cap_usd", "0"),
        ]));
    assert!(!out.contains("%"));
}

#[test]
fn zero_data_still_downgrades_to_dash() {
    let out = CostDisplay.render_compact(&session_data(), &Theme::default(), &WidgetConfig::default());
    assert_eq!(out, "—");
}
```

（若 `Theme::default()` 或 `SessionData` 构造方式与既有测试不一致，参照现有 widget 测试调整；`WidgetConfig { values }` 字段为 pub 可直接构造。）

- [ ] **Step 2: 运行确认失败**

Run: `cargo test budget_pct` → Expected: FAIL（无 "· 62%"）

- [ ] **Step 3a: pricing.rs 注入**（inject_cost_realtime 末尾追加，与 effective_cost 同款写入方式）

```rust
widget_config.set_f64("budget_cap_usd", config.budget.cap_usd);
```
（若现有注入用 `values.insert(...)` 字符串方式，则保持一致：`widget_config.values.insert("budget_cap_usd".into(), config.budget.cap_usd.to_string());`）

- [ ] **Step 3b: cost_display.rs render_compact**（组构建后、ansi_fg 前）

```rust
let mut group = format!(
    "{}{}{:.2} · {}/{} tok",
    prefix,
    symbol,
    cost,
    format_tokens(t_in),
    format_tokens(t_out)
);
// ⑳+ 预算水位：cap_usd>0 且 cost>0 时组尾追加百分比（超 100 如实显示，不钳制）
let budget_cap = config.get_f64("budget_cap_usd", 0.0);
if budget_cap > 0.0 && cost > 0.0 {
    group.push_str(&format!(" · {:.0}%", (cost / budget_cap) * 100.0));
}
ansi::ansi_fg(&group, color)
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test budget_pct` → PASS；`cargo test` 全绿；`cargo check` 无新增 warning

---

### Task 5: W1a 简单清理（imports / 变量 / trait 方法 / 孤立函数）

**Files:**
- Modify: `src/core/pricing.rs:7`（删 `TokenTotal`）、`src/core/state.rs:5`（删 `PathBuf`，留 `Path`）、`src/widgets/context_bar.rs:3`（删 `Color`）、`src/widgets/script_widget.rs:10`（删 `use crate::core::ansi;`）、`src/widgets/model_display.rs:30`（`theme` → `_theme`）、`src/core/widget.rs:75-82`（删 `dashboard_size`/`needs_tick` 两个默认方法）、`src/core/theme.rs:225`（删 `interpolate_hex`）

- [ ] **Step 1: 逐处删除**（如上清单；删除前 grep 确认无引用）

- [ ] **Step 2: 验证**

Run: `cargo check 2>&1 | grep -c warning` → Expected: 13（17 - 4）；`cargo test` 全绿

---

### Task 6: W1b animation.rs 收缩

**Files:**
- Modify: `src/core/animation.rs`、`src/widgets/agent_detail.rs:48`

- [ ] **Step 1: animation.rs 删除**（保留 `AnimationState { frame }`、`new()`、`tick()`、`neon_breathing`）

删除清单：`enabled` 字段（:7-8）、`spectrum_cycle`（:34-37）、`eased_value`（:40-42）、`barber_offset`（:45-47）、`spark_frame`（:50-62）、`glitch_offset`（:65-71）、`marquee_offset`（:74-80）、`wave_offset`（:83-86）、`liquid_height`（:89-92）、`scanline_alpha`（:95-105）、`Spark` 结构体（:108-112）、`hsl_to_rgb`（:114-130）。
`new()` 签名去掉 `enabled` 参数：
```rust
pub fn new() -> Self {
    Self { frame: 0 }
}
```

- [ ] **Step 2: agent_detail.rs:48 同步**

```rust
anim: Mutex::new(AnimationState::new()),
```

- [ ] **Step 3: 若 animation.rs 测试模块引用了被删项，同步删除相关测试**

- [ ] **Step 4: 验证**

Run: `cargo check 2>&1 | grep -c warning` → Expected: 7（13 - 6）；`cargo test` 全绿

---

### Task 7: W1c 结构体字段清理（session / transcript / history）

**Files:**
- Modify: `src/core/session.rs`（SubagentInfo）、`src/core/transcript.rs`（枚举变体 + 结构体）、`src/core/history.rs`（SessionRecord + SELECT/INSERT）

- [ ] **Step 1: session.rs 删字段**（SubagentInfo，:117-126）

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct SubagentInfo {
    #[serde(default)]
    pub elapsed_secs: u64,
    #[serde(default)]
    pub is_active: bool,
}
```
（`name`/`model`/`task` 无任何读取方——stdin JSON 多余键 serde 自动忽略，契约探针不受影响；删除前 grep `\.name|\.model|\.task` 确认仅 transcript 侧 SubagentEntry 在用。）

- [ ] **Step 2: transcript.rs 删变体与结构体**

- 枚举删 `ToolResult(ToolResultEntry)`（:106）与 `UserEntry(UserEntry)`（:108）——serde tag 未知名落入 `#[serde(other)] Unknown`，解析不破。
- 删 `ToolResultEntry`（:156-160）与 `UserEntry`（:162-166）结构体；`MessageContent` 保留（AssistantEntry.message 在用）。
- `entry_ts`（:136-145）与 `read_updates` match（`_ => {}` 兜底）均不引用被删项，无需改。

- [ ] **Step 3: history.rs 三处同步**

SessionRecord 删字段（:19-21）：
```rust
pub struct SessionRecord {
    pub id: i64,
    pub started_at: String,
    pub duration_secs: u64,
    pub total_cost_usd: f64,
    pub total_tokens: u64,
    pub agent_count: usize,
}
```
recent_sessions SELECT（:145-146）删 `lines_added, lines_removed, mod_used`；行解析删 `row.get(6)..row.get(8)`（:160-162，序号顺移）。record_session INSERT（:97-98）改为：
```sql
INSERT INTO sessions (duration_secs, total_cost_usd, total_tokens, agent_count) VALUES (?1, ?2, ?3, ?4)
```
params 删 :104-106 三项。**CREATE TABLE 不动**（SQLite 列保留，0 默认值，无迁移）。

- [ ] **Step 4: 验证**

Run: `cargo check 2>&1 | grep -c warning` → Expected: **0**；`cargo test` 全绿

---

### Task 8: 黑盒用例（W4 振荡去重 + W5 预算显示）

**Files:**
- Modify: `scripts/hudlib/cases.py`（P5 列表追加 2 例，总数 138 → 140）

- [ ] **Step 1: W4 用例**（参照 P4-01 模板：pre_cmds 4 次 render A→B→A→B，remove_db=True）

```python
render_case("P5-09", "path 振荡 A→B→A→B → 结账去重仅 2 条", "P5",
            {"exit": 0, "stdout_contains": ["Sessions: 2", "#2"],
             "stdout_not_contains": ["#3"]},
            args=["history"], config=DEFAULT_CONFIG,
            pre_cmds=[
                {"args": ["render"],
                 "stdin": j(full_dict(**{"transcript_path": "/a.jsonl"}))},
                {"args": ["render"],
                 "stdin": j(full_dict(**{"transcript_path": "/b.jsonl"}))},
                {"args": ["render"],
                 "stdin": j(full_dict(**{"transcript_path": "/a.jsonl"}))},
                {"args": ["render"],
                 "stdin": j(full_dict(**{"transcript_path": "/b.jsonl"}))},
            ],
            remove_db=True,
            note="W4：r2 结账 A(#1)、r3 结账 B(#2)、r4 prev=A 冷却内跳过 → 恰好 2 条"),
```

- [ ] **Step 2: W5 用例**（config 带 [budget]+[pricing]，stdin 覆盖 token 使 pct 为整数 62）

```python
W5_CONFIG = DEFAULT_CONFIG + """
[budget]
cap_usd = 5.0

[pricing]
"deepseek-v4-flash" = { input = 1.0, output = 2.0 }
"""

render_case("P5-10", "[budget] 命中 → cost 组尾追加预算百分比", "P5",
            {"exit": 0, "stdout_contains": ["· 62%"]},
            stdin=j(full_dict(**{"total_input_tokens": 3_100_000,
                                 "total_output_tokens": 0})),
            config=W5_CONFIG, env_extra={"COLUMNS": "200"},
            note="W5：in=3.1M×$1/M → ≈$3.10，cap=5 → 62%；COLUMNS=200 防组级截断"),

render_case("P5-11", "无 [budget] → 无预算百分比", "P5",
            {"exit": 0, "stdout_contains": ["≈$3.10"],
             "stdout_not_contains": ["· 62%"]},
            stdin=j(full_dict(**{"total_input_tokens": 3_100_000,
                                 "total_output_tokens": 0})),
            config=DEFAULT_CONFIG, env_extra={"COLUMNS": "200"},
            note="W5：cap=0 注入 → cost 组无百分比后缀"),
```

> 若 DEFAULT_CONFIG 的 model.id 不是 `deepseek-v4-flash`，以实际 fixture model.id 为准调整 [pricing] 键；若 context_pct 恰为 62 造成子串歧义，改用 3_150_000 token（63%）。

- [ ] **Step 3: 运行确认通过**

Run: `python scripts/test_hud.py --case P5-09` / `--case P5-10` / `--case P5-11` → 各自 PASS；全量 `python scripts/test_hud.py` → 140/140

---

### Task 9: 文档同步

**Files:**
- Modify: `CHANGELOG.md`（[Unreleased] 段）、`COMPLETE.md`（§20 动画行 + §21 路线图）、`DEPLOY.md`（预算显示样例）、`TASKS.md`（延期队列加动画批次行）

- [ ] **Step 1: CHANGELOG [Unreleased] 追加**

```markdown
- v0.3 性能与卫生：17 构建 warning 清零（死代码删除，动画原语 frame 制脚手架移除——时间相位重建留 v0.4）+ serve 历史 30s TTL 缓存 + token_timeline 6h 窗口封顶 + 结账同 path 冷却去重（防 double-billing）+ 状态栏预算百分比（[budget] 命中时 cost 组尾 `· 62%`）
```

- [ ] **Step 2: COMPLETE.md**

- §20 🟡 动画行改为：`| 动画 | neon_breathing 接入 agent_detail（仪表盘卡顿色）；渐变进度条为 3 档变色；frame 制原语已删，时间相位重建排 v0.4 |`
- §20 ✅ 段尾部追加 v0.3 项（0 warnings + 缓存 + 封顶 + 去重 + 预算显示 + 黑盒 140 例）
- §21 路线图加行：`| v0.3 性能与卫生（W1-W5，2026-08-04） | 17 warning 清零 + serve 缓存 + timeline 封顶 + 结账去重 + 预算百分比 + 黑盒用例 140 例 + 单元测试 ~123 个 | ✅ |`

- [ ] **Step 3: DEPLOY.md**

- 预算告警（⑳）小节补一句：状态栏 `[budget]` 配置后 cost 组尾显示预算百分比（`≈$3.10 · 3.1M/0 tok · 62%`）；`cap_usd=0` 不显示。

- [ ] **Step 4: TASKS.md 延期队列追加行**

```markdown
| 动画接入（v0.4 候选） | 规划（2026-08-04 拍板） | v0.3 完成后启动；时间相位纯函数重建 animation.rs + 6 效果分档（渐变进度条/呼吸/缓动计数器/CRT 扫描线/伪 3D 面板/盲文频谱）；其余 9 种装饰效果拍板砍除（跑马灯被 ⑮ 截断取代）；可与仪表盘布局补全合并为 v0.4 视觉批次 |
```

---

### Task 10: 全量验证与提交

- [ ] **Step 1: 全量验证**

Run:
- `export PATH="$HOME/.cargo/bin:$PATH" && cargo check 2>&1 | grep -c warning` → **0**
- `cargo test` → 全绿（约 123 个）
- `python scripts/test_hud.py` → **140/140**
- `claude-hud doctor` → 输出正常

- [ ] **Step 2: 询问用户是否代提交**（用户全局规则：git 操作需显式授权；批次 5 个源码/测试/文档改动按主题拆分提交，参照 v0.2 做法）
