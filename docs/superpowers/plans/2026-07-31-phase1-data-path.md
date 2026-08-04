# 第一期（任务① ②⑧ ⑬ ⑫）实施计划

> **For agentic workers:** 本计划按任务逐条执行。步骤用 checkbox（`- [ ]`）跟踪。TDD：先写失败测试 → 验证失败 → 最小实现 → 验证通过。
>
> **提交约定（用户全局规范）**：本仓库禁止自动执行 `git add` / `git commit` / `git push`。每个任务末尾的"提交"步骤由**用户手动执行**（本计划只给出建议命令与提交信息）。

**Goal:** 落地 state.json 数据通路（render ↔ dashboard/serve 共享层），修复增量读取失效（②⑧）、TTY 阻塞（①）、通知防轰炸（⑫）、状态栏静默失效（⑬）。

**Architecture:** state.json（`~/.claude/plugins/claude-hud/state.json`）成为 render 与长驻进程的唯一共享层，五段结构全字段 `#[serde(default)]`，render 每次全量原子写。render 管线：读 stdin → 恢复 TranscriptReader（path 匹配才恢复）→ 累计 summary 推给 widgets → git/脚本 TTL 缓存 → 越阈告警（跨进程冷却）→ 渲染 → 持久化。dashboard/serve 用 IsTerminal 分发：TTY → 读 state 快照（不再卡死），非 TTY → 旧 stdin 路径（向后兼容）。

**Tech Stack:** Rust 2021、serde/serde_json（状态文件）、chrono 0.4（新增，ISO8601）、crossterm IsTerminal（分发）、既有 write_atomic 模式、Python 黑盒套件（scripts/test_hud.py + hudlib）。

---

## 设计适配说明（实现相对 spec 的机械调整，语义不变）

1. **git TTL 探测位置**：spec §4.1 管线步骤⑤"git 探测"在实现中由 `git_status` widget 在渲染时自持执行（`probe_git_cached(state_path)` 窄键读写 state 文件）——widget 是 git 数据的唯一消费者，管线无法把探测结果传给它。TTL 语义（branch/dirty 30s、ahead/behind 60s、命中不 spawn）与"回写"完整保留，与脚本节流同一模式。
2. **窄键写 vs 全量写的覆盖问题**：脚本节流 / git 缓存由 widget 在管线中途通过 read-modify-write 写 state 文件，管线步骤⑨的全量持久化会把它们冲掉。解法：持久化前 `StateFile::merge_cache_from_disk()` 把磁盘上的 cache 段（git + script_throttle）并入内存 state 再写（spec 并发规则"cache 写入方只有 render"的精神保留：全量写仍只来自 render，窄键写是自持 key 的例外，与 alerts 段同性质）。
3. **黑盒用例 5（dashboard 无 stdin + q 可退出）**：测试环境无 pty，无法机械注入按键。落地为：D7-01 存量回归（DEVNULL stdin 下 TUI 存活 10s 不崩溃）+ 手动验证步骤（Task 2 / Task 9 含终端内 `claude-hud dashboard` → 按 q 退出的手动检查）。新黑盒用例 P1-01..P1-06 覆盖 spec 用例 1/2/3/4/6，用例 7 = 存量回归。

## 文件结构

| 文件 | 责任 |
|------|------|
| `src/core/state.rs`（新增） | `StateFile` 五段结构、`read`/`write`/`update`/`write_last_error`/`merge_cache_from_disk`、`SnapshotSegment` 双向转换与 30s 新鲜度、`now_secs()`、`write_atomic`（从 main.rs 迁入）、共享 `read_current_data()` |
| `src/alert.rs`（新增） | `AlertKind` 枚举（Task 1 骨架，Task 7 完整）、`AlertCooldown`、纯函数 `check_alerts`、`send_notifications` |
| `src/core/transcript.rs` | 累计状态提升为 self 字段、`from_state`/`to_state`、截断重置、`TranscriptSegment` + 记录类型 serde |
| `src/core/config.rs` | `state_path()`、`[alerts]` 段 `AlertsConfig`（95/10/90/10，0=关闭） |
| `src/core/session.rs` | `CurrentUsage` 增加 `Serialize`（快照序列化需要） |
| `src/probe/git.rs` | `probe_git_cached(state_path)`：TTL 查缓存，命中复用不 spawn |
| `src/widgets/git_status.rs` | widget 改走 cached 探测（+state_path 字段） |
| `src/widgets/script_widget.rs` | 节流跨进程化（state.cache.script_throttle） |
| `src/widgets/alerts.rs` | 呼吸动画改时间相位驱动 |
| `src/compact.rs` | render 管线（恢复/累计/告警/持久化）、`should_restore`、`hud_err_marker` |
| `src/dashboard.rs` | IsTerminal 分发、state 快照初始化 reader/data、Task 7 换 alert.rs 告警 |
| `src/serve.rs` | 共用 `state::read_current_data()`，删本地卡死实现 |
| `src/main.rs` | `mod alert;`、render 错误路径（stdout 标记 + last_error）、`register_all(registry, &config)`、`write_atomic` 迁移 |
| `src/doctor.rs` | 新增检查项：state.json 有效且新鲜、last render failure |
| `src/widgets/mod.rs` | `register_all` 带 config、脚本 widget 构造带 state_path |
| `Cargo.toml` | + `chrono = "0.4"` |
| `scripts/hudlib/cases.py` | `render_case`/`serve_case` 透传扩展字段、growing transcript 工具、`read_state_json_value` |
| `scripts/hudlib/assertions.py` | `check_state_json` 状态文件断言 |
| `scripts/test_hud.py` | `pre_render_stdin` / `grow_fixture` / `remove_state` 机制 + state_json 集成 |
| `COMPLETE.md` | 第 20 章状态表：① ②⑧ ⑫ ⑬ → ✅ |

---

### Task 1: state.rs 骨架 + state_path + write_atomic 迁移

**Files:**
- Create: `src/core/state.rs`
- Create: `src/alert.rs`（仅 `AlertKind` 枚举，Task 7 补全）
- Modify: `src/core/mod.rs`、`src/main.rs`（`mod alert;`）、`src/core/session.rs`（CurrentUsage +Serialize）、`src/core/config.rs`（`state_path()`）、`Cargo.toml`（chrono 留到 Task 6，本任务不需要）
- Test: `src/core/state.rs` 内 `#[cfg(test)]`

- [ ] **Step 1: 接线模块 + 写失败测试**

`src/core/mod.rs` 追加一行（在 `pub mod cc_config;` 之后）：

```rust
pub mod state;
```

`src/main.rs` 第 7 行 `mod serve;` 之后追加：

```rust
mod alert;
```

创建 `src/alert.rs`（本任务只有枚举骨架）：

```rust
use serde::{Deserialize, Serialize};

/// Notification kinds, keyed by the same strings in state.json's alerts
/// segment (snake_case, e.g. "cost_threshold").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    ContextCritical,
    CostThreshold,
    RateLimit,
}
```

创建 `src/core/state.rs`，只写测试（此时 `StateFile` 等未定义，`cargo test` 编译失败 = RED）：

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::alert::AlertKind;

