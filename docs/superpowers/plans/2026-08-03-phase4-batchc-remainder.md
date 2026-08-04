# 批次 C 剩余（⑨⑩⑪⑮⑯⑰⑱）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付 TASKS.md 批次 C 剩余 7 项：⑨历史库消费、⑩Shell Widget Windows 分支+删死代码、⑪占位功能收尾、⑮compact 宽度感知、⑯dashboard 交互、⑰安装占位符/时间戳备份/全局提示、⑱升级通路。

**Architecture:** 每任务独立可提交。数据层在 `state.rs`（SnapshotSegment.agent_count 结账）+ `compact.rs run_pipeline`（路径切换结账钩子）；CLI 层新增 `history` 与 `update check` 子命令（update 逻辑抽到 `src/core/update.rs`，doctor/CLI 复用）；渲染层在 `compact.rs` 加宽度感知（COLUMNS + fit_line）；TUI 层在 `dashboard.rs` 加布局循环/持久化/帮助/通知接线；安装层改 `install.sh/ps1`（占位符检测 + 三态输出）与 `setup`（时间戳备份）。黑盒 harness 扩展 `pre_cmds` 支持 stdin、新增 `remove_db` 标志与 `env_extra`/`stdout_visible_width_max` 断言。

**Tech Stack:** Rust（clap 4 派生、serde、toml 0.8、ratatui、rusqlite、ureq 2）；新依赖 `clap_complete = "4"`、`unicode-width = "0.2"`；测试：cargo 单元测试 + `scripts/test_hud.py` 黑盒（用例 123 → **130**）。

---

## 执行约定（沿用前批次）

- **git**：禁止自动 `git add/commit/push`。每个任务的提交步骤把命令交给用户手动执行。
- **绝不运行 `cargo fmt`**。
- cargo 不在 PATH：每个 cargo 命令前加 `export PATH="$HOME/.cargo/bin:$PATH" &&`。
- 黑盒全量：`python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`；单用例：加 `--case P4-01`。
- 需求不明确时列出澄清问题，禁止猜测。

## 文件总览

| 文件 | 动作 | 归属 |
|------|------|------|
| `src/core/state.rs` | 改：SnapshotSegment 增 agent_count + from_session 填充 | ⑨ |
| `src/compact.rs` | 改：should_checkout + run_pipeline 结账；⑮ columns_from/columns_env/fit_line + render_with_data 应用 | ⑨⑮ |
| `src/main.rs` | 改：History 子命令、Update 子命令、completion 真实现、setup 备份重写、uninstall 提示、全局生效提示 | ⑨⑪⑰⑱ |
| `src/serve.rs` | 改：/api/data 增 weekly + HTML This Week 卡片 | ⑨ |
| `src/core/scripting.rs` | 改：run_shell_command 平台分支 | ⑩ |
| `src/probe/system.rs` | 删：整个文件 | ⑩ |
| `src/probe/mod.rs` | 改：删 `pub mod system;` | ⑩ |
| `src/dashboard.rs` | 改：next_layout/agents_edge/通知接线/l/?/footer/持久化 | ⑪⑯ |
| `src/core/ansi.rs` | 改：strip_ansi | ⑮ |
| `src/widgets/model_display.rs` | 改：display_name truncate 24 | ⑮ |
| `src/widgets/git_status.rs` | 改：branch truncate 24 | ⑮ |
| `src/widgets/agent_detail.rs` | 改：agent name truncate 24 | ⑮ |
| `src/core/cc_config.rs` | 改：has_status_line | ⑰ |
| `src/core/update.rs` | 建：update check 逻辑 + cmp_versions | ⑱ |
| `src/core/mod.rs` | 改：`pub mod update;` | ⑱ |
| `src/doctor.rs` | 改：update 信息项 | ⑱ |
| `Cargo.toml` | 改：clap_complete、unicode-width | ⑪⑮ |
| `scripts/install.sh` / `install.ps1` | 改：占位符检测 + 三态输出 | ⑰⑱ |
| `scripts/test_hud.py` | 改：pre_cmds dict stdin、remove_db、env_extra 三处 | ⑨⑮ |
| `scripts/hudlib/assertions.py` | 改：stdout_visible_width_max | ⑮ |
| `scripts/hudlib/cases.py` | 改：P4 组 7 用例 + D5-07/08/P2-03/D5-15 断言更新，断言数 130 | ⑨⑮⑰⑱ |
| `docs/...` COMPLETE/DEPLOY/CHANGELOG/README | 改：批次收尾文档 | ⑧ |

**对 spec 测试策略的两处收敛**（执行时在报告中说明）：
1. P4-04（completion）折叠进既有用例 D5-07/D5-08 的断言更新——真实现后 `completion bash` 输出不再含 "bash"、`completion powershell` 不再报 "Unsupported"，两个用例必须改，改后即覆盖 P4-04 全部验收。
2. P4-09（doctor update 行）折叠进 P2-03 断言追加（spec 括号原文："doctor 既有用例同步断言"）。
3. P4-01 采用 harness 扩展 `pre_cmds` 支持 dict+stdin 的方案（两次 render 作为 pre_cmds，主运行直接是 `history` 命令输出断言），替代"断言 sqlite 行数"的内部方案——输出断言才是用户可见契约。

---

## 任务 1：⑨ 历史库消费

**Files:**
- Modify: `src/core/state.rs`（SnapshotSegment 增 agent_count，from_session 填充）
- Modify: `src/compact.rs`（should_checkout + run_pipeline 结账）
- Modify: `src/main.rs`（History 子命令 + run_history）
- Modify: `src/serve.rs`（weekly 字段 + HTML 卡片）
- Modify: `scripts/test_hud.py`（pre_cmds dict stdin + remove_db）
- Modify: `scripts/hudlib/cases.py`（P4-01/P4-02 + 断言数 130）

- [ ] **Step 1: state.rs — 加 agent_count 字段 + from_session 填充（含测试）**

在 `src/core/state.rs` 的 `SnapshotSegment`（42-55 行）增加字段：

```rust
    #[serde(default)]
    pub transcript_path: Option<String>,
    /// 结账用：快照时刻的活跃代理数（to_session 不还原该字段）。
    #[serde(default)]
    pub agent_count: usize,
```

`from_session`（177-201 行）在 `transcript_path: data.transcript_path.clone(),` 后追加：

```rust
            agent_count: data
                .subagent_status_line
                .as_ref()
                .map(|s| s.agents.len())
                .unwrap_or(0),
```

在同文件 `mod tests` 中追加测试：

```rust
    #[test]
    fn from_session_counts_agents() {
        let json = r#"{
            "model": {"id": "m", "display_name": "m"},
            "subagent_status_line": {"agents": [
                {"name": "a", "model": "x"},
                {"name": "b", "model": "x"}
            ]}
        }"#;
        let data = super::super::session::SessionData::from_stdin_json(json).unwrap();
        let snap = SnapshotSegment::from_session(&data, 42);
        assert_eq!(snap.agent_count, 2);
        assert_eq!(snap.timestamp_secs, 42);
    }
```

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test from_session_counts_agents`
预期：FAIL（`agent_count` 字段不存在，编译错误）。

- [ ] **Step 2: 实现后验证**

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test from_session_counts_agents`
预期：PASS。

- [ ] **Step 3: compact.rs — should_checkout 纯函数（先测后码）**

在 `src/compact.rs` 的 `should_restore`（137-139 行）附近新增：

```rust
/// ⑨ 会话切换结账判定：前次快照有结账信息（ts≠0、path 非空）且 path 变化 → 结账。
pub fn should_checkout(prev_ts: u64, prev_path: Option<&str>, cur_path: Option<&str>) -> bool {
    prev_ts != 0
        && !prev_path.map(|p| p.is_empty()).unwrap_or(true)
        && prev_path != cur_path
}
```

在 `mod tests` 中追加：