/// A fresh render snapshot is considered stale after this many seconds.
pub const SNAPSHOT_MAX_AGE_SECS: u64 = 30;

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("claude-hud-state-{}-{}", std::process::id(), name));
        p
    }

    fn cleanup(p: &Path) {
        let _ = std::fs::remove_file(p);
        let _ = std::fs::remove_file(p.with_extension("json.tmp"));
    }

    #[test]
    fn write_read_round_trip() {
        let path = tmp_path("roundtrip.json");
        cleanup(&path);
        let mut st = StateFile::default();
        st.snapshot.model.display_name = "deepseek-v4-flash".into();
        st.alerts.insert(AlertKind::CostThreshold, 42);
        assert!(st.write(&path).is_ok());
        let back = StateFile::read(&path);
        assert_eq!(back.snapshot.model.display_name, "deepseek-v4-flash");
        assert_eq!(back.alerts.get(&AlertKind::CostThreshold), Some(&42));
        cleanup(&path);
    }

    #[test]
    fn missing_file_reads_default() {
        let path = tmp_path("missing.json");
        cleanup(&path);
        let st = StateFile::read(&path);
        assert!(st.snapshot.model.display_name.is_empty());
        assert!(st.alerts.is_empty());
    }

    #[test]
    fn corrupt_file_reads_default() {
        let path = tmp_path("corrupt.json");
        cleanup(&path);
        std::fs::write(&path, "{ not json !!").unwrap();
        let st = StateFile::read(&path);
        assert!(st.last_error.is_none());
        cleanup(&path);
    }

    #[test]
    fn snapshot_freshness_window() {
        let now = 1_000_000;
        let mut snap = SnapshotSegment::default();
        assert!(!snap.is_fresh(now)); // ts 0 = never written
        snap.timestamp_secs = now - 10;
        assert!(snap.is_fresh(now));
        snap.timestamp_secs = now - SNAPSHOT_MAX_AGE_SECS - 1;
        assert!(!snap.is_fresh(now));
        assert!(snap.to_session_if_fresh(now).is_none());
        snap.timestamp_secs = now - 5;
        assert!(snap.to_session_if_fresh(now).is_some());
    }

    #[test]
    fn update_applies_read_modify_write() {
        let path = tmp_path("update.json");
        cleanup(&path);
        StateFile::update(&path, |st| st.snapshot.model.display_name = "x".into())
            .unwrap();
        let st = StateFile::read(&path);
        assert_eq!(st.snapshot.model.display_name, "x");
        cleanup(&path);
    }
}
```

- [ ] **Step 2: 跑测试验证失败**

Run: `cargo test state`
Expected: 编译错误（`StateFile`、`SnapshotSegment` 未定义）——测试全部失败。

- [ ] **Step 3: 实现 state.rs + state_path**

在 `src/core/state.rs` 的测试块**上方**追加完整实现（替换掉占位）：

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::config::AppConfig;
use super::session::{CurrentUsage, SessionData};
use super::transcript::TranscriptSegment;
use crate::alert::AlertKind;

/// Five-segment shared state file: the only data layer between the 5s render
/// process and long-running processes (dashboard / serve / doctor).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StateFile {
    #[serde(default)]
    pub snapshot: SnapshotSegment,
    #[serde(default)]
    pub transcript: TranscriptSegment,
    #[serde(default)]
    pub cache: CacheSegment,
    #[serde(default)]
    pub alerts: HashMap<AlertKind, u64>,
    #[serde(default)]
    pub last_error: Option<LastError>,
}

/// Last render failure, written before exit so doctor can surface it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LastError {
    #[serde(default)]
    pub ts_iso: String,
    #[serde(default)]
    pub msg: String,
}

/// Session snapshot restored by dashboard/serve when stdin is a TTY.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotSegment {
    #[serde(default)]
    pub timestamp_secs: u64,
    #[serde(default)]
    pub model: ModelSnapshot,
    #[serde(default)]
    pub context_window: ContextSnapshot,
    #[serde(default)]
    pub cost: CostSnapshot,
    #[serde(default)]
    pub rate_limits: RateLimitSnapshot,
    #[serde(default)]
    pub transcript_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelSnapshot {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub display_name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextSnapshot {
    #[serde(default)]
    pub used_percentage: f64,
    #[serde(default)]
    pub total_input_tokens: u64,
    #[serde(default)]
    pub total_output_tokens: u64,
    #[serde(default)]
    pub context_window_size: u64,
    #[serde(default)]
    pub current_usage: CurrentUsage,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostSnapshot {
    #[serde(default)]
    pub total_cost_usd: f64,
    #[serde(default)]
    pub total_duration_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RateLimitSnapshot {
    #[serde(default)]
    pub five_hour_pct: f64,
    #[serde(default)]
    pub seven_day_pct: f64,
}

/// Best-effort probe caches: values are reused without spawning processes
/// while their TTL is unexpired.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheSegment {
    #[serde(default)]
    pub git: GitCache,
    #[serde(default)]
    pub script_throttle: HashMap<String, u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitCache {
    #[serde(default)]
    pub branch: CachedValue<String>,
    #[serde(default)]
    pub dirty: CachedValue<bool>,
    #[serde(default)]
    pub ahead_behind: CachedValue<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CachedValue<T> {
    #[serde(default)]
    pub value: T,
    #[serde(default)]
    pub ts: u64,
}

impl StateFile {
    /// Read the state file; missing or corrupt files yield defaults
    /// (never a hard failure).
    pub fn read(path: &Path) -> StateFile {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return StateFile::default(),
        };
        serde_json::from_str(&content).unwrap_or_default()
    }

    /// Write the whole file atomically; creates parent dirs.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("serialize state: {}", e))?;
        write_atomic(path, &json)
    }

    /// Read-modify-write: applies `f` to the on-disk state and writes it back.
    pub fn update(path: &Path, f: impl FnOnce(&mut StateFile)) -> Result<(), String> {
        let mut st = StateFile::read(path);
        f(&mut st);
        st.write(path)
    }

    /// Overlay the on-disk cache into `self` so a full-state persist never
    /// clobbers narrow cache writes made by widgets mid-pipeline.
    pub fn merge_cache_from_disk(&mut self, path: &Path) {
        let disk = StateFile::read(path);
        self.cache.git = disk.cache.git;
        self.cache.script_throttle = disk.cache.script_throttle;
    }
}

impl SnapshotSegment {
    pub fn from_session(data: &SessionData, now_secs: u64) -> Self {
        Self {
            timestamp_secs: now_secs,
            model: ModelSnapshot {
                id: data.model.id.clone(),
                display_name: data.model.display_name.clone(),
            },
            context_window: ContextSnapshot {
                used_percentage: data.context_window.used_percentage,
                total_input_tokens: data.context_window.total_input_tokens,
                total_output_tokens: data.context_window.total_output_tokens,
                context_window_size: data.context_window.context_window_size,
                current_usage: data.context_window.current_usage.clone(),
            },
            cost: CostSnapshot {
                total_cost_usd: data.cost.total_cost_usd,
                total_duration_ms: data.cost.total_duration_ms,
            },
            rate_limits: RateLimitSnapshot {
                five_hour_pct: data.rate_limits.five_hour.used_percentage,
                seven_day_pct: data.rate_limits.seven_day.used_percentage,
            },
            transcript_path: data.transcript_path.clone(),
        }
    }

    pub fn to_session(&self) -> SessionData {
        SessionData {
            model: super::session::ModelInfo {
                id: self.model.id.clone(),
                display_name: self.model.display_name.clone(),
            },
            context_window: super::session::ContextWindow {
                used_percentage: self.context_window.used_percentage,
                total_input_tokens: self.context_window.total_input_tokens,
                total_output_tokens: self.context_window.total_output_tokens,
                context_window_size: self.context_window.context_window_size,
                current_usage: self.context_window.current_usage.clone(),
            },
            cost: super::session::CostInfo {
                total_cost_usd: self.cost.total_cost_usd,
                total_duration_ms: self.cost.total_duration_ms,
                total_lines_added: 0,
                total_lines_removed: 0,
            },
            rate_limits: super::session::RateLimits {
                five_hour: super::session::RateLimitBucket {
                    used_percentage: self.rate_limits.five_hour_pct,
                },
                seven_day: super::session::RateLimitBucket {
                    used_percentage: self.rate_limits.seven_day_pct,
                },
            },
            transcript_path: self.transcript_path.clone(),
            subagent_status_line: None,
        }
    }

    /// True when the snapshot is fresh enough to be presented as live data.
    pub fn is_fresh(&self, now_secs: u64) -> bool {
        self.timestamp_secs != 0
            && now_secs.saturating_sub(self.timestamp_secs) <= SNAPSHOT_MAX_AGE_SECS
    }

    pub fn to_session_if_fresh(&self, now_secs: u64) -> Option<SessionData> {
        self.is_fresh(now_secs).then(|| self.to_session())
    }
}

/// Unix epoch seconds — single time source for cache TTLs and cooldowns.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Write atomically (temp file + rename) so a crash mid-write never leaves
/// the target truncated or partially written.
pub fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, content)
        .map_err(|e| format!("write {}: {}", tmp.display(), e))?;
    fs::rename(&tmp, path)
        .map_err(|e| format!("rename to {}: {}", path.display(), e))?;
    Ok(())
}

/// Current session data: piped stdin (legacy, unchanged) or, when stdin is a
/// TTY, the freshest state.json snapshot (never blocks on the terminal).
pub fn read_current_data() -> Option<SessionData> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        return read_stdin_json();
    }
    let path = AppConfig::state_path().ok()?;
    let st = StateFile::read(&path);
    st.snapshot.to_session_if_fresh(now_secs())
}

fn read_stdin_json() -> Option<SessionData> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).ok()?;
    SessionData::from_stdin_json(&buf).ok()
}
```

`src/core/session.rs` 第 35 行 `CurrentUsage` 的 derive 增加 `Serialize`：

```rust
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct CurrentUsage {
```

`src/core/config.rs` 在 `impl AppConfig` 的 `mods_dir()`（第 170 行）之后追加：

```rust
    pub fn state_path() -> Result<PathBuf, String> {
        let base = dirs::home_dir()
            .ok_or_else(|| "cannot find home directory".to_string())?;
        Ok(base.join(".claude").join("plugins").join("claude-hud").join("state.json"))
    }
```

`src/main.rs`：删除本地 `write_atomic`（第 177-184 行），并删除第 177 行的注释块；第 10-14 行 import 区追加 `use core::state::write_atomic;`。`run_setup`（第 236 行）与 `run_uninstall`（第 251 行）的 `write_atomic(...)` 调用保持不变（解析到新 import）。

注意：`StateFile` 引用 `TranscriptSegment`（`super::transcript`），该类型在 Task 3 才定义——本任务先建一个最小占位，保证编译：

`src/core/transcript.rs` 顶部（`use serde::Deserialize;` 之下）追加：

```rust
use serde::{Deserialize, Serialize};
```

并在文件末尾（`impl TranscriptSummary` 之后）追加占位（Task 3 替换为完整定义）：

```rust
/// Cross-process persisted transcript state (state.json `transcript`
/// segment). Full definition lands in Task 3.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TranscriptSegment {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub last_pos: u64,
}
```

- [ ] **Step 4: 跑测试验证通过**

Run: `cargo test state`
Expected: `test result: ok. 5 passed`（state 模块 5 个测试全绿），其余既有测试不受影响。

- [ ] **Step 5: 提交（用户手动执行）**

```bash
git add src/core/state.rs src/alert.rs src/core/mod.rs src/core/session.rs src/core/config.rs src/core/transcript.rs src/main.rs
git commit -m "feat: state.json 五段共享层骨架（state.rs + state_path + write_atomic 迁移）"
```

---

### Task 2: dashboard/serve IsTerminal 分发 + 快照初始化

**Files:**
- Modify: `src/dashboard.rs`、`src/serve.rs`
- Test: 无新单元测试（TTY 检测无法单测，快照新鲜度已由 Task 1 的 `snapshot_freshness_window` 覆盖）；验证靠 `cargo test` + D6/D7 黑盒回归 + 手动 q 退出

- [ ] **Step 1: 实现分发（本次修复的核心：dashboard.rs:174-179 / serve.rs:228-233 的 TTY 阻塞）**

`src/dashboard.rs`：

- 第 2 行 `use std::io;` 保留（`io::stdout` 用）；删除 `read_current_data` 函数（第 174-179 行）。
- `run_loop`（第 44-104 行）改为：

```rust
fn run_loop(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> Result<(), String> {
    let tick_rate = std::time::Duration::from_millis(config.dashboard.refresh_interval_ms);
    let mut last_agent_count: usize = 0;

    // 启动时从 state.json 恢复：数据（新鲜快照）、transcript 游标、告警冷却
    let initial = StateFile::read(&AppConfig::state_path().unwrap_or_default());
    let mut data = initial
        .snapshot
        .to_session_if_fresh(state::now_secs())
        .unwrap_or_default();
    let mut transcript_reader: Option<TranscriptReader> = if initial.transcript.path.is_empty() {
        None
    } else {
        Some(TranscriptReader::from_state(&initial.transcript))
    };

    // Open history store for session recording
    let history = HistoryStore::open().ok();

    loop {
        // TTY → state.json 快照；非 TTY → 旧 stdin 路径。None 时保留上次数据
        // （占位显示，避免空白闪烁）。
        if let Some(d) = state::read_current_data() {
            data = d;
        }

        // Init transcript reader if we have a path
        if transcript_reader.is_none() {
            if let Some(ref path) = data.transcript_path {
                transcript_reader = Some(TranscriptReader::new(PathBuf::from(path)));
            }
        }

        // Read transcript updates and push to all widgets
        if let Some(ref mut reader) = transcript_reader {
            let summary = reader.read_updates();
            // Push transcript summary to all widgets that accept it
            for widget in &registry.widgets {
                widget.update_transcript(&summary);
            }
        }

        // Check for notification triggers
        check_alerts(&data, &last_agent_count);

        terminal
            .draw(|frame| {
                draw_dashboard(frame, registry, &data, theme, config);
            })
            .map_err(|e| format!("draw: {}", e))?;

        if event::poll(tick_rate).map_err(|e| format!("poll: {}", e))? {
            if let Event::Key(key) = event::read().map_err(|e| format!("read event: {}", e))? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        // Record session before exit
                        if let Some(ref h) = history {
                            let _ = h.record_session(&data, last_agent_count, &config.active_mod);
                        }
                        return Ok(());
                    }
                    KeyCode::Char('1'..='9') => {
                        // Tab switching between dashboard layouts (future)
                    }
                    _ => {}
                }
            }
        }
    }
}
```