```rust
    #[test]
    fn should_checkout_four_states() {
        assert!(!should_checkout(0, Some("/a.jsonl"), Some("/b.jsonl"))); // ts=0 不结账
        assert!(!should_checkout(100, Some(""), Some("/b.jsonl"))); // prev path 为空
        assert!(!should_checkout(100, None, Some("/b.jsonl")));
        assert!(!should_checkout(100, Some("/a.jsonl"), Some("/a.jsonl"))); // 同 path 不重复
        assert!(should_checkout(100, Some("/a.jsonl"), Some("/b.jsonl"))); // 不同 path
        assert!(should_checkout(100, Some("/a.jsonl"), None)); // 新会话无 path 也结账
    }
```

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test should_checkout_four_states`
预期：FAIL（函数不存在）。实现后再运行：PASS。

- [ ] **Step 4: compact.rs — run_pipeline 结账钩子**

`run_pipeline` 中，在 `state.alerts = cooldown.to_state();`（119 行）与 `state.snapshot = SnapshotSegment::from_session(data, now);`（123 行）之间插入：

```rust
    // ⑨ 会话切换结账：transcript_path 变化 → 上一会话写入历史库（失败仅警告，不中断渲染）
    if should_checkout(
        state.snapshot.timestamp_secs,
        state.snapshot.transcript_path.as_deref(),
        data.transcript_path.as_deref(),
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
    }
```

文件顶部 import 追加：`use crate::core::history::HistoryStore;`

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo build`
预期：编译通过（HistoryStore::record_session 签名 `(&self, &SessionData, usize, &str)` 已匹配）。

- [ ] **Step 5: main.rs — History 子命令**

`Commands` 枚举（`Completion` 变体后）新增：

```rust
    /// Cross-session usage history (weekly stats, recent sessions, daily cost)
    History,
```

`main()` 的 match 新增分支：

```rust
        Commands::History => run_history(&config),
```

import 追加：`use core::history::HistoryStore;`

文件末尾（`handle_widget` 之后）新增：

```rust
/// ⑨ `history`：本周统计 / 最近会话 / 近 7 天日费用。空库显示 —，不显示 0。
fn run_history(config: &AppConfig) -> Result<(), String> {
    let store = HistoryStore::open()?;
    let symbol = &config.currency_symbol;
    let weekly = store.weekly_stats()?;
    println!("Weekly stats:");
    if weekly.total_sessions == 0 {
        println!("  Cost: — | Sessions: — | Tokens: — | Avg duration: — | Avg agents: —");
    } else {
        println!(
            "  Cost: {}{:.2} | Sessions: {} | Tokens: {} | Avg duration: {:.1}m | Avg agents: {:.1}",
            symbol, weekly.total_cost, weekly.total_sessions, weekly.total_tokens,
            weekly.avg_duration_min, weekly.avg_agents_per_session,
        );
    }
    println!("Recent sessions:");
    let recent = store.recent_sessions(5)?;
    if recent.is_empty() {
        println!("  —");
    } else {
        for r in recent {
            println!(
                "  #{}  {}  {}{:.2}  {}  {} agents  {} tok",
                r.id, r.started_at, symbol, r.total_cost_usd,
                format_history_duration(r.duration_secs), r.agent_count,
                format_history_tokens(r.total_tokens),
            );
        }
    }
    println!("Daily cost (last 7 days):");
    let trend = store.daily_cost_trend()?;
    if trend.is_empty() {
        println!("  —");
    } else {
        for (day, cost) in trend {
            println!("  {}  {}{:.2}", day, symbol, cost);
        }
    }
    Ok(())
}

/// 时长人类化：≥60s 显示 "Nm"，否则 "Ns"。
fn format_history_duration(secs: u64) -> String {
    if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}

/// 千位缩写（spec 样例口径）：45000 → "45k"。
fn format_history_tokens(tokens: u64) -> String {
    if tokens >= 1000 {
        format!("{}k", (tokens as f64 / 1000.0).round() as u64)
    } else {
        tokens.to_string()
    }
}
```

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo build`
预期：编译通过。

- [ ] **Step 6: serve.rs — weekly 字段 + 前端卡片**

`src/serve.rs` import 追加：`use crate::core::history::HistoryStore;`

`build_api_json` 的 format! 追加 weekly 字段：

```rust
    format!(
        r#"{{"model":"{}","context_pct":{},"cost_usd":{},"duration_ms":{},"weekly":{},"widgets":[{}]}}"#,
        data.model.display_name,
        data.context_window.used_percentage,
        data.cost.total_cost_usd,
        data.cost.total_duration_ms,
        weekly_json(),
        widgets_json.join(","),
    )
```

同文件新增：

```rust
/// ⑨ 本周聚合统计：open/query 失败 → available:false 全 0（前端显示 —）。
fn weekly_json() -> String {
    let weekly = HistoryStore::open()
        .ok()
        .and_then(|h| h.weekly_stats().ok());
    match weekly {
        Some(w) => format!(
            r#"{{"available":true,"total_cost":{},"total_sessions":{},"total_tokens":{},"avg_duration_min":{},"avg_agents_per_session":{}}}"#,
            w.total_cost, w.total_sessions, w.total_tokens, w.avg_duration_min,
            w.avg_agents_per_session,
        ),
        None => r#"{"available":false,"total_cost":0,"total_sessions":0,"total_tokens":0,"avg_duration_min":0,"avg_agents_per_session":0}"#
            .to_string(),
    }
}
```

HTML：`build_dashboard_html` 中 "Duration" 卡片（181-184 行）之后插入：

```html
  <div class="card">
    <div class="card-title">This Week</div>
    <div class="metric-big" id="val-week-cost">--</div>
    <div class="metric-label"><span id="val-week-sessions">--</span> sessions</div>
  </div>
```

JS `refresh()` 中 `document.getElementById('val-dur')...` 之后插入：

```js
    const wk = data.weekly || {};
    if (wk.available) {
      document.getElementById('val-week-cost').textContent = '$' + wk.total_cost.toFixed(2);
      document.getElementById('val-week-sessions').textContent = wk.total_sessions;
    } else {
      document.getElementById('val-week-cost').textContent = '—';
      document.getElementById('val-week-sessions').textContent = '—';
    }
```

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo build`
预期：编译通过。

- [ ] **Step 7: harness — pre_cmds dict stdin + remove_db**

`scripts/test_hud.py` 中 `runner.write_config` 调用之后（`pre_warnings` 循环之前）插入：

```python
    # P4 ⑨：可选清空 history.db（必须在任何 checkout 渲染之前）
    if case.get("remove_db"):
        db_path = os.path.join(runner.HUD_DIR, "history.db")
        if os.path.isfile(db_path):
            os.remove(db_path)
```

pre_cmds 循环（原 `for pre in case.get("pre_cmds", []):` 单行调用）替换为：

```python
    pre_warnings = []
    for pre in case.get("pre_cmds", []):
        if isinstance(pre, dict):
            r = runner.run_exe(exe_path, pre["args"],
                               stdin_text=pre.get("stdin"),
                               env_extra=case.get("env_extra"),
                               timeout_s=10)
        else:
            r = runner.run_exe(exe_path, pre,
                               env_extra=case.get("env_extra"),
                               timeout_s=10)
        if r.exit_code != 0 or r.timed_out:
            pre_warnings.append(f"pre_cmd exit={r.exit_code}: {pre!r}")
            print(
                f"  [WARN] pre_cmd failed (exit={r.exit_code}): {pre!r}"
            )
```

- [ ] **Step 8: cases.py — P4-01 / P4-02 + 断言数 130**

`scripts/hudlib/cases.py` 中 `P3 = [...]` 之后、`CASES = D1 + ... + P3` 之前新增：

```python
# --- Phase 4（⑨⑩⑪⑮⑯⑰⑱ 批次 C 剩余）---
# P4-01 通过 pre_cmds dict+stdin 执行两次 render（不同 transcript_path），
# 主运行直接断言 `history` 命令输出（用户可见契约，而非 sqlite 内部行数）。
P4 = [
    render_case("P4-01", "两次 render 不同 path → history 结账 1 条", "P4",
                {"exit": 0, "stdout_contains": ["Weekly stats",
                                                "Recent sessions", "#1"]},
                args=["history"], config=DEFAULT_CONFIG,
                pre_cmds=[
                    {"args": ["render"],
                     "stdin": j(full_dict(**{"transcript_path": "/a.jsonl"}))},
                    {"args": ["render"],
                     "stdin": j(full_dict(**{"transcript_path": "/b.jsonl"}))},
                ],
                remove_db=True,
                note="⑨：render A（/a.jsonl）→ render B（/b.jsonl）切换时结账 A；history 输出 1 条 Recent session（#1）"),
    render_case("P4-02", "history 空库显示 —", "P4",
                {"exit": 0, "stdout_contains": ["—"]},
                args=["history"], config=DEFAULT_CONFIG,
                remove_db=True,
                note="⑨：空库各数值位输出 —（不显示 0）；HistoryStore::open 失败则 Err 上报"),
]
```

`CASES` 行改为：

```python
CASES = D1 + D2 + D3 + D4 + D5 + D6 + D7 + D8 + P1 + P2 + P3 + P4
assert len(CASES) == 130, f"expected 130 cases, got {len(CASES)}"
```

- [ ] **Step 9: 验证**

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test && cargo build`
预期：全量单元测试 PASS。

运行：`python scripts/test_hud.py --case P4-01 --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
预期：P4-01 PASS（"Weekly stats"、"Recent sessions"、"#1" 均在 stdout）。

运行：`python scripts/test_hud.py --case P4-02 --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
预期：P4-02 PASS（stdout 含 "—"）。

运行：`python scripts/test_hud.py --case D6-02 --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
预期：D6-02 PASS（新增 weekly 键不破坏 JSON 解析）。

- [ ] **Step 10: 提交（用户手动执行，AI 不跑 git mutating 命令）**

```bash
git add src/core/state.rs src/compact.rs src/main.rs src/serve.rs scripts/test_hud.py scripts/hudlib/cases.py
git commit -m "feat: ⑨ history 子命令 + 会话切换自动结账 + serve weekly 字段"
```

---

## 任务 2：⑩ Shell Widget Windows 分支 + 删死代码

**Files:**
- Modify: `src/core/scripting.rs`（run_shell_command 平台分支）
- Delete: `src/probe/system.rs`
- Modify: `src/probe/mod.rs`

- [ ] **Step 1: scripting.rs — 平台分支**

`src/core/scripting.rs:77-91` 的 `run_shell_command` 整体替换为：

```rust
/// Execute a shell command. On Windows use `cmd /C`, elsewhere `sh -c`.
pub fn run_shell_command(command: &str) -> Result<String, String> {
    use std::process::Command;
    #[cfg(windows)]
    let mut cmd = Command::new("cmd");
    #[cfg(not(windows))]
    let mut cmd = Command::new("sh");
    #[cfg(windows)]
    cmd.arg("/C").arg(command);
    #[cfg(not(windows))]
    cmd.arg("-c").arg(command);
    let output = cmd
        .output()
        .map_err(|e| format!("shell command failed: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("shell error: {}", stderr.trim()))
    }
}
```

- [ ] **Step 2: 删除死代码 + 验证**

运行：`grep -rn "time_now\|memory_mb\|probe::system" src/`
预期：无匹配（确认无调用者后删除）。

删除 `src/probe/system.rs`；`src/probe/mod.rs` 中删除 `pub mod system;` 一行。

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo build`
预期：编译通过（Windows 分支走 cmd /C，本机即 Windows，构建即验证）。

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test`
预期：全量 PASS（无回归）。

- [ ] **Step 3: 黑盒确认 shell widget 不回归**

运行：`python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")" 2>&1 | tail -5`
预期：全部 PASS（既有 shell widget 用例在 cmd /C 下行为一致）。

- [ ] **Step 4: 提交（用户手动执行）**

```bash
git add src/core/scripting.rs src/probe/system.rs src/probe/mod.rs
git commit -m "fix: ⑩ Shell Widget Windows 分支（cmd /C）；删除无调用者的 probe/system.rs"
```

---

## 任务 3：⑪ 占位功能与死代码收尾

**Files:**
- Modify: `Cargo.toml`（clap_complete）
- Modify: `src/main.rs`（generate_completion 真实现）
- Modify: `src/dashboard.rs`（删空分支、agents_edge、通知接线）
- Modify: `scripts/hudlib/cases.py`（D5-07/D5-08 更新）

- [ ] **Step 1: Cargo.toml + completion 真实现（先测后码）**

`Cargo.toml` 的 `clap = { version = "4", features = ["derive"] }` 下追加：

```toml
clap_complete = "4"
```

`src/main.rs` import 行改为：`use clap::{CommandFactory, Parser, Subcommand};`

`generate_completion`（679-689 行）整体替换为：

```rust
/// Generate shell completions for the given shell name.
fn generate_completion(shell: &str) -> Result<(), String> {
    let sh = clap_complete::Shell::from_shell_name(shell)
        .ok_or_else(|| format!("unsupported shell: {}", shell))?;
    clap_complete::generate(sh, &mut Cli::command(), "claude-hud", &mut std::io::stdout());
    Ok(())
}
```

`main()` 中 `Commands::Completion` 分支改为：

```rust
        Commands::Completion { shell } => generate_completion(&shell),
```

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo build`
预期：编译通过（`CommandFactory` 提供 `Cli::command()`）。

黑盒验证（真输出）：

```bash
echo '{"model":{"id":"t","display_name":"t"}}' | target/debug/claude-hud.exe completion bash | head -3
```

预期：输出含 `_claude_hud`（bash 补全函数）。再运行 `target/debug/claude-hud.exe completion nope`，预期 stderr 含 `error: unsupported shell: nope`、exit 1。

- [ ] **Step 2: cases.py — D5-07/D5-08 断言更新**

D5-07（481-483 行）改为：

```python
    render_case("D5-07", "completion bash 真补全脚本", "D5",
                {"exit": 0, "stdout_contains": ["_claude_hud"]},
                args=["completion", "bash"],
                note="⑪：clap_complete 真实现，输出 bash 补全函数 _claude_hud（原占位文本已删）"),
```

D5-08（484-486 行）改为：

```python
    render_case("D5-08", "completion 不支持 shell 报错", "D5",
                {"exit": -1, "stderr_contains": ["unsupported shell"]},
                args=["completion", "nope"],
                note="⑪：不支持的 shell 走统一错误路径 exit 1（powershell 已被 clap_complete 支持，不再报错）"),
```

- [ ] **Step 3: dashboard.rs — 删空分支 + agents_edge（先测后码）**

删除 `run_loop` 中 `KeyCode::Char('1'..='9')` 空分支（132-134 行）。

`src/dashboard.rs` 文件末尾新增 `mod tests`：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_layout_cycles_three_layouts() {
        assert_eq!(next_layout("grid-2x2"), "sidebar");
        assert_eq!(next_layout("sidebar"), "focus");
        assert_eq!(next_layout("focus"), "grid-2x2");
    }

    #[test]
    fn next_layout_unknown_starts_from_grid() {
        assert_eq!(next_layout(""), "grid-2x2");
        assert_eq!(next_layout("tabbed"), "grid-2x2");
    }

    #[test]
    fn agents_edge_three_states() {
        assert_eq!(agents_edge(0, 0), None);
        assert_eq!(agents_edge(2, 2), None);
        assert_eq!(agents_edge(2, 0), Some(2));
        assert_eq!(agents_edge(0, 2), None);
    }
}
```

（next_layout 在任务 5 实现；此处先放测试跑出 FAIL 也可以，但为保持每任务可编译，本任务先实现 agents_edge + 保留 next_layout 到任务 5。**顺序调整：本任务只做 agents_edge，next_layout 测试随任务 5 一起写。**）

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test agents_edge`
预期：FAIL（函数不存在）。实现 `agents_edge`：

```rust
/// ⑪ 通知边界：活跃代理数从 >0 降到 0 → Some(前值)（"全部代理已结束"触发条件）。
pub fn agents_edge(prev: usize, cur: usize) -> Option<usize> {
    if prev > 0 && cur == 0 {
        Some(prev)
    } else {
        None
    }
}
```

再运行：PASS。

- [ ] **Step 4: dashboard.rs — 通知接线**

`run_loop` 开头（`let tick_rate = ...` 后）：

```rust
    let mut last_agent_count: usize = 0;
    let mut notified_stalled: HashSet<String> = HashSet::new();