- 顶部 import 区（第 14-19 行）追加：

```rust
use crate::core::state::{self, StateFile};
```

（`TranscriptReader` import 已在第 18 行。）

`src/serve.rs`：

- 删除 `use std::io::Read;`（第 1 行）与本地 `read_current_data`（第 228-233 行）。
- `build_api_json` 第 60 行改为：

```rust
    let data = state::read_current_data().unwrap_or_default();
```

- 顶部 import 区（第 4-7 行）追加：

```rust
use crate::core::state;
```

- [ ] **Step 2: 编译 + 全量测试**

Run: `cargo test`
Expected: 全绿（既有 89 用例不受影响；dashboard/serve 改动是等价替换 + 新数据源）。

- [ ] **Step 3: 黑盒回归 D6 / D7**

Run: `python scripts/test_hud.py --case D6-02` 与 `python scripts/test_hud.py --case D7-01`
Expected: D6-02 PASS（serve 非 TTY → 占位 JSON）；D7-01 PASS（dashboard 非 TTY → TUI 存活 10s）。

- [ ] **Step 4: 手动验证 TTY 路径（q 可退出）**

Run: 在真实终端执行 `cargo run -- dashboard`（或 `target/debug/claude-hud.exe dashboard`）
Expected: 启动不卡死、显示占位或实时数据；按 `q` 立即退出返回 shell（修复前在此场景卡死）。
若终端有活跃 Claude Code 会话（state.json 新鲜），应显示真实会话数据。

- [ ] **Step 5: 提交（用户手动执行）**

```bash
git add src/dashboard.rs src/serve.rs
git commit -m "fix: dashboard/serve 按 IsTerminal 分发读 state 快照，TTY 不再卡死（任务①）"
```

### Task 3: TranscriptReader 跨进程累计重构

**Files:**
- Modify: `src/core/transcript.rs`
- Test: `src/core/transcript.rs` 内 `#[cfg(test)]`

- [ ] **Step 1: 写失败测试**

`src/core/transcript.rs` 文件末尾（`impl TranscriptSummary` 结束之后）追加测试模块。测试依赖 fixtures（`fixtures/transcript/agents.jsonl`，仓库已有）：

```rust
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
```

- [ ] **Step 2: 跑测试验证失败**

Run: `cargo test transcript`
Expected: 编译错误（`to_state` / `from_state` 不存在）——RED。

- [ ] **Step 3: 实现重构**

`src/core/transcript.rs` 全文替换（保留第 1-143 行的类型定义与 `TranscriptEntry` 等，改动从第 145 行 `TranscriptReader` 开始；下面给出完整新代码，含 serde 派生、`TranscriptSegment` 完整定义与记录类型 Serialize）：

记录类型补 derive（原 `#[derive(Debug, Clone)]` 处）：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
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
```

`TranscriptReader` 结构（替换原第 145-159 行）：

```rust
/// Incremental transcript reader with cross-process cumulative state.
pub struct TranscriptReader {
    path: PathBuf,
    last_pos: u64,
    base_time_secs: Option<u64>,
    agents: HashMap<String, AgentRecord>,
    skills: HashMap<String, SkillCall>,
    mcps: HashMap<String, McpCall>,
    tool_counts: HashMap<String, usize>,
    total_tokens: TokenTotal,
    token_timeline: Vec<TokenSnapshot>,
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
}
```

`impl TranscriptReader` 全文替换（原第 152-314 行）：

```rust
impl TranscriptReader {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            last_pos: 0,
            base_time_secs: None,
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
        reader.base_time_secs = Some(0);
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
        TranscriptSegment {
            path: self.path.to_string_lossy().into_owned(),
            last_pos: self.last_pos,
            agents: self.agents.values().cloned().collect(),
            skill_calls: self.skills.values().cloned().collect(),
            mcp_calls: self.mcps.values().cloned().collect(),
            tool_counts: self.tool_counts.clone(),
            total_tokens: self.total_tokens.clone(),
            token_timeline: self.token_timeline.clone(),
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
        }
        if file_len <= self.last_pos {
            return self.cumulative_summary(); // No new data
        }

        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(self.last_pos)).is_err() {
            return self.cumulative_summary();
        }

        // Set base time from first entry timestamp if not set
        if self.base_time_secs.is_none() {
            self.base_time_secs = Some(0); // epoch-relative
        }

        let mut line = String::new();
        let mut current_secs = self
            .token_timeline
            .last()
            .map(|t| t.timestamp_secs)
            .unwrap_or(0);

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

                        // Update agent last-tool-call timestamp
                        // (We detect agents from subagent_start events, track their tool activity here)
                    }
                    TranscriptEntry::SubagentStart(sub) => {
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
                    TranscriptEntry::SubagentStop { name } => {
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
                        // Snapshot per ~60s bucket
                        if self.token_timeline.is_empty()
                            || current_secs
                                - self.token_timeline.last().unwrap().timestamp_secs
                                >= 60
                        {
                            self.token_timeline.push(TokenSnapshot {
                                timestamp_secs: current_secs,
                                input_tokens: self.total_tokens.input,
                                output_tokens: self.total_tokens.output,
                                total_tokens: self.total_tokens.input + self.total_tokens.output,
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

        self.cumulative_summary()
    }
}
```

注意：`use serde::Deserialize;`（第 1 行）改为 `use serde::{Deserialize, Serialize};`。

- [ ] **Step 4: 跑测试验证通过**

Run: `cargo test transcript`
Expected: `test result: ok. 3 passed`（新 3 个测试全绿），既有测试不受影响。

- [ ] **Step 5: 提交（用户手动执行）**

```bash
git add src/core/transcript.rs
git commit -m "refactor: TranscriptReader 累计状态 + from_state/to_state 跨进程恢复（任务②⑧）"
```

### Task 4: render 管线整合（compact.rs）

**Files:**
- Modify: `src/compact.rs`
- Test: `src/compact.rs` 内 `#[cfg(test)]`（`should_restore` 纯函数）

- [ ] **Step 1: 写失败测试**

`src/compact.rs` 文件末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_restore_matches_same_path() {
        assert!(should_restore("a/b.jsonl", Some("a/b.jsonl")));
        assert!(!should_restore("", Some("a/b.jsonl")));      // 无持久化状态
        assert!(!should_restore("a/b.jsonl", Some("c/d.jsonl"))); // path 变化
        assert!(!should_restore("a/b.jsonl", None));          // 本次无 transcript
    }
}
```

- [ ] **Step 2: 跑测试验证失败**

Run: `cargo test compact`
Expected: 编译错误（`should_restore` 未定义）——RED。

- [ ] **Step 3: 实现管线**

`src/compact.rs` 全文替换：

```rust
use std::path::PathBuf;

use crate::core::config::AppConfig;
use crate::core::session::SessionData;
use crate::core::state::{self, SnapshotSegment, StateFile};
use crate::core::theme::Theme;
use crate::core::transcript::TranscriptReader;
use crate::core::widget::WidgetRegistry;

/// Render the compact status bar from stdin JSON data.
pub fn render(
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> Result<String, String> {
    let stdin_data = read_stdin()?;
    let data = SessionData::from_stdin_json(&stdin_data)
        .map_err(|e| format!("parse stdin JSON: {}", e))?;
    run_pipeline(&data, registry, config, theme)
}

/// The 5s render pipeline: restore state → transcript → git/scripts →
/// render → persist. Returns the rendered status line.
fn run_pipeline(
    data: &SessionData,
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> Result<String, String> {
    let state_path = AppConfig::state_path()?;
    let mut state = StateFile::read(&state_path);

    // Transcript: restore cumulative state only when the path is unchanged
    // (双窗口/新会话天然隔离，path 变化即全新 reader)。
    let mut reader = if should_restore(&state.transcript.path, data.transcript_path.as_deref()) {
        TranscriptReader::from_state(&state.transcript)
    } else {
        match &data.transcript_path {
            Some(p) => TranscriptReader::new(PathBuf::from(p)),
            None => TranscriptReader::new(PathBuf::new()),
        }
    };
    let summary = reader.read_updates();
    for widget in &registry.widgets {
        widget.update_transcript(&summary);
    }

    let output = render_with_data(data, registry, config, theme)?;

    // 持久化（best-effort：写失败不中断状态栏，仅 stderr 警告）。
    // 脚本/git widget 可能在管线中途写了 cache 窄键 → 先合并磁盘 cache。
    let now = state::now_secs();
    state.snapshot = SnapshotSegment::from_session(data, now);
    state.transcript = reader.to_state();
    state.last_error = None;
    state.merge_cache_from_disk(&state_path);
    if let Err(e) = state.write(&state_path) {
        eprintln!("[claude-hud] warning: state write failed: {}", e);
    }

    Ok(output)
}

/// True when the persisted transcript segment matches the current stdin
/// path, i.e. the reader should resume from the persisted offset instead of
/// re-parsing the whole file.
pub fn should_restore(state_path: &str, data_path: Option<&str>) -> bool {
    !state_path.is_empty() && data_path == Some(state_path)
}

/// Build the stdout error marker for render failures. The message is
/// truncated so the marker stays readable in a terminal status line.
pub fn hud_err_marker(msg: &str) -> String {
    let short: String = msg.chars().take(80).collect();
    format!("[hud err] {} — run 'claude-hud doctor'", short)
}

/// Render the compact status bar from an already-parsed session snapshot.
/// Shared by `render` (stdin) and `doctor` (sample data). No transcript
/// parsing here — that lives in `run_pipeline`.
pub fn render_with_data(
    data: &SessionData,
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> Result<String, String> {
    let layout = &config.compact_layout;
    if layout.is_empty() {
        return Ok(String::new());
    }

    let lines = config
        .runtime_overrides
        .as_ref()
        .and_then(|o| o.compact_lines)
        .unwrap_or(theme.compact_lines) as usize;

    let sep = &config.separator;
    let widgets_per_line = if lines == 1 {
        layout.len()
    } else {
        (layout.len() + lines - 1) / lines
    };

    let mut output = String::new();
    for line_idx in 0..lines {
        let start = line_idx * widgets_per_line;
        let end = (start + widgets_per_line).min(layout.len());
        if start >= end {
            break;
        }
        let line_widgets: Vec<String> = layout[start..end]
            .iter()
            .filter_map(|id| {
                let w = registry.get(id)?;
                let widget_config = config.widget_config(id);
                let rendered = w.render_compact(data, theme, &widget_config);
                if rendered.is_empty() {
                    None
                } else {
                    Some(rendered)
                }
            })
            .collect();
        if !line_widgets.is_empty() {
            output.push_str(&line_widgets.join(sep));
            output.push('\n');
        }
    }

    Ok(output.trim_end().to_string())
}

fn read_stdin() -> Result<String, String> {
    use std::io::Read;
    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|e| format!("read stdin: {}", e))?;
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_restore_matches_same_path() {
        assert!(should_restore("a/b.jsonl", Some("a/b.jsonl")));
        assert!(!should_restore("", Some("a/b.jsonl")));      // 无持久化状态
        assert!(!should_restore("a/b.jsonl", Some("c/d.jsonl"))); // path 变化
        assert!(!should_restore("a/b.jsonl", None));          // 本次无 transcript
    }
}
```

（`hud_err_marker` 的测试在 Task 6 补齐；此处先实现，避免 Task 4 与 ⑬ 耦合。）

注意：`render_with_data` 中删除了原 `parse_and_push_transcript` 调用与函数（第 30 行、第 80-97 行）——transcript 处理已上移到 `run_pipeline`。`doctor::sample_render` 的调用不受影响（sample JSON 无 transcript_path）。

- [ ] **Step 4: 跑测试验证通过**

Run: `cargo test`
Expected: 全绿（新增 `should_restore` 1 个测试；D1-19/D1-20/D8 系列行为不变——transcript 缺失静默、损坏跳过）。

- [ ] **Step 5: 提交（用户手动执行）**

```bash
git add src/compact.rs
git commit -m "feat: render 管线整合 state 恢复/累计/持久化（任务②⑧）"
```

### Task 5: git TTL 缓存 + 脚本节流跨进程 + 时间相位动画

**Files:**
- Modify: `src/probe/git.rs`、`src/widgets/git_status.rs`、`src/widgets/script_widget.rs`、`src/widgets/alerts.rs`、`src/widgets/mod.rs`、`src/main.rs`
- Test: 三个模块内 `#[cfg(test)]`

- [ ] **Step 1: 写失败测试**

`src/probe/git.rs` 文件末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::{now_secs, CachedValue, GitCache, StateFile};
    use std::path::{Path, PathBuf};

    fn tmp_state() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("hud-git-cache-{}.json", std::process::id()));
        p
    }

    fn seed(p: &Path, branch: &str, dirty: bool, ab: &str, ts: u64) {
        let mut st = StateFile::default();
        st.cache.git = GitCache {
            branch: CachedValue { value: branch.into(), ts },
            dirty: CachedValue { value: dirty, ts },
            ahead_behind: CachedValue { value: ab.into(), ts },
        };
        st.write(p).unwrap();
    }

    #[test]
    fn fresh_cache_reused_without_spawning() {
        let p = tmp_state();
        let _ = std::fs::remove_file(&p);
        seed(&p, "fake-cached-branch", true, "3/1", now_secs());
        let s = probe_git_cached(&p).expect("cached branch non-empty");
        assert_eq!(s.branch, "fake-cached-branch");
        assert!(s.is_dirty);
        assert_eq!((s.ahead, s.behind), (3, 1));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn stale_cache_reprobes() {
        let p = tmp_state();
        let _ = std::fs::remove_file(&p);
        seed(&p, "fake-cached-branch", false, "0/0", 0); // ts 0 = 从未新鲜
        let s = probe_git_cached(&p).expect("real repo has a branch");
        assert_ne!(s.branch, "fake-cached-branch");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn cached_not_a_repo_returns_none() {
        let p = tmp_state();
        let _ = std::fs::remove_file(&p);
        seed(&p, "", false, "0/0", now_secs()); // 空 branch = 非 git 仓库（已缓存）
        assert!(probe_git_cached(&p).is_none());
        let _ = std::fs::remove_file(&p);
    }
}
```

`src/widgets/script_widget.rs` 文件末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_state() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("hud-throttle-{}.json", std::process::id()));
        p
    }

    #[test]
    fn cross_process_throttle_uses_state() {
        let p = tmp_state();
        let _ = std::fs::remove_file(&p);
        let theme = crate::core::theme::Theme::default();
        let data = crate::core::session::SessionData::default();
        let cfg = WidgetConfig::default();
        let key = "echo hi".to_string();

        // state 节流新鲜 → 不刷新（cached 为空 → 渲染空串）
        let widget = ScriptWidget::new_shell("echo hi".into(), 30, p.clone());
        crate::core::state::StateFile::update(&p, |st| {
            st.cache.script_throttle.insert(key.clone(), crate::core::state::now_secs());
        })
        .unwrap();
        assert_eq!(widget.render_compact(&data, &theme, &cfg), "");

        // state 节流过期 → 刷新并回写 last_run
        let widget2 = ScriptWidget::new_shell("echo hi".into(), 30, p.clone());
        crate::core::state::StateFile::update(&p, |st| {
            st.cache.script_throttle.insert(key.clone(), 0);
        })
        .unwrap();
        assert_eq!(widget2.render_compact(&data, &theme, &cfg), "hi");
        let st = crate::core::state::StateFile::read(&p);
        assert!(st.cache.script_throttle.get(&key).copied().unwrap_or(0) > 0);
        let _ = std::fs::remove_file(&p);
    }
}
```

`src/widgets/alerts.rs` 文件末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_phase_is_periodic() {
        assert!(time_phase(8) < 8);
        assert_eq!(time_phase(8), time_phase(8));
    }
}
```

- [ ] **Step 2: 跑测试验证失败**

Run: `cargo test git_status && cargo test script_widget && cargo test alerts`
Expected: 编译错误（`probe_git_cached` / 新构造器签名 / `time_phase` 不存在）——RED。

- [ ] **Step 3: 实现**

`src/probe/git.rs`：保留原 `GitStatus`、`probe_git`、`run_cmd`，在 `run_cmd` 之后追加：

```rust
use std::path::Path;

const TTL_BRANCH_DIRTY_SECS: u64 = 30;
const TTL_AHEAD_BEHIND_SECS: u64 = 60;

/// Probe git with a TTL cache: fresh cache values are reused without
/// spawning git. `branch` value "" is the cached "not a repo" sentinel.
pub fn probe_git_cached(state_path: &Path) -> Option<GitStatus> {
    let now = crate::core::state::now_secs();
    let mut st = crate::core::state::StateFile::read(state_path);
    let mut status = GitStatus {
        branch: String::new(),
        is_dirty: false,
        ahead: 0,
        behind: 0,
    };
    let mut changed = false;

    // branch + not-a-repo sentinel (30s)
    if now.saturating_sub(st.cache.git.branch.ts) <= TTL_BRANCH_DIRTY_SECS {
        if st.cache.git.branch.value.is_empty() {
            return None;
        }
        status.branch = st.cache.git.branch.value.clone();
    } else {
        let branch = run_cmd(&["branch", "--show-current"])
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        st.cache.git.branch = crate::core::state::CachedValue {
            value: branch.clone(),
            ts: now,
        };
        changed = true;
        if branch.is_empty() {
            let _ = st.write(state_path);
            return None;
        }
        status.branch = branch;
    }

    // dirty (30s)
    if now.saturating_sub(st.cache.git.dirty.ts) <= TTL_BRANCH_DIRTY_SECS {
        status.is_dirty = st.cache.git.dirty.value;
    } else {
        status.is_dirty = run_cmd(&["status", "--porcelain"])
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        st.cache.git.dirty = crate::core::state::CachedValue {
            value: status.is_dirty,
            ts: now,
        };
        changed = true;
    }

    // ahead/behind (60s)
    if now.saturating_sub(st.cache.git.ahead_behind.ts) <= TTL_AHEAD_BEHIND_SECS {
        let (a, b) = parse_ab(&st.cache.git.ahead_behind.value);
        status.ahead = a;
        status.behind = b;
    } else {
        status.ahead = run_cmd(&["rev-list", "--count", "@{u}..HEAD"])
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        status.behind = run_cmd(&["rev-list", "--count", "HEAD..@{u}"])
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        st.cache.git.ahead_behind = crate::core::state::CachedValue {
            value: format!("{}/{}", status.ahead, status.behind),
            ts: now,
        };
        changed = true;
    }

    if changed {
        let _ = st.write(state_path);
    }
    Some(status)
}

fn parse_ab(s: &str) -> (usize, usize) {
    let mut it = s.splitn(2, '/');
    let a = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let b = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    (a, b)
}
```

`src/widgets/git_status.rs`：

- 第 13-15 行结构体加字段，构造器带参：

```rust
pub struct GitStatusWidget {
    cached: Mutex<Option<GitStatus>>,
    state_path: std::path::PathBuf,
}

impl GitStatusWidget {
    pub fn new(state_path: std::path::PathBuf) -> Self {
        Self { cached: Mutex::new(None), state_path }
    }
}
```

- 第 48-53 行 `render_compact` 改为：

```rust
    fn render_compact(&self, _data: &SessionData, theme: &Theme, _config: &WidgetConfig) -> String {
        let status = crate::probe::git::probe_git_cached(&self.state_path);
        let output = render_git_status(status.as_ref(), theme);
        if let Ok(ref mut guard) = self.cached.lock() { **guard = status; }
        output
    }
```

`src/widgets/mod.rs`：

- `register_all` 签名与调用（第 19 行、第 27 行）：

```rust
pub fn register_all(registry: &mut WidgetRegistry, config: &crate::core::config::AppConfig) {
    ...
    registry.register(Box::new(git_status::GitStatusWidget::new(
        crate::core::config::AppConfig::state_path().unwrap_or_default(),
    )));
```

- `register_script_widgets` 内（`for (_name, value) in &config.widgets` 之前）取一次 state_path，三个构造器加参：

```rust
    let state_path = crate::core::config::AppConfig::state_path().unwrap_or_default();
    for (_name, value) in &config.widgets {
        ...
                "rhai_script" => {
                    if let Some(path) = table.get("script_path").and_then(|v| v.as_str()) {
                        registry.register(Box::new(
                            script_widget::ScriptWidget::new_rhai(path.to_string(), state_path.clone()),
                        ));
                    }
                }
                "shell_output" => {
                    ...
                        registry.register(Box::new(
                            script_widget::ScriptWidget::new_shell(cmd.to_string(), refresh, state_path.clone()),
                        ));
                }
                "http_poll" => {
                    ...
                        registry.register(Box::new(
                            script_widget::ScriptWidget::new_http(url.to_string(), refresh, state_path.clone()),
                        ));
                }
```

`src/main.rs` 第 113 行：

```rust
    widgets::register_all(&mut registry, &config);
```

`src/widgets/script_widget.rs` 全文替换：

```rust
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Text};
use ratatui::widgets::Paragraph;

use crate::core::ansi;
use crate::core::scripting::{ScriptEngine, http_poll, run_shell_command};
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::widget::{Widget, WidgetConfig};

pub enum ScriptWidgetType {
    Rhai { script_path: String, engine: ScriptEngine },
    Shell { command: String, refresh_secs: u64 },
    Http { url: String, refresh_secs: u64 },
}

pub struct ScriptWidget {
    widget_type: ScriptWidgetType,
    cached_output: Mutex<String>,
    last_refresh: Mutex<Option<Instant>>,
    state_path: PathBuf,
}

impl ScriptWidget {
    pub fn new_rhai(script_path: String, state_path: PathBuf) -> Self {
        Self { widget_type: ScriptWidgetType::Rhai { script_path, engine: ScriptEngine::new() }, cached_output: Mutex::new(String::new()), last_refresh: Mutex::new(None), state_path }
    }
    pub fn new_shell(command: String, refresh_secs: u64, state_path: PathBuf) -> Self {
        Self { widget_type: ScriptWidgetType::Shell { command, refresh_secs }, cached_output: Mutex::new(String::new()), last_refresh: Mutex::new(None), state_path }
    }
    pub fn new_http(url: String, refresh_secs: u64, state_path: PathBuf) -> Self {
        Self { widget_type: ScriptWidgetType::Http { url, refresh_secs }, cached_output: Mutex::new(String::new()), last_refresh: Mutex::new(None), state_path }
    }

    /// Throttle key: the command/url/script path (unique per instance).
    fn throttle_key(&self) -> String {
        self.display_name().to_string()
    }

    fn should_refresh(&self) -> bool {
        let secs = match &self.widget_type {
            ScriptWidgetType::Shell { refresh_secs, .. } | ScriptWidgetType::Http { refresh_secs, .. } => *refresh_secs,
            ScriptWidgetType::Rhai { .. } => 5,
        };
        let in_process_fresh = self
            .last_refresh
            .lock()
            .ok()
            .map_or(false, |t| t.map_or(false, |t| t.elapsed().as_secs() < secs));
        if in_process_fresh {
            return false;
        }
        // 跨进程：last_run 持久化在 state.cache.script_throttle
        let now = crate::core::state::now_secs();
        let last_run = crate::core::state::StateFile::read(&self.state_path)
            .cache
            .script_throttle
            .get(&self.throttle_key())
            .copied()
            .unwrap_or(0);
        now.saturating_sub(last_run) >= secs
    }

    fn refresh_output(&self, data: &SessionData, theme: &Theme) {
        let output = match &self.widget_type {
            ScriptWidgetType::Rhai { script_path, engine } => engine.run_widget_script(script_path, data, theme).unwrap_or_else(|e| format!("rhai: {}", e)),
            ScriptWidgetType::Shell { command, .. } => run_shell_command(command).unwrap_or_else(|e| format!("shell: {}", e)),
            ScriptWidgetType::Http { url, .. } => http_poll(url).unwrap_or_else(|e| format!("http: {}", e)),
        };
        if let Ok(ref mut guard) = self.cached_output.lock() { **guard = output; }
        if let Ok(ref mut guard) = self.last_refresh.lock() { **guard = Some(Instant::now()); }
        // 回写跨进程节流时间戳（窄键 read-modify-write，失败静默）
        let now = crate::core::state::now_secs();
        let key = self.throttle_key();
        let _ = crate::core::state::StateFile::update(&self.state_path, |st| {
            st.cache.script_throttle.insert(key, now);
        });
    }
}

impl Widget for ScriptWidget {
    fn id(&self) -> &str {
        match &self.widget_type {
            ScriptWidgetType::Rhai { .. } => "script_rhai",
            ScriptWidgetType::Shell { .. } => "script_shell",
            ScriptWidgetType::Http { .. } => "script_http",
        }
    }
    fn display_name(&self) -> &str {
        match &self.widget_type {
            ScriptWidgetType::Rhai { script_path, .. } => script_path.as_str(),
            ScriptWidgetType::Shell { command, .. } => command.as_str(),
            ScriptWidgetType::Http { url, .. } => url.as_str(),
        }
    }

    fn render_compact(&self, data: &SessionData, theme: &Theme, _config: &WidgetConfig) -> String {
        if self.should_refresh() { self.refresh_output(data, theme); }
        self.cached_output.lock().ok().map_or(String::new(), |g| g.lines().next().unwrap_or("").to_string())
    }

    fn render_dashboard(&self, data: &SessionData, area: Rect, frame: &mut Frame, theme: &Theme, _config: &WidgetConfig) {
        if self.should_refresh() { self.refresh_output(data, theme); }
        if let Ok(ref guard) = self.cached_output.lock() {
            let lines: Vec<Line> = guard.lines().map(|l| Line::from(l.to_string())).collect();
            frame.render_widget(Paragraph::new(Text::from(lines)), area);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_state() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("hud-throttle-{}.json", std::process::id()));
        p
    }

    #[test]
    fn cross_process_throttle_uses_state() {
        let p = tmp_state();
        let _ = std::fs::remove_file(&p);
        let theme = crate::core::theme::Theme::default();
        let data = crate::core::session::SessionData::default();
        let cfg = WidgetConfig::default();
        let key = "echo hi".to_string();

        // state 节流新鲜 → 不刷新（cached 为空 → 渲染空串）
        let widget = ScriptWidget::new_shell("echo hi".into(), 30, p.clone());
        crate::core::state::StateFile::update(&p, |st| {
            st.cache.script_throttle.insert(key.clone(), crate::core::state::now_secs());
        })
        .unwrap();
        assert_eq!(widget.render_compact(&data, &theme, &cfg), "");

        // state 节流过期 → 刷新并回写 last_run
        let widget2 = ScriptWidget::new_shell("echo hi".into(), 30, p.clone());
        crate::core::state::StateFile::update(&p, |st| {
            st.cache.script_throttle.insert(key.clone(), 0);
        })
        .unwrap();
        assert_eq!(widget2.render_compact(&data, &theme, &cfg), "hi");
        let st = crate::core::state::StateFile::read(&p);
        assert!(st.cache.script_throttle.get(&key).copied().unwrap_or(0) > 0);
        let _ = std::fs::remove_file(&p);
    }
}
```

`src/widgets/alerts.rs`：`render_compact` 的时间相位动画改造 + 删掉 anim 字段：

- 第 16-25 行结构体与构造器：

```rust
pub struct Alerts {
    summary: Mutex<Option<TranscriptSummary>>,
}

impl Alerts {
    pub fn new() -> Self {
        Self { summary: Mutex::new(None) }
    }
}
```

- 删除第 3 行 `use crate::core::animation::AnimationState;`。
- `render_compact` 开头（第 32-35 行）替换为：

```rust
    fn render_compact(&self, data: &SessionData, theme: &Theme, config: &WidgetConfig) -> String {
        let mut alerts = vec![];
        let pct = data.context_window.used_percentage;
        let critical = config.get_f64("context_critical", 95.0);
        let warn = config.get_f64("context_warn", 80.0);
        let cost_warn = config.get_f64("cost_warn_usd", 10.0);

        if pct >= critical {
            let color = if time_phase(8) < 4 { &theme.danger } else { &theme.warning };
            alerts.push(ansi::ansi_fg(&format!("⚠ ctx {:.0}%", pct), color));
```

- 文件末尾（`update_transcript` 之后）追加：

```rust
/// Seconds-based phase so the breathing animation survives across 5s
/// render processes (per-process frame counters would freeze the phase).
fn time_phase(period: u64) -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() % period)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_phase_is_periodic() {
        assert!(time_phase(8) < 8);
        assert_eq!(time_phase(8), time_phase(8));
    }
}
```

- [ ] **Step 4: 跑测试验证通过**

Run: `cargo test`
Expected: 全绿（新增 git_status 3 + script_widget 1 + alerts 1 = 5 个测试；`should_refresh` 既有的 D2/D3 黑盒不受影响）。

- [ ] **Step 5: 提交（用户手动执行）**

```bash
git add src/probe/git.rs src/widgets/git_status.rs src/widgets/script_widget.rs src/widgets/alerts.rs src/widgets/mod.rs src/main.rs
git commit -m "feat: git TTL 缓存 + 脚本节流跨进程 + 时间相位动画（任务②⑧）"
```

### Task 6: [hud err] 标记 + last_error 落盘 + doctor 检查项（⑬）

**Files:**
- Modify: `Cargo.toml`（+ chrono）、`src/core/state.rs`（`write_last_error` + 测试）、`src/main.rs`（render 错误分支）、`src/doctor.rs`（2 个新检查项）
- Test: `src/core/state.rs` / `src/compact.rs` 内 `#[cfg(test)]`（`hud_err_marker` 已在 Task 4 实现，本任务补齐其测试）

- [ ] **Step 1: Cargo.toml 加 chrono + 写失败测试**

`Cargo.toml` 的 `[dependencies]` 段追加一行（保持字母序）：

```toml
chrono = "0.4"
```

`src/core/state.rs` 测试块内（`update_applies_read_modify_write` 之后）追加：

```rust
    #[test]
    fn write_last_error_round_trip() {
        let path = tmp_path("lasterr.json");
        cleanup(&path);
        StateFile::write_last_error(&path, "parse stdin JSON: boom");
        let st = StateFile::read(&path);
        let le = st.last_error.expect("last_error written");
        assert_eq!(le.msg, "parse stdin JSON: boom");
        assert!(!le.ts_iso.is_empty(), "ISO8601 timestamp set");
        assert!(le.ts_iso.starts_with("20"), "ts_iso looks like a date: {}", le.ts_iso);
        cleanup(&path);
    }
```

`src/compact.rs` 测试块内（`should_restore_matches_same_path` 之后）追加：

```rust
    #[test]
    fn hud_err_marker_short_and_truncated() {
        let short = hud_err_marker("parse stdin JSON: bad");
        assert!(short.starts_with("[hud err] parse stdin JSON: bad"));
        assert!(short.contains("claude-hud doctor"));

        let long_msg = "x".repeat(200);
        let long = hud_err_marker(&long_msg);
        assert_eq!(
            long,
            format!("[hud err] {} — run 'claude-hud doctor'", "x".repeat(80))
        );
    }
```

- [ ] **Step 2: 跑测试验证失败**

Run: `cargo test state::tests::write_last_error`
Expected: 编译错误（`write_last_error` 不存在）——RED。（`hud_err_marker` 测试此时已通过，因为 Task 4 已实现该函数。）

- [ ] **Step 3: 实现 `StateFile::write_last_error`**

`src/core/state.rs` 的 `impl StateFile` 内（`merge_cache_from_disk` 之后）追加：

```rust
    /// Record a render failure: ISO8601 timestamp + message. Best-effort —
    /// a failure to persist must never mask the original render error.
    pub fn write_last_error(path: &Path, msg: &str) {
        let mut st = StateFile::read(path);
        st.last_error = Some(LastError {
            ts_iso: chrono::Utc::now().to_rfc3339(),
            msg: msg.to_string(),
        });
        let _ = st.write(path);
    }
```

- [ ] **Step 4: main.rs render 错误分支（stdout 标记 + 落盘）**

`src/main.rs` 第 11-14 行 import 区把 Task 1 加入的 `use core::state::write_atomic;` 改为：

```rust
use core::state::{StateFile, write_atomic};
```

`Commands::Render` 分支（第 119-127 行）改为：

```rust
        Commands::Render => match compact::render(&registry, &config, &theme) {
            Ok(output) => {
                print!("{}", output);
                Ok(())
            }
            Err(e) => {
                // ⑬ 状态栏静默失效修复：错误写进 state.json（doctor 可查），
                // 同时在 stdout 打印可读标记（statusLine 输出原样上屏）。
                let state_path = AppConfig::state_path().unwrap_or_default();
                StateFile::write_last_error(&state_path, &e);
                println!("{}", compact::hud_err_marker(&e));
                Err(e)
            }
        },
```

- [ ] **Step 5: doctor.rs 新增 2 个检查项**

`src/doctor.rs` 顶部 import（第 3 行 `use crate::core::session::SessionData;` 之后）追加：

```rust
use crate::core::state::StateFile;
```

在 `run()` 的 "statusLine configured" 检查（第 33-38 行）之后、`icon set` 检查之前插入：

```rust
    let state_path = AppConfig::state_path();
    let state_ok = state_path
        .as_ref()
        .map(|p| !p.exists() || StateFile::read(p).snapshot.timestamp_secs != 0)
        .unwrap_or(true);
    failures += check(
        "state.json",
        state_ok,
        "exists and parses (missing = never rendered yet)",
        "run 'claude-hud render' once with real stdin JSON",
    );

    let last_err = state_path
        .as_ref()
        .and_then(|p| StateFile::read(p).last_error);
    failures += check(
        "last render",
        last_err.is_none(),
        "no recorded failure",
        "inspect state.json last_error, then run 'claude-hud render' to clear",
    );
    if let Some(le) = &last_err {
        println!("    last failure at {}: {}", le.ts_iso, le.msg);
    }
```

- [ ] **Step 6: 跑测试验证通过**

Run: `cargo test`
Expected: 全绿（新增 `write_last_error_round_trip` + `hud_err_marker_short_and_truncated` 2 个测试；D1-06..D1-11 失败用例现在 stdout 多一行 `[hud err]` 标记、state.json 落 last_error——黑盒断言只查 stderr/exit，不受影响）。

- [ ] **Step 7: 提交（用户手动执行）**

```bash
git add Cargo.toml Cargo.lock src/core/state.rs src/compact.rs src/main.rs src/doctor.rs
git commit -m "feat: [hud err] 标记 + last_error 落盘 + doctor 状态检查（任务⑬）"
```

---

### Task 7: 告警跨进程冷却 + 可配置阈值（⑫）

**Files:**
- Modify: `src/core/config.rs`（`AlertsConfig` + `AppConfig.alerts` 字段）、`src/alert.rs`（补全实现）、`src/compact.rs`（管线步骤⑦）、`src/dashboard.rs`（换用 alert.rs + 冷却）
- Test: `src/alert.rs` 4 个单测、`src/core/config.rs` 1 个默认值单测

- [ ] **Step 1: 写失败测试**

`src/alert.rs` 文件末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn session(pct: f64, cost: f64, rate: f64) -> SessionData {
        let json = format!(
            r#"{{"model":{{"id":"m","display_name":"m"}},
                "context_window":{{"used_percentage":{pct},"total_input_tokens":1,
                "context_window_size":100}},
                "cost":{{"total_cost_usd":{cost},"total_duration_ms":1}},
                "rate_limits":{{"five_hour":{{"used_percentage":{rate}}},
                "seven_day":{{"used_percentage":0}}}}}}"#
        );
        SessionData::from_stdin_json(&json).unwrap()
    }

    fn cfg() -> AlertsConfig {
        AlertsConfig {
            context_critical_pct: 95.0,
            cost_threshold_usd: 10.0,
            rate_limit_pct: 90.0,
            cooldown_minutes: 10,
        }
    }

    #[test]
    fn threshold_crossing_fires_once_per_cooldown() {
        let data = session(96.0, 12.0, 95.0);
        let mut cd = AlertCooldown::default();
        let fired = check_alerts(&data, &cfg(), &mut cd, 1000);
        assert_eq!(fired.len(), 3);
        // 冷却窗口内第二次调用：不再触发
        let again = check_alerts(&data, &cfg(), &mut cd, 1001);
        assert!(again.is_empty());
    }

    #[test]
    fn cooldown_expiry_refires() {
        let data = session(96.0, 0.0, 0.0);
        let mut cd = AlertCooldown::default();
        check_alerts(&data, &cfg(), &mut cd, 1000);
        // 窗口 600s：10 分钟前触发过 → 重新触发
        let fired = check_alerts(&data, &cfg(), &mut cd, 1601);
        assert!(fired.contains(&AlertKind::ContextCritical));
    }

    #[test]
    fn zero_threshold_disables_alert() {
        let mut c = cfg();
        c.context_critical_pct = 0.0;
        let data = session(100.0, 0.0, 0.0);
        let mut cd = AlertCooldown::default();
        assert!(check_alerts(&data, &c, &mut cd, 1).is_empty());
    }

    #[test]
    fn from_state_to_state_round_trip() {
        let mut map = HashMap::new();
        map.insert(AlertKind::CostThreshold, 42);
        let cd = AlertCooldown::from_state(&map);
        assert_eq!(cd.to_state().get(&AlertKind::CostThreshold), Some(&42));
    }
}
```

`src/core/config.rs` 文件末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alerts_defaults() {
        let a = AlertsConfig::default();
        assert_eq!(a.context_critical_pct, 95.0);
        assert_eq!(a.cost_threshold_usd, 10.0);
        assert_eq!(a.rate_limit_pct, 90.0);
        assert_eq!(a.cooldown_minutes, 10);
    }
}
```