```

`alert::send_notifications(...)` 调用（108-114 行）之后、`terminal.draw` 之前插入：

```rust
        // ⑪ 通知接线：代理全部结束（agents_edge 上升沿） / 代理卡顿（进程内去重）
        let now = state::now_secs();
        let active = data
            .subagent_status_line
            .as_ref()
            .map(|s| s.agents.len())
            .unwrap_or(0);
        if let Some(done) = agents_edge(last_agent_count, active) {
            crate::notify::agents_complete(done);
        }
        last_agent_count = active;

        if let Some(ref s) = summary {
            let threshold = config
                .widget_config("agent_overview")
                .get_u64("stall_threshold_sec", 30);
            let stalled = s.stalled_agents(threshold, now);
            if stalled.is_empty() {
                notified_stalled.clear();
            } else {
                for agent in stalled {
                    if notified_stalled.insert(agent.name.clone()) {
                        let idle = agent
                            .last_tool_call_secs
                            .map(|t| now.saturating_sub(t))
                            .unwrap_or(0);
                        crate::notify::agent_stalled(&agent.name, idle);
                    }
                }
            }
        }
```

import 追加：`use std::collections::HashSet;`

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test && cargo build`
预期：全量 PASS + 编译通过（`stalled_agents(&self, u64, u64) -> Vec<&AgentRecord>` 签名匹配）。

- [ ] **Step 5: 黑盒验证**

运行：`python scripts/test_hud.py --case D5-07 --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
预期：PASS（`_claude_hud`）。

运行：`python scripts/test_hud.py --case D5-08 --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
预期：PASS（`completion nope` exit 非零 + stderr `unsupported shell`）。

运行：`python scripts/test_hud.py --case D7-01 --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
预期：PASS（timed_out=True 语义不变，通知接线不改变非 TTY 行为——summary=None 时卡顿块不执行）。

- [ ] **Step 6: 提交（用户手动执行）**

```bash
git add Cargo.toml src/main.rs src/dashboard.rs scripts/hudlib/cases.py
git commit -m "feat: ⑪ completion 真实现（clap_complete）；dashboard 通知接线 + 空分支清理"
```

---

## 任务 4：⑮ compact 零宽度感知

**Files:**
- Modify: `Cargo.toml`（unicode-width）
- Modify: `src/core/ansi.rs`（strip_ansi）
- Modify: `src/compact.rs`（columns_from/columns_env/fit_line + render_with_data 应用）
- Modify: `src/widgets/model_display.rs`、`src/widgets/git_status.rs`、`src/widgets/agent_detail.rs`
- Modify: `scripts/test_hud.py`（env_extra 三处）
- Modify: `scripts/hudlib/assertions.py`（stdout_visible_width_max）
- Modify: `scripts/hudlib/cases.py`（P4-07/P4-08）

- [ ] **Step 1: Cargo.toml + ansi.rs strip_ansi（先测后码）**

`Cargo.toml` `[dependencies]` 中 `dirs = "5"` 后追加：

```toml
# 终端宽度测量（Phase 4 ⑮）
unicode-width = "0.2"
```

`src/core/ansi.rs` 末尾（`truncate` 之后）追加：

```rust
/// Strip ANSI SGR escape sequences (e.g. \x1b[38;2;r;g;bm ... \x1b[0m).
/// Any ESC[ … letter sequence is removed; other text passes through.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}
```

`mod tests` 追加：

```rust
    #[test]
    fn strip_ansi_no_codes_passthrough() {
        assert_eq!(strip_ansi("plain text"), "plain text");
    }

    #[test]
    fn strip_ansi_single_segment() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(strip_ansi("\x1b[38;2;255;0;0mrgba\x1b[0m"), "rgba");
    }

    #[test]
    fn strip_ansi_adjacent_segments() {
        assert_eq!(
            strip_ansi("\x1b[38;2;255;0;0mA\x1b[0m\x1b[1mB\x1b[0m"),
            "AB"
        );
    }

    #[test]
    fn strip_ansi_mixed_plain_and_codes() {
        assert_eq!(strip_ansi("a\x1b[0mb\x1b[31mc"), "abc");
    }
```

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test strip_ansi`
预期：FAIL（函数不存在）→ 实现后 PASS。

- [ ] **Step 2: compact.rs — columns_from / columns_env / fit_line（先测后码）**

`src/compact.rs` 顶部 import 追加：`use crate::core::ansi;` 与 `use unicode_width::UnicodeWidthStr;`

`should_restore` 附近追加：

```rust
/// 解析 COLUMNS 值（None = 缺失）：非法 → 80；最小 40（statusLine 最小可用宽度）。
pub fn columns_from(value: Option<&str>) -> u16 {
    value
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(80)
        .max(40)
}

/// 当前终端可见宽度源：COLUMNS 环境变量（statusLine 场景唯一可靠来源）。
pub fn columns_env() -> u16 {
    columns_from(std::env::var("COLUMNS").ok().as_deref())
}

/// ⑮ 从行尾整组丢弃直至可见宽度 ≤ max_width（剥 ANSI 后按 unicode 宽度测）；
/// 至少保留 1 组；sep 为空时原样返回。
pub fn fit_line(line: &str, sep: &str, max_width: usize) -> String {
    if sep.is_empty() {
        return line.to_string();
    }
    let groups: Vec<&str> = line.split(sep).collect();
    let mut keep = groups.len();
    while keep > 1 {
        let candidate = groups[..keep].join(sep);
        if ansi::strip_ansi(&candidate).as_str().width() <= max_width {
            break;
        }
        keep -= 1;
    }
    groups[..keep].join(sep)
}
```

`mod tests` 追加：

```rust
    #[test]
    fn columns_from_missing_or_invalid_defaults_to_80() {
        assert_eq!(columns_from(None), 80);
        assert_eq!(columns_from(Some("abc")), 80);
        assert_eq!(columns_from(Some("-5")), 80); // 解析失败走默认
    }

    #[test]
    fn columns_from_parses_and_clamps_min_40() {
        assert_eq!(columns_from(Some("120")), 120);
        assert_eq!(columns_from(Some("30")), 40);
    }

    #[test]
    fn fit_line_drops_tail_groups_when_over_width() {
        let line = "aaaa │ bbbb │ cccc";
        assert_eq!(fit_line(line, " │ ", 12), "aaaa │ bbbb");
        assert_eq!(fit_line(line, " │ ", 17), line);
    }

    #[test]
    fn fit_line_keeps_single_overwide_group() {
        assert_eq!(fit_line("toolonggroup", " │ ", 5), "toolonggroup");
    }

    #[test]
    fn fit_line_ignores_ansi_width() {
        let line = "\x1b[31mabc\x1b[0m │ x";
        assert_eq!(fit_line(line, " │ ", 7), line);
    }

    #[test]
    fn fit_line_measures_cjk_width() {
        assert_eq!(fit_line("中文 │ abc", " │ ", 8), "中文");
        assert_eq!(fit_line("中文 │ abc", " │ ", 10), "中文 │ abc");
    }
```

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test fit_line && cargo test columns_from`
预期：FAIL（函数不存在）→ 实现后 PASS。

- [ ] **Step 3: render_with_data 应用 fit_line**

`render_with_data` 中（201-204 行）替换为：

```rust
        if !line_widgets.is_empty() {
            let joined = line_widgets.join(sep);
            // ⑮ 宽度感知：超出终端列宽时从行尾整组丢弃
            output.push_str(&fit_line(&joined, sep, columns_env() as usize));
            output.push('\n');
        }
```

- [ ] **Step 4: 三个 widget 字段级截断**

`src/widgets/model_display.rs`（17 行）：

```rust
        let name = ansi::truncate(&data.model.display_name, 24);
```

第 25 行 `ansi_fg(name, ...)` 改为 `ansi_fg(&name, ...)`。

`src/widgets/git_status.rs`（29 行）：

```rust
        parts.push(ansi::ansi_fg(&ansi::truncate(&s.branch, 24), &theme.accent));
```

`src/widgets/agent_detail.rs`（80 行）：

```rust
                    let name = ansi::ansi_fg(&ansi::truncate(&agent.name, 24), &theme.accent);
```

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test && cargo build`
预期：全量 PASS + 编译通过。

- [ ] **Step 5: harness — env_extra 三处 + stdout_visible_width_max**

`scripts/test_hud.py` 中三处 `runner.run_exe(...)` 调用追加 `env_extra=case.get("env_extra"),`：

1. pre_cmds 循环内（任务 1 已改的两行 dict/非 dict 分支）；
2. pre_render 调用（216-217 行）：
```python
        r = runner.run_exe(exe_path, ["render"], stdin_text=pre_text,
                           env_extra=case.get("env_extra"), timeout_s=10)
```
3. 主运行 render 分支（247-249 行）：
```python
        r = runner.run_exe(exe_path, case["args"], stdin_text=stdin_text,
                           env_extra=case.get("env_extra"), timeout_s=10)
```

`scripts/hudlib/assertions.py`：文件顶部 import 区追加 `import unicodedata`，`check()` 的 `_strip_ansi` 定义之后新增：

```python
def _visible_width(s: str) -> int:
    """Visible column width: ANSI stripped; CJK wide/fullwidth chars count 2."""
    w = 0
    for ch in _strip_ansi(s):
        w += 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1
    return w
```

`check()` 函数末尾（stderr 断言之后、`if fails:` 之前）追加：

```python
    if "stdout_visible_width_max" in spec:
        max_w = max((_visible_width(l) for l in out.splitlines()), default=0)
        if max_w > spec["stdout_visible_width_max"]:
            fails.append(
                f"stdout visible width {max_w} > {spec['stdout_visible_width_max']}"
            )
```

文件头 docstring 的 spec keys 列表追加一行：`- stdout_visible_width_max: int (max visible width of any output line)`。

- [ ] **Step 6: cases.py — P4-07 / P4-08**

`P4 = [...]` 列表（任务 1 创建）末尾追加：

```python
    render_case("P4-07", "COLUMNS=30 → 可见宽度 ≤ 40", "P4",
                {"exit": 0, "stdout_visible_width_max": 40},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                env_extra={"COLUMNS": "30"},
                note="⑮：columns_env clamp 到 40，fit_line 从行尾丢组直至 ≤40 列"),
    render_case("P4-08", "COLUMNS=200 → 输出完整无截断", "P4",
                {"exit": 0,
                 "stdout_contains": ["deepseek-v4-flash", "$0.03"],
                 "stdout_not_contains": ["..."]},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                env_extra={"COLUMNS": "200"},
                note="⑮：宽终端（≥120 列）与无 COLUMNS 行为一致——不丢组、无 truncate 省略号"),
```

- [ ] **Step 7: 验证**

运行：`python scripts/test_hud.py --case P4-07 --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
预期：PASS（输出各行可见宽度 ≤ 40）。

运行：`python scripts/test_hud.py --case P4-08 --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
预期：PASS。

运行：`python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")" 2>&1 | tail -5`
预期：全量 PASS（既有用例在默认宽度下不回归）。

- [ ] **Step 8: 提交（用户手动执行）**

```bash
git add Cargo.toml src/core/ansi.rs src/compact.rs src/widgets/model_display.rs src/widgets/git_status.rs src/widgets/agent_detail.rs scripts/test_hud.py scripts/hudlib/assertions.py scripts/hudlib/cases.py
git commit -m "feat: ⑮ compact 零宽度感知（COLUMNS + fit_line 组级截断 + 字段级 24 字符截断）"
```

---

## 任务 5：⑯ dashboard 交互

**Files:**
- Modify: `src/dashboard.rs`（next_layout/persist_layout/footer/帮助面板/l? 按键）

- [ ] **Step 1: next_layout（先测后码）**

`src/dashboard.rs` 文件末尾追加 `mod tests`（含任务 3 延后的 next_layout 测试）：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_layout_cycles_three_layouts() {
        assert_eq!(next_layout("grid-2x2"), "sidebar");
        assert_eq!(next_layout("sidebar"), "focus");
        assert_eq!(next_layout("focus"), "grid-2x2");
    }

    #[test]
    fn next_layout_unknown_starts_from_grid() {
        assert_eq!(next_layout(""), "grid-2x2");
        assert_eq!(next_layout("tabbed"), "grid-2x2");
    }
}
```

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test next_layout`
预期：FAIL（函数不存在）。实现：

```rust
/// ⑯ 'l' 键布局循环：grid-2x2 → sidebar → focus → grid-2x2；未知值从 grid-2x2 起步。
pub fn next_layout(cur: &str) -> String {
    match cur {
        "grid-2x2" => "sidebar".to_string(),
        "sidebar" => "focus".to_string(),
        "focus" => "grid-2x2".to_string(),
        _ => "grid-2x2".to_string(),
    }
}
```

再运行：PASS。

- [ ] **Step 2: run_loop 状态与按键**

`run_loop` 开头（`let mut last_agent_count: usize = 0;` 附近）追加：

```rust
    let mut layout_name = config.dashboard.default_layout.clone();
    let mut show_help = false;
```

按键 match（q/Esc 分支后）追加：

```rust
                    KeyCode::Char('l') => {
                        layout_name = next_layout(&layout_name);
                        persist_layout(config, &layout_name); // best-effort
                    }
                    KeyCode::Char('?') => show_help = !show_help;
```

`terminal.draw` 调用改为：

```rust
        terminal
            .draw(|frame| {
                draw_dashboard(
                    frame, registry, &data, theme, config, summary.as_ref(),
                    &layout_name, show_help,
                );
            })
            .map_err(|e| format!("draw: {}", e))?;
```

- [ ] **Step 3: draw_dashboard — 布局来源 + footer + 帮助面板**

`draw_dashboard` 签名追加两参数，body 重写：

```rust
fn draw_dashboard(
    frame: &mut Frame,
    registry: &WidgetRegistry,
    data: &SessionData,
    theme: &Theme,
    config: &AppConfig,
    summary: Option<&TranscriptSummary>,
    layout_name: &str,
    show_help: bool,
) {
    let area = frame.area();

    // 底部 1 行 footer；帮助面板展开时在其上方让出空间
    let areas = if show_help {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(HELP_PANEL_HEIGHT),
                Constraint::Length(1),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(area)
    };
    let main_area = areas[0];
    let footer_area = areas[areas.len() - 1];
    let help_area = if show_help { Some(areas[1]) } else { None };

    let layout = match layout_name {
        "sidebar" => build_sidebar(main_area),
        "focus" | "tabbed" => build_single_panel(main_area),
        _ => build_grid_2x2(main_area),
    };

    // Map widgets to panels (use compact_layout order as panel assignment)
    let widget_ids: Vec<&str> = config.compact_layout.iter()
        .map(|s| s.as_str())
        .collect();

    for (i, panel_area) in layout.iter().enumerate() {
        let widget_id = widget_ids.get(i).copied().unwrap_or("context_bar");
        if let Some(widget) = registry.get(widget_id) {
            let mut widget_config = config.widget_config(widget_id);
            pricing::inject_cost(data, summary, config, &mut widget_config);
            widget.render_dashboard(data, *panel_area, frame, theme, &widget_config);
        }
    }

    if let Some(h) = help_area {
        render_help(frame, h, config);
    }
    let footer = format!(
        "Layout: {} · Mod: {} · l=cycle ?=help q=quit",
        layout_name, config.active_mod
    );
    frame.render_widget(Paragraph::new(Text::from(footer)), footer_area);
}
```

文件末尾（mod tests 之前）新增：