- [ ] **Step 2: 跑测试验证失败**

Run: `cargo test alert`
Expected: 编译错误（`AlertCooldown` / `check_alerts` / `AlertsConfig` 不存在）——RED。

- [ ] **Step 3: config.rs 增加 `AlertsConfig`**

`src/core/config.rs` 在 `default_dash_layout()`（第 54 行）之后追加：

```rust
/// [alerts] section: thresholds (0 = disabled) and cooldown window.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AlertsConfig {
    #[serde(default = "default_ctx_critical")]
    pub context_critical_pct: f64,
    #[serde(default = "default_cost_threshold")]
    pub cost_threshold_usd: f64,
    #[serde(default = "default_rate_limit")]
    pub rate_limit_pct: f64,
    #[serde(default = "default_cooldown")]
    pub cooldown_minutes: u64,
}

fn default_ctx_critical() -> f64 { 95.0 }
fn default_cost_threshold() -> f64 { 10.0 }
fn default_rate_limit() -> f64 { 90.0 }
fn default_cooldown() -> u64 { 10 }

impl Default for AlertsConfig {
    fn default() -> Self {
        Self {
            context_critical_pct: 95.0,
            cost_threshold_usd: 10.0,
            rate_limit_pct: 90.0,
            cooldown_minutes: 10,
        }
    }
}
```

`AppConfig` 结构体（`runtime_overrides` 字段之后）追加字段：

```rust
    #[serde(default = "default_alerts")]
    pub alerts: AlertsConfig,
```

并在文件末尾追加：

```rust
fn default_alerts() -> AlertsConfig {
    AlertsConfig::default()
}
```

`impl Default for AppConfig`（第 191-211 行）末尾追加 `alerts: AlertsConfig::default(),`。

- [ ] **Step 4: alert.rs 补全实现**

`src/alert.rs` 在 `AlertKind` 枚举（Task 1 骨架）之后、测试块之前追加：

```rust
use std::collections::HashMap;

use crate::core::config::AlertsConfig;
use crate::core::session::SessionData;

/// Cross-process cooldown state. render 是唯一权威：从 state.json 加载、
/// 判定后回写；dashboard 只在启动时 seed 一次、运行期仅内存。
#[derive(Debug, Clone, Default)]
pub struct AlertCooldown {
    last_fired: HashMap<AlertKind, u64>,
}

impl AlertCooldown {
    /// Seed from persisted state (render) or the initial snapshot (dashboard).
    pub fn from_state(alerts: &HashMap<AlertKind, u64>) -> Self {
        Self { last_fired: alerts.clone() }
    }

    /// Persistable view of the cooldown map (state.json `alerts` segment).
    pub fn to_state(&self) -> HashMap<AlertKind, u64> {
        self.last_fired.clone()
    }
}

/// Pure threshold check + cooldown. Returns kinds that fired now (threshold
/// crossed AND cooldown expired); each returned kind is marked as fired in
/// `cooldown`, so the next call within the cooldown window returns nothing.
/// Threshold 0 = disabled. No OS side effects — trivially unit-testable.
pub fn check_alerts(
    data: &SessionData,
    cfg: &AlertsConfig,
    cooldown: &mut AlertCooldown,
    now: u64,
) -> Vec<AlertKind> {
    let mut fired = Vec::new();
    if cfg.context_critical_pct > 0.0
        && data.context_window.used_percentage >= cfg.context_critical_pct
    {
        fired.push(AlertKind::ContextCritical);
    }
    if cfg.cost_threshold_usd > 0.0 && data.cost.total_cost_usd >= cfg.cost_threshold_usd {
        fired.push(AlertKind::CostThreshold);
    }
    if cfg.rate_limit_pct > 0.0
        && data.rate_limits.five_hour.used_percentage >= cfg.rate_limit_pct
    {
        fired.push(AlertKind::RateLimit);
    }
    let window = cfg.cooldown_minutes.saturating_mul(60);
    fired.retain(|kind| {
        let last = cooldown.last_fired.get(kind).copied().unwrap_or(0);
        if now.saturating_sub(last) >= window {
            cooldown.last_fired.insert(*kind, now);
            true
        } else {
            false
        }
    });
    fired
}

/// Send OS notifications for fired alerts (best-effort; notify::send logs
/// its own failures and never panics).
pub fn send_notifications(fired: &[AlertKind], data: &SessionData, cfg: &AlertsConfig) {
    for kind in fired {
        match kind {
            AlertKind::ContextCritical => {
                crate::notify::context_critical(data.context_window.used_percentage)
            }
            AlertKind::CostThreshold => {
                crate::notify::cost_threshold(data.cost.total_cost_usd, cfg.cost_threshold_usd)
            }
            AlertKind::RateLimit => {
                crate::notify::rate_limit_warning(data.rate_limits.five_hour.used_percentage)
            }
        }
    }
}
```