```rust
/// 帮助面板高度（6 行内容 + 边框 2 行）。
const HELP_PANEL_HEIGHT: u16 = 8;

/// ⑯ 帮助面板：全部按键 + 全局生效说明。
fn render_help(frame: &mut Frame, area: ratatui::layout::Rect, config: &AppConfig) {
    let lines = vec![
        Line::from("q / Esc  quit"),
        Line::from("l        cycle layout (grid-2x2 → sidebar → focus)"),
        Line::from("?        toggle this help"),
        Line::from(""),
        Line::from("Layout & mod changes are global — they apply to all windows"),
        Line::from(format!(
            "(persisted to config.toml) · Active mod: {}",
            config.active_mod
        )),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(ratatui::widgets::Block::bordered().title(" Help ")),
        area,
    );
}

/// ⑯ 读-改-写 config.toml 的 dashboard.default_layout；失败 eprintln 警告不中断。
/// TOML 往返会丢失注释（拍板取舍，doctor 与文档提示）。
fn persist_layout(config: &AppConfig, layout: &str) {
    let config_path = match AppConfig::config_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[claude-hud] warning: cannot persist layout: {}", e);
            return;
        }
    };
    let Some(mut root) = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|c| toml::from_str::<toml::Value>(&c).ok())
        .filter(|v| v.is_table())
    else {
        eprintln!("[claude-hud] warning: config.toml unreadable; layout switch not persisted");
        return;
    };
    let Some(dashboard) = root
        .as_table_mut()
        .expect("filtered to a table")
        .entry("dashboard")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
    else {
        eprintln!("[claude-hud] warning: [dashboard] is not a table; layout switch not persisted");
        return;
    };
    dashboard.insert(
        "default_layout".to_string(),
        toml::Value::String(layout.to_string()),
    );
    let Ok(out) = toml::to_string_pretty(&root) else {
        eprintln!("[claude-hud] warning: serialize config failed; layout switch not persisted");
        return;
    };
    if let Err(e) = std::fs::write(&config_path, out) {
        eprintln!("[claude-hud] warning: write config: {}", e);
    }
}
```

import 追加：`use ratatui::widgets::Paragraph;`

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test && cargo build`
预期：全量 PASS + 编译通过。

- [ ] **Step 4: 黑盒验证 dashboard 既有行为**

运行：`python scripts/test_hud.py --case D7-01 --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
预期：PASS（非 TTY 超时语义不变）。

- [ ] **Step 5: 提交（用户手动执行）**

```bash
git add src/dashboard.rs
git commit -m "feat: ⑯ dashboard 布局循环（l 键）+ config.toml 持久化 + 帮助面板 + footer"
```

---

## 任务 6：⑰ 安装占位符 / 时间戳备份 / 全局提示

**Files:**
- Modify: `src/core/cc_config.rs`（has_status_line）
- Modify: `src/main.rs`（setup 备份重写、uninstall 提示、全局生效提示）
- Modify: `scripts/install.sh`、`scripts/install.ps1`（占位符检测）
- Modify: `scripts/hudlib/cases.py`（P4-05/P4-06、D5-15 更新）

- [ ] **Step 1: cc_config.rs — has_status_line（先测后码）**

`src/core/cc_config.rs` 中 `remove_status_line` 之后新增：

```rust
/// True when the settings JSON contains a statusLine key (any shape).
/// Unparseable JSON returns false.
pub fn has_status_line(existing: &str) -> bool {
    match serde_json::from_str::<Value>(existing) {
        Ok(v) => v.get("statusLine").is_some(),
        Err(_) => false,
    }
}
```

`mod tests` 追加：

```rust
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
```

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test has_status_line`
预期：FAIL（函数不存在）→ 实现后 PASS。

- [ ] **Step 2: main.rs — setup_cc_settings 时间戳备份重写**

`setup_cc_settings`（217-249 行）整体替换为：

```rust
/// Merge the HUD statusLine into ~/.claude/settings.json. A timestamped
/// backup (settings.json.hud.bak-<epoch>) is written only when an existing
/// statusLine or unparseable JSON would be overwritten; the fixed-name
/// json.bak is gone and .hud.bak-* is never deleted by setup/uninstall.
fn setup_cc_settings() -> Result<(), String> {
    let home = dirs::home_dir()
        .ok_or_else(|| "cannot find home directory".to_string())?;
    let settings_path = home.join(".claude").join("settings.json");
    let original = if settings_path.exists() {
        std::fs::read_to_string(&settings_path)
            .map_err(|e| format!("read settings.json: {}", e))?
    } else {
        String::new()
    };

    let valid_json = serde_json::from_str::<serde_json::Value>(&original).is_ok();
    if !original.trim().is_empty() && (cc_config::has_status_line(&original) || !valid_json) {
        let backup = settings_path.with_file_name(format!(
            "settings.json.hud.bak-{}",
            now_secs()
        ));
        std::fs::write(&backup, &original)
            .map_err(|e| format!("backup settings.json: {}", e))?;
        if cc_config::has_status_line(&original) {
            println!("replacing existing statusLine (backup at {:?})", backup);
        } else {
            println!(
                "warning: settings.json is not valid JSON — original saved to {:?}; rebuilding with minimal config (restore other settings from the backup)",
                backup
            );
        }
    }

    let merged = if valid_json {
        cc_config::merge_status_line(&original)?
    } else {
        cc_config::merge_status_line("")?
    };
    write_atomic(&settings_path, &merged)?;
    println!("Claude Code status line configured in {:?}", settings_path);
    Ok(())
}
```

import 行 `use core::state::{StateFile, write_atomic};` 改为：
`use core::state::{StateFile, now_secs, write_atomic};`

- [ ] **Step 3: main.rs — uninstall 提示 + 全局生效提示**

`run_uninstall` 的 `println!("Done. ...")` 之前追加：

```rust
    println!("Your original settings backup (if any) is at ~/.claude/settings.json.hud.bak-* — copy it back over ~/.claude/settings.json to restore.");
```

全局生效提示（`handle_mod` / `handle_theme` 各 println 追加 ` (applies to all windows)`）：

1. `mod use`（非 `-` 分支，324 行）：`println!("Switched to mod '{}' ✓ (applies to all windows)", target);`
2. `mod use -`（313 行）：`println!("Switched back to mod '{}' ✓ (applies to all windows)", prev);`
3. `mod pick`（446 行）：`println!("Switched to mod '{}' ✓ (applies to all windows)", target);`
4. `mod save`（361 行）：`println!("Saved mod '{}' to {:?} (applies to all windows)", name, path);`
5. `mod delete`（386 行）：`println!("Deleted mod '{}' (applies to all windows)", name);`
6. `mod import`（379 行）：`println!("Imported mod '{}' to {:?} (applies to all windows)", name, path);`
7. `mod reset`（398 行）：`println!("Reset to factory default (Glacier Workstation) (applies to all windows)");`
8. `theme import`（650 行）：`println!("Theme imported to config.toml [theme] section (applies to all windows)");`

- [ ] **Step 4: install.sh / install.ps1 — 占位符检测**

`scripts/install.sh` 的 `else` 网络分支内、`LATEST="$(curl ...)"` 之前插入：

```bash
  if [ "$REPO" = "user/claude-hud" ]; then
    echo "error: Claude HUD 尚未发布，请使用源码构建（cargo build --release）" >&2
    exit 1
  fi
```

`scripts/install.ps1` 的 `else` 分支内、TLS 行之前插入：

```powershell
    if ($Repo -eq 'user/claude-hud') {
        Write-Host 'error: Claude HUD 尚未发布，请使用源码构建（cargo build --release）'
        exit 1
    }