注意：枚举上方已有的 `use serde::{Deserialize, Serialize};` 保留；新增 `use` 放在其下。

- [ ] **Step 5: compact.rs 管线步骤⑦ + dashboard.rs 换用**

`src/compact.rs` import 区（`use crate::core::widget::WidgetRegistry;` 之后）追加 `use crate::alert;`。`run_pipeline` 的持久化块（Task 4 版本）改为：

```rust
    // ⑦ 越阈告警：render 是跨进程冷却权威（加载 → 判定 → 回写 state.alerts）
    let now = state::now_secs();
    let mut cooldown = alert::AlertCooldown::from_state(&state.alerts);
    let fired = alert::check_alerts(&data, &config.alerts, &mut cooldown, now);
    alert::send_notifications(&fired, &data, &config.alerts);
    state.alerts = cooldown.to_state();

    // 持久化（best-effort：写失败不中断状态栏，仅 stderr 警告）。
    // 脚本/git widget 可能在管线中途写了 cache 窄键 → 先合并磁盘 cache。
    state.snapshot = SnapshotSegment::from_session(data, now);
```

（原 `let now = state::now_secs();` 上移进告警块。）

`src/dashboard.rs`：

- import 区追加 `use crate::alert;`。
- `run_loop` 中 `let initial = ...`（Task 2 已加）之后追加：

```rust
    // 告警冷却只 seed 一次，运行期仅内存（render 是跨进程权威）
    let mut cooldown = alert::AlertCooldown::from_state(&initial.alerts);
```

- 第 78 行 `check_alerts(&data, &last_agent_count);` 替换为：

```rust
        let fired = alert::check_alerts(&data, &config.alerts, &mut cooldown, state::now_secs());
        alert::send_notifications(&fired, &data, &config.alerts);
```

- 删除本地 `check_alerts` 函数（第 181-193 行，硬编码阈值、无冷却——这正是 ⑫ 的轰炸 bug）。

- [ ] **Step 6: 跑测试验证通过**

Run: `cargo test`
Expected: 全绿（alert 4 测试 + config 1 测试；dashboard 不再引用本地 check_alerts）。

- [ ] **Step 7: 提交（用户手动执行）**

```bash
git add src/alert.rs src/core/config.rs src/compact.rs src/dashboard.rs
git commit -m "feat: 告警阈值可配置 + 跨进程冷却（render 权威回写 state.alerts）（任务⑫）"
```

---

### Task 8: 黑盒用例 P1-01..P1-06 + harness 机制

**Files:**
- Modify: `scripts/hudlib/assertions.py`（`check_state_json` + `_dig`）、`scripts/hudlib/cases.py`（`render_case` 透传 `**extra`、P1 用例、CASES 89→95）、`scripts/test_hud.py`（`prepare_case` 重构、`pre_render` / fixture 增删 / `remove_state` / state_json 集成）

- [ ] **Step 1: assertions.py 加 `check_state_json`**

`scripts/hudlib/assertions.py` 顶部 import 区追加：

```python
import json
import os
```

文件末尾追加：

```python
_MISSING = object()


def _dig(node, path):
    """Dig a dot path; _MISSING when any segment is absent."""
    for part in path.split("."):
        if isinstance(node, dict) and part in node:
            node = node[part]
        else:
            return _MISSING
    return node


def check_state_json(spec: dict, state: dict) -> list[str]:
    """Evaluate a state_json spec against a parsed state dict.
    Spec keys: exists (default True), segments (all must be present),
    absent (must be missing or null), min (dot-path -> numeric floor),
    equals (dot-path -> exact value). Returns failure strings (empty = pass).
    """
    fails = []
    if not spec.get("exists", True):
        if state:
            fails.append("state file present but expected absent")
        return fails
    if not state:
        fails.append("state file missing or unparseable")
        return fails
    for seg in spec.get("segments", []):
        if seg not in state:
            fails.append(f"state segment missing: {seg}")
    for key in spec.get("absent", []):
        if _dig(state, key) not in (_MISSING, None):
            fails.append(f"state key present but expected absent: {key}")
    for key, want in (spec.get("min") or {}).items():
        got = _dig(state, key)
        if not isinstance(got, (int, float)) or got < want:
            fails.append(f"state.{key}: expected >= {want}, got {got!r}")
    for key, want in (spec.get("equals") or {}).items():
        got = _dig(state, key)
        if got != want:
            fails.append(f"state.{key}: expected {want!r}, got {got!r}")
    return fails
```

- [ ] **Step 2: cases.py 透传 + P1 用例 + CASES 95**

`render_case`（第 67-72 行）改为透传扩展字段：

```python
def render_case(cid, name, dim, spec, args=None, stdin=None, stdin_file=None,
                config=None, pre_cmds=None, note=None, **extra):
    case = {"id": cid, "name": name, "dim": dim, "args": args or ["render"],
            "stdin": stdin, "stdin_file": stdin_file, "config": config,
            "spec": spec, "run_kind": "render",
            "pre_cmds": pre_cmds or [], "note": note}
    case.update(extra)
    return case
```

模块 docstring 的 case 键清单追加：

```python
  pre_render (bool)       -- 主运行前先 render 一次（复用主 stdin，可用 pre_render_stdin 覆盖）
  pre_render_stdin (str)  -- pre_render 的独立 stdin（如坏 JSON）
  pre_exit (int)          -- pre_render 期望退出码
  pre_stdout_contains (list[str]) -- pre_render stdout 必须包含
  transcript_copy (str)   -- fixtures/transcript/<name> 复制到 tmp_dir 并把 stdin 的
                             transcript_path 指向副本（P1 用例专用，避免污染共享 fixture）
  grow_fixture (dict)     -- {"agent_pairs": N} 追加 N 对 subagent_start/stop 行
  truncate_fixture (dict) -- {"keep_lines": N} 只保留前 N 行
  remove_state (bool)     -- 主运行前删除 state.json
  state_json (dict)       -- check_state_json 断言（segments/absent/min/equals；
                             min/equals 的值可为 "<FIXTURE_SIZE>" 运行期替换）
  pre_state_json (dict)   -- 对 pre_render 之后的 state.json 做 check_state_json
  state_json_same_as_pre (list[str]) -- 这些点路径在主运行前后必须不变
```

`D8` 之后、`CASES` 之前追加：

```python
# ---------------------------------------------------------------------------
# P1: state.json 数据通路（第一期任务① ②⑧ ⑫ ⑬）
# ---------------------------------------------------------------------------
P1 = [
    render_case("P1-01", "render 创建五段 state.json", "P1",
                {"exit": 0, "stderr_empty": True,
                 "state_json": {
                     "segments": ["snapshot", "transcript", "cache",
                                  "alerts", "last_error"],
                     "absent": ["last_error"],
                     "equals": {"snapshot.model.display_name": "deepseek-v4-flash",
                                "transcript.last_pos": "<FIXTURE_SIZE>"},
                 }},
                stdin=j(full_dict(**{"context_window.used_percentage": 3})),
                config=DEFAULT_CONFIG, transcript_copy="agents.jsonl",
                note="任务①：render 全量原子写 state.json，快照与 transcript 游标落盘；修复前根本没有 state.json"),
    render_case("P1-02", "同文件两次 render 计数稳定", "P1",
                {"exit": 0,
                 "stdout_contains": ["2 agents"],
                 "stdout_not_contains": ["4 agents"],
                 "state_json_same_as_pre": ["transcript.last_pos"],
                 "state_json": {"equals": {"transcript.last_pos": "<FIXTURE_SIZE>"}}},
                stdin=j(full_dict()), config=DEFAULT_CONFIG,
                transcript_copy="agents.jsonl", pre_render=True,
                note="任务②：游标持久化 → 重复 render 只读新行，计数不翻倍；若实现只恢复游标不恢复计数 → '0 agents' 失败"),
    render_case("P1-03", "增量追加后累计续读", "P1",
                {"exit": 0,
                 "stdout_contains": ["4 agents"],
                 "stdout_not_contains": ["2 agents"],
                 "state_json": {"equals": {"transcript.last_pos": "<FIXTURE_SIZE>"}}},
                stdin=j(full_dict()), config=DEFAULT_CONFIG,
                transcript_copy="agents.jsonl", pre_render=True,
                grow_fixture={"agent_pairs": 2},
                note="任务②：游标续读 + 计数跨进程恢复 → 2 旧 + 2 新 = 4 agents"),
    render_case("P1-04", "截断文件自动重置游标", "P1",
                {"exit": 0,
                 "stdout_contains": ["1 agents"],
                 "stdout_not_contains": ["2 agents"],
                 "state_json": {"equals": {"transcript.last_pos": "<FIXTURE_SIZE>"}}},
                stdin=j(full_dict()), config=DEFAULT_CONFIG,
                transcript_copy="agents.jsonl", pre_render=True,
                truncate_fixture={"keep_lines": 4},
                note="任务⑧：last_pos > 文件长度 → 丢弃累计状态从 0 重读 → 1 agent；未重置会 0 agents + 游标卡死"),
    render_case("P1-05", "[hud err] 标记 + last_error 落盘与清除", "P1",
                {"exit": 0,
                 "state_json": {"absent": ["last_error"]}},
                stdin=j(full_dict()), config=DEFAULT_CONFIG,
                pre_render=True, pre_render_stdin="{ not json",
                pre_exit=1, pre_stdout_contains=["[hud err]"],
                note="任务⑬：坏 stdin → stdout 标记 + last_error 落盘；随后成功 render 清除 last_error"),
    render_case("P1-06", "doctor 上报 last render 失败", "P1",
                {"exit": 1, "stdout_contains": ["last render", "fix"]},
                args=["doctor"], config=DEFAULT_CONFIG,
                pre_render=True, pre_render_stdin="{ not json",
                note="任务⑬：doctor 读 state.json last_error 并给修复提示（计 1 个 failed check）"),
]

CASES = D1 + D2 + D3 + D4 + D5 + D6 + D7 + D8 + P1
assert len(CASES) == 95, f"expected 95 cases, got {len(CASES)}"
```

（原 `CASES = D1 + ... + D8` 与 `assert 89` 两行替换为上面两行。）

- [ ] **Step 3: test_hud.py 集成机制**

`prepare_case`（第 40-55 行）全文替换：

```python
def prepare_case(case, tmp_dir):
    """Return stdin text for render cases (None when no stdin).

    transcript_copy: copies fixtures/transcript/<name> once into tmp_dir and
    rewrites the stdin JSON's transcript_path to the copy, so P1 cases can
    grow/truncate their transcript without mutating the shared fixture.
    """
    if case.get("run_kind", "render") != "render" and not case.get("pre_render"):
        return None
    if case.get("transcript_copy") and not case.get("_transcript_copy_path"):
        src = cases.fx(os.path.join("transcript", case["transcript_copy"]))
        dst = os.path.join(tmp_dir, f"{case['id']}-transcript.jsonl")
        shutil.copyfile(src, dst)
        case["_transcript_copy_path"] = dst
    if case.get("stdin") is not None:
        text = case["stdin"]
    elif case.get("stdin_file"):
        with open(cases.fx(case["stdin_file"]), encoding="utf-8") as f:
            text = f.read()
    else:
        return None
    if case.get("_transcript_copy_path"):
        data = _json.loads(text)
        data["transcript_path"] = case["_transcript_copy_path"].replace("\\", "/")
        text = _json.dumps(data)
    if "<LARGE_FIXTURE>" in text:
        text = text.replace("<LARGE_FIXTURE>",
                            cases.prepare_large_transcript(tmp_dir))
    return text
```

`run_one`（第 161-212 行）中 `pre_warnings` 收集之后、`if case["run_kind"] == "serve":` 之前插入 P1 三机制：

```python
    # P1 机制 1：pre_render（默认复用主 stdin，可覆盖）；断言失败即判负
    pre_fails = []
    if case.get("pre_render"):
        pre_text = case.get("pre_render_stdin")
        if pre_text is None:
            pre_text = prepare_case(case, tmp_dir)
        else:
            pre_text = prepare_case({"stdin": pre_text}, tmp_dir)
        r = runner.run_exe(exe_path, ["render"], stdin_text=pre_text,
                           timeout_s=10)
        if case.get("pre_exit") is not None and r.exit_code != case["pre_exit"]:
            pre_fails.append(
                f"pre_render exit={r.exit_code}, expected {case['pre_exit']}"
            )
        for want in case.get("pre_stdout_contains", []):
            if want not in r.stdout:
                pre_fails.append(f"pre_render stdout missing {want!r}")
        case["_pre_state"] = _read_state_json()

    # P1 机制 2：fixture 增删（只在有 per-case transcript 副本时生效）
    _apply_fixture_ops(case)

    # P1 机制 3：可选清空 state.json（模拟全新状态）
    if case.get("remove_state"):
        sp = os.path.join(runner.HUD_DIR, "state.json")
        if os.path.isfile(sp):
            os.remove(sp)
```

`run_one` 中 `passed, detail = assertions.check(r, case["spec"])` 之后、`if pre_warnings:` 之前插入 state 断言：

```python
    state_fails = []
    if case.get("state_json"):
        state_fails = assertions.check_state_json(
            case["state_json"], _read_state_json()
        )
    if case.get("pre_state_json"):
        state_fails += assertions.check_state_json(
            case["pre_state_json"], case.get("_pre_state", {})
        )
    for dot in case.get("state_json_same_as_pre", []):
        cur = _dig_state(_read_state_json(), dot)
        pre = _dig_state(case.get("_pre_state", {}), dot)
        if cur != pre:
            state_fails.append(f"state.{dot}: pre={pre!r} now={cur!r}")
    extra = pre_fails + state_fails
    if extra:
        passed = False
        detail = ("; ".join(extra) + "; " + detail) if detail != "ok" else "; ".join(extra)
```

`run_one` 之后追加三个 helper：

```python
def _read_state_json() -> dict:
    """Current HUD_DIR/state.json as dict ({} when missing/unparseable)."""
    path = os.path.join(runner.HUD_DIR, "state.json")
    try:
        with open(path, encoding="utf-8") as f:
            return _json.load(f)
    except (OSError, ValueError):
        return {}


def _dig_state(state: dict, dot: str):
    """Dig a dot path in the state dict; None when missing."""
    node = state
    for part in dot.split("."):
        if isinstance(node, dict) and part in node:
            node = node[part]
        else:
            return None
    return node


def _apply_fixture_ops(case):
    """grow (append agent pairs) / truncate the per-case transcript copy,
    then substitute <FIXTURE_SIZE> in the state_json spec with the size
    after the ops (the expected last_pos for the next render)."""
    path = case.get("_transcript_copy_path")
    if not path:
        return
    pairs = (case.get("grow_fixture") or {}).get("agent_pairs", 0)
    if pairs:
        with open(path, "a", encoding="utf-8") as f:
            for i in range(pairs):
                f.write(f'{{"type":"subagent_start","name":"extra-{i}","model":"m","task":"t"}}\n')
                f.write(f'{{"type":"subagent_stop","name":"extra-{i}"}}\n')
    keep = (case.get("truncate_fixture") or {}).get("keep_lines")
    if keep is not None:
        with open(path, encoding="utf-8") as f:
            lines = f.readlines()
        with open(path, "w", encoding="utf-8") as f:
            f.writelines(lines[:keep])
    if case.get("state_json"):
        size = os.path.getsize(path)
        for sect in ("equals", "min"):
            for key, val in (case["state_json"].get(sect) or {}).items():
                if val == "<FIXTURE_SIZE>":
                    case["state_json"][sect][key] = size
```

- [ ] **Step 4: 跑全套黑盒**

Run: `python scripts/test_hud.py`
Expected: `95/95 passed`（P1-01..P1-06 全绿；D1-D8 存量 89 个不回归）。

- [ ] **Step 5: 提交（用户手动执行）**

```bash
git add scripts/hudlib/assertions.py scripts/hudlib/cases.py scripts/test_hud.py
git commit -m "test: state.json 数据通路黑盒用例 P1-01..P1-06（89→95）"
```

---

### Task 9: 全量验证 + COMPLETE.md 状态更新

- [ ] **Step 1: 单元测试全量**

Run: `cargo test`
Expected: 全绿（state/compact/transcript/git/script/alerts/config 各模块测试）。

- [ ] **Step 2: 黑盒套件全量**

Run: `python scripts/test_hud.py`
Expected: `95/95 passed`。

- [ ] **Step 3: 手动验证 dashboard（适配说明 3）**

在真实终端执行 `claude-hud dashboard`：
- TUI 正常渲染（不卡死在 stdin 读取——TTY 下走 state.json 快照）；
- 按 `q` 立即退出，退出码 0；
- 停 30s 以上再开 dashboard：快照过期（SNAPSHOT_MAX_AGE_SECS=30）→ 显示占位空数据而非旧数据。

- [ ] **Step 4: doctor 全绿**

Run: `claude-hud doctor`
Expected: `All checks passed.`（含新增 `state.json` / `last render` 两行 [ok]）。

- [ ] **Step 5: COMPLETE.md 第 20 章状态更新**

`COMPLETE.md` 第 632 行（✅ 完整实现段落）追加：

```markdown
· 第一期数据通路：state.json 五段共享层（render 全量原子写）· dashboard/serve IsTerminal 分发 · Transcript 跨进程累计恢复 · git/脚本 TTL 缓存 · 告警跨进程冷却（[alerts] 可配置阈值）· [hud err] 状态栏错误标记 + doctor state 检查
```

第 21 章路线图表（第 657-663 行）追加一行：

```markdown
| Phase 1.5 数据通路 | state.json 共享层 + 跨进程累计 + 告警冷却 + 错误标记（TASKS ① ②⑧ ⑫ ⑬） | ✅ |
```

- [ ] **Step 6: 提交（用户手动执行）**

```bash
git add COMPLETE.md
git commit -m "docs: 第一期数据通路实现状态更新（① ②⑧ ⑫ ⑬ → ✅）"
```

---

## 自检（writing-plans：spec 覆盖 / 占位符 / 类型一致性）

- **Spec 覆盖**：§1 目标 → Task 1-9；§2 单文件方案 → Task 1；§3 五段结构 → Task 1/3；§4 render 管线（恢复→累计→git/脚本 TTL→告警→渲染→持久化）→ Task 4/5/7；§5 dashboard/serve IsTerminal 分发 → Task 2；⑬ 错误标记 + doctor → Task 6；§7 测试计划 → Task 1-4 单元测试 + P1-01..P1-06（用例 1/2/3/4/6；用例 5 = D7-01 + Task 9 手动；用例 7 = 存量回归）；§8 并发规则（render 唯一全量写、merge_cache_from_disk 防覆盖）→ Task 1/4/5；§9 验收 → Task 9。适配说明 1/2/3 均有对应落地（Task 5 / Task 4 / Task 2+9）。
- **占位符扫描**：全部步骤含完整代码与期望输出，无 TBD/TODO/"实现错误处理"类空指令。
- **类型一致性**：`StateFile::read/write/update/merge_cache_from_disk/write_last_error`、`AppConfig::state_path()`、`TranscriptReader::from_state/to_state`、`probe_git_cached(&Path)`、`check_alerts(&SessionData, &AlertsConfig, &mut AlertCooldown, u64) -> Vec<AlertKind>`、`AlertCooldown::from_state/to_state`、`hud_err_marker(&str) -> String` 在各任务间签名一致；state.json 键名（`alerts.context_critical`）与 `AlertKind` 的 `serde(rename_all = "snake_case")` 一致。

---

## 执行交接

计划完成，保存在 `docs/superpowers/plans/2026-07-31-phase1-data-path.md`。两种执行方式：

1. **Subagent-Driven（推荐）** —— 每个任务派发一个全新 subagent，任务间我做两阶段审查，迭代快
2. **Inline Execution** —— 在本会话用 executing-plans 批量执行，带检查点

选哪种？