```

- [ ] **Step 5: cases.py — P4-05 / P4-06 + D5-15 更新**

`P4` 列表追加：

```python
    render_case("P4-05", "mod use 输出全局生效提示", "P4",
                {"exit": 0, "stdout_contains": ["(applies to all windows)"]},
                args=["mod", "use", "ember-night"], config=DEFAULT_CONFIG,
                note="⑰：写配置命令追加全局生效提示（mod use 代表 8 处接线）"),
    render_case("P4-06", "theme import 输出全局生效提示", "P4",
                {"exit": 0, "stdout_contains": ["imported",
                                                "(applies to all windows)"]},
                args=["theme", "import", fx("theme/nord_partial.toml")],
                config=DEFAULT_CONFIG,
                config_file_contains=["[theme]", "accent = \"#ff00ff\""],
                note="⑰：theme import 追加提示且落盘行为不变（复用 P3-06 流程）"),
```

D5-15（509-514 行）stdout_contains 改为：

```python
                {"exit": 0,
                 "stdout_contains": ["settings.json",
                                     "replacing existing statusLine (backup at"]},
```

note 更新为：

```python
                note="⑰：本机 settings.json 已有 statusLine → 时间戳备份 + replacing 提示（真实环境 statusLine 不存在时该断言不适用）"),
```

- [ ] **Step 6: 验证**

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test && cargo build`
预期：全量 PASS + 编译通过。

运行：`python scripts/test_hud.py --case P4-05 --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
预期：PASS（stdout 含 `(applies to all windows)`）。

运行：`python scripts/test_hud.py --case P4-06 --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
预期：PASS。

运行：`python scripts/test_hud.py --case D5-15 --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
预期：PASS（注意：本次运行会在真实 `~/.claude/` 留下一个 `settings.json.hud.bak-<epoch>`，属设计行为，不清理）。

运行：`python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")" 2>&1 | tail -5`
预期：全量 PASS。

- [ ] **Step 7: 提交（用户手动执行）**

```bash
git add src/core/cc_config.rs src/main.rs scripts/install.sh scripts/install.ps1 scripts/hudlib/cases.py
git commit -m "feat: ⑰ 时间戳备份（settings.json.hud.bak-epoch）+ 安装占位符检测 + 全局生效提示"
```

---

## 任务 7：⑱ 升级通路

**Files:**
- Create: `src/core/update.rs`
- Modify: `src/core/mod.rs`
- Modify: `src/main.rs`（Update 子命令）
- Modify: `src/doctor.rs`（update 信息项）
- Modify: `scripts/install.sh`、`scripts/install.ps1`（三态输出）
- Modify: `scripts/hudlib/cases.py`（P4-03、P2-03 更新）

- [ ] **Step 1: src/core/update.rs — cmp_versions 与 check_update（先测后码）**

新建 `src/core/update.rs`：

```rust
use std::cmp::Ordering;

/// 发布仓库占位符：发布前与 install.sh / install.ps1 / Cargo.toml repository
/// 同步替换为真实仓库；占位符阶段 update check 短路为 NotPublished（零网络）。
pub const UPDATE_REPO: &str = "user/claude-hud";

/// 版本比较：按 `.` 分段逐段数字比较；段数不同时缺段视为 0；前缀 v 忽略。
pub fn cmp_versions(a: &str, b: &str) -> Ordering {
    let a_nums: Vec<u64> = a
        .trim_start_matches('v')
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    let b_nums: Vec<u64> = b
        .trim_start_matches('v')
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    let max_len = a_nums.len().max(b_nums.len());
    for i in 0..max_len {
        let av = a_nums.get(i).copied().unwrap_or(0);
        let bv = b_nums.get(i).copied().unwrap_or(0);
        if av != bv {
            return av.cmp(&bv);
        }
    }
    Ordering::Equal
}

/// 升级检查结果。
pub enum UpdateStatus {
    /// 已是最新：携带当前版本号。
    UpToDate(String),
    /// 有新版本：携带最新版本号。
    Available(String),
    /// 仓库未发布（占位符或 404）。
    NotPublished,
    /// 网络/其他错误。
    Unavailable,
}

/// 查询 GitHub latest release 与本地版本比较。占位符仓库零网络短路。
pub fn check_update() -> UpdateStatus {
    if UPDATE_REPO == "user/claude-hud" {
        return UpdateStatus::NotPublished; // 占位符短路，零网络
    }
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        UPDATE_REPO
    );
    let resp = ureq::get(&url)
        .set("User-Agent", "claude-hud")
        .timeout(std::time::Duration::from_secs(10))
        .call();
    let body = match resp {
        Ok(r) => r.into_string().unwrap_or_default(),
        Err(ureq::Error::Status(404, _)) => return UpdateStatus::NotPublished,
        Err(_) => return UpdateStatus::Unavailable,
    };
    let Some(tag) = extract_tag_name(&body) else {
        return UpdateStatus::Unavailable;
    };
    let latest = tag.trim_start_matches('v').to_string();
    let current = env!("CARGO_PKG_VERSION").to_string();
    if cmp_versions(&current, &latest) != Ordering::Less {
        UpdateStatus::UpToDate(current)
    } else {
        UpdateStatus::Available(latest)
    }
}

/// 从 GitHub release JSON 中提取 tag_name。
fn extract_tag_name(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// 用户可读的检查结果（update check / doctor 共用；exit 0 恒定）。
pub fn describe(status: &UpdateStatus) -> String {
    match status {
        UpdateStatus::UpToDate(v) => format!("✓ up to date (v{})", v),
        UpdateStatus::Available(v) => {
            format!("↗ update available: v{} — re-run the install script to upgrade", v)
        }
        UpdateStatus::NotPublished => "not published yet".to_string(),
        UpdateStatus::Unavailable => "update check unavailable".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmp_versions_equal() {
        assert_eq!(cmp_versions("1.2.3", "1.2.3"), Ordering::Equal);
        assert_eq!(cmp_versions("v1.2.3", "1.2.3"), Ordering::Equal);
    }

    #[test]
    fn cmp_versions_newer_and_older() {
        assert_eq!(cmp_versions("1.2.3", "1.2.4"), Ordering::Less);
        assert_eq!(cmp_versions("1.2.4", "1.2.3"), Ordering::Greater);
    }

    #[test]
    fn cmp_versions_missing_segment_is_zero() {
        assert_eq!(cmp_versions("1.2", "1.2.3"), Ordering::Less);
        assert_eq!(cmp_versions("1.2.3", "1.2"), Ordering::Greater);
    }

    #[test]
    fn describe_matches_spec_wording() {
        assert_eq!(describe(&UpdateStatus::NotPublished), "not published yet");
        assert_eq!(describe(&UpdateStatus::Unavailable), "update check unavailable");
        assert_eq!(describe(&UpdateStatus::UpToDate("0.2.0".into())), "✓ up to date (v0.2.0)");
        assert_eq!(
            describe(&UpdateStatus::Available("0.3.0".into())),
            "↗ update available: v0.3.0 — re-run the install script to upgrade"
        );
    }
}
```

`src/core/mod.rs` 追加：`pub mod update;`

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test cmp_versions && cargo test describe_matches_spec_wording`
预期：FAIL（模块不存在）→ 实现后 PASS。

- [ ] **Step 2: main.rs — Update 子命令**

`Commands` 枚举（`History` 变体后）新增：

```rust
    /// Upgrade checks
    Update {
        #[command(subcommand)]
        cmd: UpdateCommands,
    },
```

`History` 变体后新增：

```rust
#[derive(Subcommand)]
enum UpdateCommands {
    /// Check for a new release (placeholder repo: reports not published)
    Check,
}
```

match 新增分支：

```rust
        Commands::Update { cmd } => match cmd {
            UpdateCommands::Check => {
                let status = core::update::check_update();
                println!("{}", core::update::describe(&status));
                Ok(())
            }
        },
```

- [ ] **Step 3: doctor.rs — update 信息项**

`doctor::run` 中 `pricing_check(config, &mut failures);` 之后追加：

```rust
    update_check();
```

同文件末尾新增：

```rust
/// ⑱ 升级检查（信息项，永不计数为 failure）。
fn update_check() {
    let status = crate::core::update::check_update();
    match &status {
        crate::core::update::UpdateStatus::UpToDate(v) => {
            println!("  [ok] update: up to date (v{})", v)
        }
        crate::core::update::UpdateStatus::Available(v) => {
            println!(
                "  [ok] update: update available v{} — re-run the install script",
                v
            )
        }
        _ => println!("  [..] update: {}", crate::core::update::describe(&status)),
    }
}
```

- [ ] **Step 4: install.sh / install.ps1 — 三态输出**

`scripts/install.sh` 中现有的已安装判断块（31-35 行）：

```bash
  if [ -f "$INSTALL_DIR/version.txt" ] \
      && [ "$(cat "$INSTALL_DIR/version.txt")" = "$LATEST" ]; then
    echo "claude-hud ${LATEST} already installed — nothing to do."
    exit 0
  fi
```

替换为：

```bash
  if [ -f "$INSTALL_DIR/version.txt" ]; then
    OLD="$(cat "$INSTALL_DIR/version.txt")"
    if [ "$OLD" = "$LATEST" ]; then
      echo "claude-hud v${LATEST} is up to date"
      exit 0
    fi
    echo "upgrading v${OLD} → v${LATEST}"
  else
    echo "installing claude-hud v${LATEST}"
  fi
```

`scripts/install.ps1` 中（29-32 行）：

```powershell
    if ((Test-Path $VersionFile) -and ((Get-Content $VersionFile -Raw).Trim() -eq $Tag)) {
        Write-Host "claude-hud $($Tag.Replace('v','')) already installed - nothing to do."
        return
    }
```

替换为：

```powershell
    if (Test-Path $VersionFile) {
        $Old = (Get-Content $VersionFile -Raw).Trim()
        if ($Old -eq $Tag) {
            Write-Host "claude-hud v$($Tag.TrimStart('v')) is up to date"
            return
        }
        Write-Host "upgrading v$($Old.TrimStart('v')) → v$($Tag.TrimStart('v'))"
    } else {
        Write-Host "installing claude-hud v$($Tag.TrimStart('v'))"
    }
```

- [ ] **Step 5: cases.py — P4-03 + P2-03 断言追加**

`P4` 列表追加：

```python
    render_case("P4-03", "update check 占位符短路", "P4",
                {"exit": 0, "stdout_contains": ["not published yet"]},
                args=["update", "check"], config=DEFAULT_CONFIG,
                note="⑱：占位符仓库零网络返回 NotPublished（exit 0 恒定）"),
```

P2-03（660-662 行）stdout_contains 追加 "update:"：

```python
    render_case("P2-03", "doctor 契约探针 + update 信息项", "P2",
                {"exit": 0, "stdout_contains": ["contract probe", "update:"]},
                args=["doctor"], config=DEFAULT_CONFIG,
                note="⑱：doctor 含 update 检查行（信息项，不影响 exit 0）"),
```

- [ ] **Step 6: 验证**

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test && cargo build`
预期：全量 PASS + 编译通过。

运行：`python scripts/test_hud.py --case P4-03 --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
预期：PASS（`not published yet`，零网络）。

运行：`python scripts/test_hud.py --case P2-03 --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
预期：PASS（doctor exit 0 + `update:` 行）。

运行：`python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")" 2>&1 | tail -5`
预期：全量 PASS（130/130）。

- [ ] **Step 7: 提交（用户手动执行）**

```bash
git add src/core/update.rs src/core/mod.rs src/main.rs src/doctor.rs scripts/install.sh scripts/install.ps1 scripts/hudlib/cases.py
git commit -m "feat: ⑱ update check 子命令 + doctor 信息项 + install 脚本三态输出"
```

---

## 任务 8：文档回写 + 全量回归

**Files:**
- Modify: `COMPLETE.md`（§20 ⑨⑪⑮⑯⑰⑱ 状态、§21 路线图批次行）
- Modify: `DEPLOY.md`（history 命令、宽度说明、Shell Widget 平台说明、全局生效声明、发布版本口径）
- Modify: `CHANGELOG.md`（[0.3.0] 段）
- Modify: `README.md`（not-yet-released 标注 + Upgrade 一节）

- [ ] **Step 1: 读取三份文档现状**

运行：`head -60 COMPLETE.md`、`head -40 DEPLOY.md`、`head -30 README.md`
预期：确认 §20/§21、配置章节、安装段的结构与行号（执行时按实际内容编辑）。

- [ ] **Step 2: CHANGELOG.md — [0.3.0] 段**

在 `## [Unreleased]` 之后插入：

```markdown
## [0.3.0] - 2026-08-03 (Phase 4 — batch C remainder)

### Added
- `claude-hud history` 子命令（Weekly stats / Recent sessions / Daily cost，空库显示 `—`）
- render 会话切换自动结账：transcript 路径变化 → 上一会话写入 SQLite 历史库
- serve `/api/data` 增加 `weekly` 字段 + 前端 This Week 卡片（不可用时 `available:false` 显示 `—`）
- `claude-hud update check` 子命令（占位符仓库零网络短路）+ doctor `update:` 信息项
- `completion bash/zsh/fish/powershell` 真实补全脚本（clap_complete；不支持 shell 报错）
- compact 零宽度感知：COLUMNS 宽度源 + `fit_line` 组级截断 + 字段级 24 字符截断
- dashboard `l` 布局循环（grid-2x2/sidebar/focus）+ config.toml 持久化 + `?` 帮助面板 + 底部提示
- dashboard 代理结束 / 代理卡顿桌面通知接线（进程内去重）
- setup 时间戳备份 `settings.json.hud.bak-<epoch>`（不再覆盖固定名备份，永不删除）
- 写配置命令（mod use/reset/save/delete/import、theme import）输出 `(applies to all windows)` 提示
- install.sh/ps1 占位符仓库检测（未发布报错）+ 三态安装输出（installing / up to date / upgrading）

### Fixed
- Shell Widget Windows 分支：`cmd /C`（Unix 保持 `sh -c`）；删除无调用者的 probe/system.rs
- dashboard `'1'..='9'` 空分支删除；`last_agent_count` 不再恒 0（退出结账携带真实代理数）
```

- [ ] **Step 3: COMPLETE.md §20/§21**

§20 将 ⑨⑪⑮⑯⑰⑱ 标记 ✅（附一行交付摘要：命令名 + 关键行为）；§21 路线图追加一行批次记录（日期 + 任务号 + 用例数 130 + 单元数）。

- [ ] **Step 4: DEPLOY.md 四处补充**

1. 新增 `claude-hud history` 用法（三块输出样例 + 空库 `—`）。
2. 配置章节开头声明："配置全局生效于所有会话窗口；数据层面（session/git）各窗口独立"。
3. Shell 命令 Widget 章节加平台说明：Unix `sh -c` / Windows `cmd /C`；复杂命令建议写成 `.bat`/`.sh` 脚本再调用。
4. 发布章节写明版本口径：bump Cargo.toml → tag `vX.Y.Z` → CI 出 artifacts → install 脚本 / `update check` / CI 三方同一口径。

- [ ] **Step 5: README.md**

安装段加 not-yet-released 标注（真实仓库创建后移除）；新增 Upgrade 一节：

```markdown
## Upgrade

Re-run the install command to upgrade — the installer detects the installed
version and upgrades automatically. `config.toml` and session history are kept
in `~/.claude/plugins/claude-hud/` and survive upgrades.

> **Not yet published** — the install scripts refuse to run against the
> placeholder repository until a real release is cut. Use
> `cargo build --release` locally for now.
```

- [ ] **Step 6: 全量回归**

运行：`export PATH="$HOME/.cargo/bin:$PATH" && cargo test`
预期：全部单元测试 PASS。

运行：`python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")" 2>&1 | tail -5`
预期：130/130 PASS。

运行：`python -c "import scripts.hudlib.cases"` 无需执行——断言数由 cases.py 内部 assert 保证（上述全量运行已覆盖）。

- [ ] **Step 7: 提交（用户手动执行）**

```bash
git add COMPLETE.md DEPLOY.md CHANGELOG.md README.md
git commit -m "docs: 批次 C 剩余（⑨⑩⑪⑮⑯⑰⑱）交付回写 — COMPLETE/DEPLOY/CHANGELOG/README"
```
