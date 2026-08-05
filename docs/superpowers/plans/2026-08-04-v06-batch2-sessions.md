# 批次 II — 会话复盘与浏览（⑤⑥⑦）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ⑤ `sessions` 分页列表命令 + ⑥ `session <id>` 单会话详情（transcript 尾读补工具明细）+ ⑦ 工具级成本归因排行（估算路径，挂详情段）。

**Architecture:** history.rs 加列 migration（model/transcript_path，PRAGMA table_info → ALTER TABLE ADD COLUMN，旧库自动兼容）+ 两个新查询（sessions_page / session_by_id）；main.rs 两个新子命令（clap + i18n 全接线）；pricing.rs 加纯函数 `tool_cost_ranking`（无逐工具 token → per_call 均摊估算，`≈` 标注，未命中 → None）；run_session 尾读 transcript（TranscriptReader 增量解析器复用）补 token 分解/代理列表/排行段。

**Tech Stack:** Rust 2021 · rusqlite · serde · clap 4.6 · i18n（locales/en|zh.toml）· 黑盒 harness（scripts/hudlib + test_hud.py）

**用户约束**（本仓库硬性规则）：不自动 `git add/commit/push`（批次末 AskUserQuestion 授权，不带 Co-Authored-By）；不运行 `cargo fmt`；cargo 不在 PATH（命令统一前缀 `export PATH="$HOME/.cargo/bin:$PATH" &&`）；不 stage 未跟踪的 `fixtures/`、`reports/`、`docs/superpowers/`；构建用 `cargo build`（黑盒 harness 从 target/debug/claude-hud.exe 解析）。

---

## 事实基线（本批次全部已验证）

1. **history.db schema**（history.rs:54-70）：`sessions` 表列 = id/started_at/duration_secs/total_cost_usd/total_tokens/agent_count/lines_added/lines_removed/mod_used。**无 model / 无 transcript_path 列**——spec 任务⑥声称"transcript_path 已入库"与事实不符，本批次以 migration 补齐（任务⑥ 方案点 2 依赖它尾读）。
2. **SessionRecord**（history.rs:12-19）：id/started_at/duration_secs/total_cost_usd/total_tokens/agent_count。
3. **结账调用点**（compact.rs:204 + dashboard.rs:175）：`record_session(&SessionData, agent_count)`——SessionData 有 `model.id`（session.rs:6）与 `transcript_path: Option<String>`（session.rs:12），加列直接可取。
4. **history 命令**（main.rs:900-956）：`println!` + `tr()`；空库 `—`；`format_history_duration`（≥60s "Nm"）/`format_history_tokens`（≥1000 "Nk"）复用；错误出口统一 `Err → eprintln(runtime.err) + exit(1)`（main.rs:216-219）。
5. **clap i18n**（main.rs:225-271）：`inject_help` 用 `.mut_subcommand("name", |c| c.about(tr(...)).mut_arg(...))`；dispatch 在 `run()` 的 match（main.rs:206 先例 `Commands::History { weekly } => ...`）。
6. **i18n**：`[runtime]` 段 h_* 系列（en.toml:140-146）、`[cli]` 段子命令帮助（en.toml:184-215）；`h_session_line = "  #{id}  {start}  {sym}{cost}  {dur}  {n} agents  {tok}"` 可直接复用为 sessions 列表行；zh 是 en 子集，`tr()` 回退链。
7. **任务⑦ 数据源确认**：`token_attribution()`（transcript.rs:487-499）只按代理工具调用数占比（启发式），**无逐工具 token** → 走 spec 方案 B 估算：`tool_counts × 单价 × 平均 token/调用`。`tool_counts: HashMap<String, usize>`（transcript.rs:13）在 ToolUse 分支无条件 +1（transcript.rs:355-356）。transcript 总 token 在 `summary.total_tokens`（input/output/cache_read/cache_created）。
8. **transcript 尾读**：`TranscriptReader::new(path)` + `read_updates()` 从偏移 0 读全文件（new 时 last_pos=0）→ summary 含 tool_counts/total_tokens/agents。复用增量解析器满足任务⑥ 方案点 2。
9. **内置定价**（pricing.rs:30-54）：9 个 claude 模型；`merged_pricing(config)`（57-63）唯一查询入口；`PriceEntry{input,output,cache_read,cache_creation}`。**deepseek-v4-flash 不在内置表** → 黑盒命中用例必须用 `model.id = "claude-sonnet-4-6"`（3e-6 / 15e-6 / 0.3e-6 / 3.75e-6）。
10. **harness CLI 测试**（cases.py:943-959 先例）：`args=["sessions"]` 覆盖默认 render；`pre_cmds=[{"args":["render"], "stdin": ...}]` 建数据；`remove_db=True` 清库；zh 用 `env_extra={"CLAUDE_HUD_CONFIG": fx("config/i18n_zh.toml")}` 或 inline config；断言支持 stdout_contains/stdout_not_contains/stdout_regex/stderr_contains/stderr_empty/exit（assertions.py:9-72）。
11. **结账触发**：transcript_path 变化才结账（compact.rs:190-213）——首个 render 不结账，A→B→C 三次 render 结账 2 条。
12. **fixture**：`timestamps.jsonl` = alpha + Bash + Read + 2×assistant（input 300 output 130，可靠时间轴）；`agents.jsonl` = Bash 1 + Skill 1 + assistant 500/250（首行有 ts，但第二条无 ts——注意：first_line_has_ts 只看首行 → agents.jsonl 首行有 ts → 可靠！供详情用例验证）；`token_rate.jsonl` = 只有 assistant（零工具调用）；`valid.jsonl` = Bash 1 + assistant 130 总 input（100+30）+ cache 字段。

---

## 任务 1（⑤）：`sessions` 列表命令

**Files:**
- Modify: `src/core/history.rs`（sessions_page 查询 + 单测）
- Modify: `src/main.rs`（Commands::Sessions + dispatch + run_sessions + inject_help）
- Modify: `locales/en.toml`、`locales/zh.toml`（cli.sessions* + runtime.h_sessions_title）
- Modify: `scripts/hudlib/cases.py`（b2_cases）

- [ ] **Step 1: 写失败测试**（history.rs tests 模块追加）

```rust
    #[test]
    fn sessions_page_orders_desc_and_limits() {
        let store = mem_store();
        store.record_session(&session(1.0, 1000, 500, 60_000), 1).unwrap();
        store.record_session(&session(2.0, 2000, 800, 120_000), 2).unwrap();
        store.record_session(&session(3.0, 3000, 900, 180_000), 3).unwrap();
        let page = store.sessions_page(2, 0, None).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].id, 3);
        assert_eq!(page[1].id, 2);
    }

    #[test]
    fn sessions_page_offset_skips() {
        let store = mem_store();
        store.record_session(&session(1.0, 1000, 500, 60_000), 1).unwrap();
        store.record_session(&session(2.0, 2000, 800, 120_000), 2).unwrap();
        let page = store.sessions_page(10, 1, None).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].id, 1);
    }

    #[test]
    fn sessions_page_date_filter() {
        let store = mem_store();
        store.record_session(&session(1.0, 1000, 500, 60_000), 1).unwrap();
        // started_at 默认 now；把两条会话改成不同日期再过滤
        store
            .conn
            .execute("UPDATE sessions SET started_at = '2026-08-01 10:00:00' WHERE id = 1", [])
            .unwrap();
        store.record_session(&session(2.0, 2000, 800, 120_000), 2).unwrap();
        store
            .conn
            .execute("UPDATE sessions SET started_at = '2026-08-04 10:00:00' WHERE id = 2", [])
            .unwrap();
        let page = store.sessions_page(10, 0, Some("2026-08-03")).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].id, 2);
        // 未来日期 → 空
        let none = store.sessions_page(10, 0, Some("2099-01-01")).unwrap();
        assert!(none.is_empty());
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test sessions_page 2>&1 | tail -6`
Expected: 编译错误 `no method named sessions_page`（RED）。

- [ ] **Step 3: 实现 sessions_page**（history.rs，recent_sessions 之后）

```rust
    /// ⑤ sessions 分页列表：id 降序，可选起始日期过滤（YYYY-MM-DD 前缀比较）。
    pub fn sessions_page(
        &self,
        limit: usize,
        offset: usize,
        date_from: Option<&str>,
    ) -> Result<Vec<SessionRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, started_at, duration_secs, total_cost_usd, total_tokens, agent_count
                 FROM sessions
                 WHERE (?1 IS NULL OR started_at >= ?1)
                 ORDER BY id DESC LIMIT ?2 OFFSET ?3",
            )
            .map_err(|e| format!("prepare: {}", e))?;
        let rows = stmt
            .query_map(
                rusqlite::params![date_from, limit as i64, offset as i64],
                |row| {
                    Ok(SessionRecord {
                        id: row.get(0)?,
                        started_at: row.get(1)?,
                        duration_secs: row.get(2)?,
                        total_cost_usd: row.get(3)?,
                        total_tokens: row.get::<_, i64>(4)? as u64,
                        agent_count: row.get::<_, i64>(5)? as usize,
                    })
                },
            )
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }
```

- [ ] **Step 4: 运行确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test sessions_page 2>&1 | tail -4`
Expected: 3 个测试 PASS。

- [ ] **Step 5: CLI 接线**（main.rs）

Commands enum（`History` 变体后）：

```rust
    /// List recorded sessions (paginated)
    Sessions {
        /// Maximum number of sessions to list
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Skip the first N sessions
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Only sessions started on or after this date (YYYY-MM-DD)
        #[arg(long)]
        date: Option<String>,
    },
```

dispatch（`Commands::History { weekly }` 后）：

```rust
        Commands::Sessions { limit, offset, date } => {
            run_sessions(&config, limit, offset, date.as_deref(), lang)
        }
```

inject_help（history 块后）：

```rust
        .mut_subcommand("sessions", |c| {
            c.about(tr(lang, "cli.sessions"))
                .mut_arg("limit", |a| a.help(tr(lang, "cli.sessions_limit")))
                .mut_arg("offset", |a| a.help(tr(lang, "cli.sessions_offset")))
                .mut_arg("date", |a| a.help(tr(lang, "cli.sessions_date")))
        })
```

run_sessions（run_history 之后；行格式复用 h_session_line，与 history 列表口径一致但带分页/过滤）：

```rust
/// ⑤ `sessions`：分页会话列表。空库显示 —；行格式与 history 列表一致。
fn run_sessions(
    config: &AppConfig,
    limit: usize,
    offset: usize,
    date_from: Option<&str>,
    lang: crate::core::i18n::Language,
) -> Result<(), String> {
    let store = HistoryStore::open()?;
    println!("{}", tr(lang, "runtime.h_sessions_title"));
    let symbol = &config.currency_symbol;
    let rows = store.sessions_page(limit, offset, date_from)?;
    if rows.is_empty() {
        println!("  —");
    } else {
        for r in rows {
            println!(
                "{}",
                tr(lang, "runtime.h_session_line")
                    .replace("{id}", &r.id.to_string())
                    .replace("{start}", &r.started_at)
                    .replace("{sym}", symbol)
                    .replace("{cost}", &format!("{:.2}", r.total_cost_usd))
                    .replace("{dur}", &format_history_duration(r.duration_secs))
                    .replace("{n}", &r.agent_count.to_string())
                    .replace("{tok}", &format_history_tokens(r.total_tokens))
            );
        }
    }
    Ok(())
}
```

- [ ] **Step 6: i18n keys**

`locales/en.toml` `[runtime]` 段（h_daily 后）：

```toml
h_sessions_title = "Sessions:"
```

`locales/en.toml` `[cli]` 段（history_weekly 后）：

```toml
sessions = "List recorded sessions (paginated)"
sessions_limit = "Maximum number of sessions to list"
sessions_offset = "Skip the first N sessions"
sessions_date = "Only sessions started on or after this date (YYYY-MM-DD)"
```

`locales/zh.toml` `[runtime]`（h_daily 对应位置后）：

```toml
h_sessions_title = "会话列表："
```

`locales/zh.toml` `[cli]`（history_weekly 后）：

```toml
sessions = "列出已记录的会话（分页）"
sessions_limit = "最多列出条数"
sessions_offset = "跳过前 N 条"
sessions_date = "仅列出此日期（YYYY-MM-DD）之后开始的会话"
```

- [ ] **Step 7: 全量单测**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -2`
Expected: 全量 PASS（170 + 3 = 173）。

- [ ] **Step 8: 黑盒用例**（cases.py，b5_cases 后）

```python
def b2_cases():
    """批次 II ⑤：sessions 分页列表。A→B→C 三次 render 结账 2 条。"""
    list_cfg = DEFAULT_CONFIG
    pre3 = [
        {"args": ["render"], "stdin": j(full_dict(**{"transcript_path": "/a.jsonl"}))},
        {"args": ["render"], "stdin": j(full_dict(**{"transcript_path": "/b.jsonl"}))},
        {"args": ["render"], "stdin": j(full_dict(**{"transcript_path": "/c.jsonl"}))},
    ]
    return [
        render_case(
            "B2-01", "⑤ sessions 列表（2 条结账）", "batch2",
            {"exit": 0, "stdout_contains": ["Sessions:", "#2", "#1", "agents"]},
            args=["sessions"], config=list_cfg, pre_cmds=pre3,
            remove_db=True, remove_state=True,
            note="A→B→C 结账 2 条 → 列表 id 降序 #2 #1"),
        render_case(
            "B2-02", "⑤ sessions 分页 --limit 1", "batch2",
            {"exit": 0, "stdout_contains": ["#2"], "stdout_not_contains": ["#1"]},
            args=["sessions", "--limit", "1"], config=list_cfg, pre_cmds=pre3,
            remove_db=True, remove_state=True,
            note="limit=1 → 只含最新 #2"),
        render_case(
            "B2-03", "⑤ sessions 日期过滤无命中", "batch2",
            {"exit": 0, "stdout_contains": ["—"]},
            args=["sessions", "--date", "2099-01-01"], config=list_cfg, pre_cmds=pre3,
            remove_db=True, remove_state=True,
            note="未来日期 → 空列表显示 —（started_at 为真实 now，2026-08 起）"),
        render_case(
            "B2-04", "⑤ sessions 空库 —", "batch2",
            {"exit": 0, "stdout_contains": ["Sessions:", "—"]},
            args=["sessions"], config=DEFAULT_CONFIG,
            remove_db=True,
            note="空库列表显示 —"),
        render_case(
            "B2-05", "⑤ sessions zh 表头", "batch2",
            {"exit": 0, "stdout_contains": ["会话列表："]},
            args=["sessions"], config=DEFAULT_CONFIG,
            env_extra={"CLAUDE_HUD_CONFIG": fx("config/i18n_zh.toml")},
            remove_db=True,
            note="zh locale 标题"),
    ]
```

CASES 追加 `+ b2_cases()`，总数 168 → 173：

```python
# 156 + 3（B1-01..03）+ 1（B2-01）+ 2（B3-01/02）+ 3（B4-01/02/03）+ 3（B5-01/02/03）
#   + 5（B2-01..05 sessions 列表）= 173
assert len(CASES) == 173, f"expected 173 cases, got {len(CASES)}"
```

- [ ] **Step 9: 全量验证**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1 | tail -3 && python scripts/test_hud.py 2>&1 | tail -4`
Expected: 0 warning；黑盒 173/173 PASS（P4-01/P5-05/P6-01 的 history 回归天然覆盖）。

---

## 任务 2（⑥）：`session <id>` 详情命令

**Files:**
- Modify: `src/core/history.rs`（migration + SessionRecord 字段 + record_session 写列 + session_by_id + 单测）
- Modify: `src/main.rs`（Commands::Session + dispatch + run_session + inject_help）
- Modify: `locales/en.toml`、`locales/zh.toml`（cli.session* + runtime.h_session_*）
- Modify: `scripts/hudlib/cases.py`（b2_cases 追加详情用例）

- [ ] **Step 1: 写失败测试**（history.rs tests 追加）

```rust
    #[test]
    fn session_by_id_found_and_missing() {
        let store = mem_store();
        store.record_session(&session(1.5, 1000, 500, 60_000), 2).unwrap();
        let found = store.session_by_id(1).unwrap().expect("id 1 exists");
        assert_eq!(found.id, 1);
        assert!((found.total_cost_usd - 1.5).abs() < 1e-9);
        assert_eq!(found.agent_count, 2);
        assert!(store.session_by_id(99).unwrap().is_none());
    }

    #[test]
    fn record_session_stores_model_and_transcript_path() {
        let store = mem_store();
        let mut d = session(1.0, 1000, 500, 60_000);
        d.model.id = "claude-sonnet-4-6".into();
        d.transcript_path = Some("/tmp/x.jsonl".into());
        store.record_session(&d, 1).unwrap();
        let r = store.session_by_id(1).unwrap().expect("recorded");
        assert_eq!(r.model, "claude-sonnet-4-6");
        assert_eq!(r.transcript_path.as_deref(), Some("/tmp/x.jsonl"));
    }

    #[test]
    fn legacy_schema_migrates_new_columns() {
        // 旧库：sessions 表无 model/transcript_path 列 → init_schema 后列补齐
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                started_at TEXT NOT NULL DEFAULT (datetime('now')),
                duration_secs INTEGER NOT NULL DEFAULT 0,
                total_cost_usd REAL NOT NULL DEFAULT 0.0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                agent_count INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        let store = HistoryStore { conn };
        store.init_schema().unwrap();
        store.record_session(&session(1.0, 1000, 500, 60_000), 1).unwrap();
        let r = store.session_by_id(1).unwrap().expect("works after migrate");
        assert_eq!(r.model, "");
        assert!(r.transcript_path.is_none());
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test session_by_id 2>&1 | tail -6`
Expected: 编译错误（`no method named session_by_id`；`SessionRecord` 无 model 字段）（RED）。

- [ ] **Step 3: 实现 migration + 字段 + 查询**

history.rs `SessionRecord` 追加字段（agent_count 后）：

```rust
    pub model: String,
    pub transcript_path: Option<String>,
```

（`#[derive(Debug, Clone)]` 保持——SessionRecord 无 Default 派生，两个新增字段只需在查询映射处赋值。）

`init_schema` 末尾追加 migration（CREATE TABLE 之后）：

```rust
        self.migrate()?;
        Ok(())
    }

    /// 旧库补列（model / transcript_path）：PRAGMA 检查 → ALTER ADD COLUMN。
    /// 新库 CREATE TABLE 已含新列，此路径为空操作。
    fn migrate(&self) -> Result<(), String> {
        let cols: Vec<String> = self
            .conn
            .prepare("PRAGMA table_info(sessions)")
            .map_err(|e| format!("pragma: {}", e))?
            .query_map([], |row| row.get(1))
            .map_err(|e| format!("pragma row: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        if !cols.iter().any(|c| c == "model") {
            self.conn
                .execute_batch(
                    "ALTER TABLE sessions ADD COLUMN model TEXT NOT NULL DEFAULT ''",
                )
                .map_err(|e| format!("add model column: {}", e))?;
        }
        if !cols.iter().any(|c| c == "transcript_path") {
            self.conn
                .execute_batch(
                    "ALTER TABLE sessions ADD COLUMN transcript_path TEXT NOT NULL DEFAULT ''",
                )
                .map_err(|e| format!("add transcript_path column: {}", e))?;
        }
        Ok(())
    }
```

`init_schema` 的 CREATE TABLE 追加两列（agent_count 行后）：

```sql
                    agent_count INTEGER NOT NULL DEFAULT 0,
                    model TEXT NOT NULL DEFAULT '',
                    transcript_path TEXT NOT NULL DEFAULT ''
```

（注意既有 CREATE TABLE IF NOT EXISTS 含新列 → 新库一步到位；旧库走 migrate。）

`record_session` INSERT 补两列（agent_count 后）：

```rust
            .execute(
                "INSERT INTO sessions (duration_secs, total_cost_usd, total_tokens, agent_count,
                                       model, transcript_path)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    dur_secs,
                    data.cost.total_cost_usd,
                    total_tokens,
                    agent_count,
                    data.model.id,
                    data.transcript_path.as_deref().unwrap_or("")
                ],
            )
```

`recent_sessions` 与 `sessions_page` 的 SELECT 补两列 + 映射：

```rust
                "SELECT id, started_at, duration_secs, total_cost_usd, total_tokens, agent_count,
                        model, transcript_path
                 FROM sessions ORDER BY id DESC LIMIT ?1",
```

```rust
                Ok(SessionRecord {
                    id: row.get(0)?,
                    started_at: row.get(1)?,
                    duration_secs: row.get(2)?,
                    total_cost_usd: row.get(3)?,
                    total_tokens: row.get::<_, i64>(4)? as u64,
                    agent_count: row.get::<_, i64>(5)? as usize,
                    model: row.get(6)?,
                    transcript_path: {
                        let s: String = row.get(7)?;
                        if s.is_empty() { None } else { Some(s) }
                    },
                })
```

（sessions_page 的 SELECT 同样补两列 + 同映射。）

`session_by_id`（session_page 之后）：

```rust
    /// ⑥ 单会话详情：按主键查，无 → None。
    pub fn session_by_id(&self, id: i64) -> Result<Option<SessionRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, started_at, duration_secs, total_cost_usd, total_tokens, agent_count,
                        model, transcript_path
                 FROM sessions WHERE id = ?1",
            )
            .map_err(|e| format!("prepare: {}", e))?;
        let mut rows = stmt
            .query_map([id], |row| {
                Ok(SessionRecord {
                    id: row.get(0)?,
                    started_at: row.get(1)?,
                    duration_secs: row.get(2)?,
                    total_cost_usd: row.get(3)?,
                    total_tokens: row.get::<_, i64>(4)? as u64,
                    agent_count: row.get::<_, i64>(5)? as usize,
                    model: row.get(6)?,
                    transcript_path: {
                        let s: String = row.get(7)?;
                        if s.is_empty() { None } else { Some(s) }
                    },
                })
            })
            .map_err(|e| format!("query: {}", e))?;
        rows.next().transpose().map_err(|e| format!("row: {}", e))
    }
```

- [ ] **Step 4: 运行确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test history 2>&1 | tail -4`
Expected: history 全部 PASS（既有 4 + 新增 3 = 7）。

- [ ] **Step 5: CLI 接线**（main.rs）

Commands enum（Sessions 后）：

```rust
    /// Show details for a single session
    Session {
        /// Session id
        id: String,
    },
```

dispatch：

```rust
        Commands::Session { id } => run_session(&config, &id, lang),
```

inject_help：

```rust
        .mut_subcommand("session", |c| {
            c.about(tr(lang, "cli.session"))
                .mut_arg("id", |a| a.help(tr(lang, "cli.session_id")))
        })
```

run_session（run_sessions 后；`use crate::core::transcript::TranscriptReader;` 与 `use crate::core::pricing::merged_pricing;` 顶部已存在与否编译时确认，必要时用全路径）：

```rust
/// ⑥ `session <id>`：单会话详情。transcript_path 存在 → 尾读补充
/// token 分解/代理列表/工具成本排行；未找到 → 明确报错（exit 1）。
fn run_session(
    config: &AppConfig,
    id: &str,
    lang: crate::core::i18n::Language,
) -> Result<(), String> {
    let store = HistoryStore::open()?;
    let sid: i64 = id.parse().map_err(|_| {
        tr(lang, "runtime.h_session_not_found").replace("{id}", id)
    })?;
    let Some(r) = store.session_by_id(sid)? else {
        return Err(tr(lang, "runtime.h_session_not_found").replace("{id}", id));
    };
    let symbol = &config.currency_symbol;
    println!("{}", tr(lang, "runtime.h_session_title").replace("{id}", &r.id.to_string()));
    println!(
        "{}",
        tr(lang, "runtime.h_session_model").replace("{model}", &r.model)
    );
    println!(
        "{}",
        tr(lang, "runtime.h_session_cost")
            .replace("{sym}", symbol)
            .replace("{cost}", &format!("{:.2}", r.total_cost_usd))
    );
    println!(
        "{}",
        tr(lang, "runtime.h_session_duration")
            .replace("{dur}", &format_history_duration(r.duration_secs))
    );
    println!(
        "{}",
        tr(lang, "runtime.h_session_agents").replace("{n}", &r.agent_count.to_string())
    );
    let tokens = format_history_tokens(r.total_tokens);
    let summary = match r.transcript_path.as_deref() {
        Some(path) if std::path::Path::new(path).exists() => {
            Some(crate::core::transcript::TranscriptReader::new(path.into()).read_updates())
        }
        _ => None,
    };
    match &summary {
        Some(s) => {
            println!(
                "{}",
                tr(lang, "runtime.h_session_tokens")
                    .replace("{tok}", &tokens)
                    .replace("{in}", &s.total_tokens.input.to_string())
                    .replace("{out}", &s.total_tokens.output.to_string())
            );
            println!("{}", tr(lang, "runtime.h_session_agent_list"));
            for a in &s.agents {
                println!(
                    "{}",
                    tr(lang, "runtime.h_session_agent_line")
                        .replace("{name}", &a.name)
                        .replace("{calls}", &a.tool_calls.to_string())
                );
            }
        }
        None => {
            println!(
                "{}",
                tr(lang, "runtime.h_session_tokens_plain").replace("{tok}", &tokens)
            );
        }
    }
    println!("{}", tr(lang, "runtime.h_tools_title"));
    match summary.as_ref().and_then(|s| {
        crate::core::pricing::tool_cost_ranking(
            s,
            &crate::core::pricing::merged_pricing(config),
            &r.model,
        )
    }) {
        Some(rows) if !rows.is_empty() => {
            for (tool, calls, cost) in rows.iter().take(5) {
                println!(
                    "{}",
                    tr(lang, "runtime.h_tool_line")
                        .replace("{tool}", tool)
                        .replace("{n}", &calls.to_string())
                        .replace("{sym}", symbol)
                        .replace("{cost}", &format!("{:.2}", cost))
                );
            }
        }
        _ => println!("{}", tr(lang, "runtime.h_tools_empty")),
    }
    Ok(())
}
```

- [ ] **Step 6: i18n keys**

`locales/en.toml` `[runtime]`（h_sessions_title 后）：

```toml
h_session_title = "Session #{id}:"
h_session_not_found = "Session {id} not found"
h_session_model = "  Model: {model}"
h_session_cost = "  Cost: {sym}{cost}"
h_session_duration = "  Duration: {dur}"
h_session_tokens = "  Tokens: {tok} ({in} in / {out} out)"
h_session_tokens_plain = "  Tokens: {tok}"
h_session_agents = "  Agents: {n}"
h_session_agent_list = "  Agents:"
h_session_agent_line = "    {name} · {calls} calls"
h_tools_title = "  Tools (est.):"
h_tool_line = "    {tool} · {n} calls · ≈{sym}{cost}"
h_tools_empty = "    —"
```

`locales/en.toml` `[cli]`（sessions_date 后）：

```toml
session = "Show details for a single session"
session_id = "Session id"
```

`locales/zh.toml` `[runtime]`：

```toml
h_session_title = "会话 #{id}："
h_session_not_found = "会话 {id} 不存在"
h_session_model = "  模型：{model}"
h_session_cost = "  成本：{sym}{cost}"
h_session_duration = "  时长：{dur}"
h_session_tokens = "  Token：{tok}（输入 {in} / 输出 {out}）"
h_session_tokens_plain = "  Token：{tok}"
h_session_agents = "  代理数：{n}"
h_session_agent_list = "  代理："
h_session_agent_line = "    {name} · {calls} 次调用"
h_tools_title = "  工具（估算）："
h_tool_line = "    {tool} · {n} 次调用 · ≈{sym}{cost}"
h_tools_empty = "    —"
```

`locales/zh.toml` `[cli]`：

```toml
session = "查看单个会话详情"
session_id = "会话 ID"
```

- [ ] **Step 7: 全量单测**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -2`
Expected: 全量 PASS（173 + 3 = 176）。

- [ ] **Step 8: 黑盒用例**（b2_cases 追加，注意 remove_state 隔离）：

```python
    detail_cfg = DEFAULT_CONFIG
    pre_detail = [
        {"args": ["render"],
         "stdin": j(full_dict(**{"model": {"id": "claude-sonnet-4-6", "display_name": "Sonnet"},
                                  "transcript_path": fx("transcript/timestamps.jsonl")}))},
        {"args": ["render"],
         "stdin": j(full_dict(**{"transcript_path": "/b.jsonl"}))},
    ]
```

追加用例（b2_cases 返回列表尾部，注意 `#2` 冲突——详情用例断言用 `#1`）：

```python
        render_case(
            "B2-06", "⑥ session 详情（模型 + transcript 尾读明细）", "batch2",
            {"exit": 0,
             "stdout_contains": ["Session #1:", "Model: claude-sonnet-4-6",
                                 "Agents:", "alpha", "Tools (est.):",
                                 "Bash · 1 calls", "Read · 1 calls"]},
            args=["session", "1"], config=detail_cfg, pre_cmds=pre_detail,
            remove_db=True, remove_state=True,
            note="A(timestamps)→B 结账 A：模型入库 + 尾读工具明细"),
        render_case(
            "B2-07", "⑥ session 不存在 → exit 1", "batch2",
            {"exit": 1, "stderr_contains": ["Session 99 not found"]},
            args=["session", "99"], config=DEFAULT_CONFIG,
            pre_cmds=pre_detail,
            remove_db=True, remove_state=True,
            note="未找到 id → 明确报错（exit 1 + stderr）"),
        render_case(
            "B2-08", "⑥ session 空库 → exit 1", "batch2",
            {"exit": 1, "stderr_contains": ["Session 1 not found"]},
            args=["session", "1"], config=DEFAULT_CONFIG,
            remove_db=True,
            note="空库详情 → 未找到报错"),
        render_case(
            "B2-09", "⑥ session 无 transcript → 简化详情", "batch2",
            {"exit": 0,
             "stdout_contains": ["Session #1:", "Tokens:",
                                 "Tools (est.):", "—"],
             "stdout_not_contains": ["Agents:"]},
            args=["session", "1"], config=DEFAULT_CONFIG,
            pre_cmds=[
                {"args": ["render"],
                 "stdin": j(full_dict(**{"model": {"id": "claude-sonnet-4-6", "display_name": "Sonnet"},
                                          "transcript_path": "/a.jsonl"}))},
                {"args": ["render"],
                 "stdin": j(full_dict(**{"transcript_path": "/b.jsonl"}))},
            ],
            remove_db=True, remove_state=True,
            note="transcript_path /a.jsonl 不存在 → 无尾读：Token 总量 + 排行 —"),
]
```

CASES 总数 173 → 177。

- [ ] **Step 9: 全量验证**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1 | tail -3 && python scripts/test_hud.py 2>&1 | tail -4`
Expected: 0 warning；黑盒 177/177 PASS。

---

## 任务 3（⑦）：工具级成本归因排行

**Files:**
- Modify: `src/core/pricing.rs`（tool_cost_ranking 纯函数 + 单测）
- Modify: `src/main.rs`（run_session 排行段——任务 2 Step 5 已含调用，本任务实现函数后编译通过）
- Create: `fixtures/transcript/tools.jsonl`（新 fixture，不 stage）
- Modify: `scripts/hudlib/cases.py`（b2_cases 追加排行用例）

- [ ] **Step 1: 写失败测试**（pricing.rs tests 追加）

```rust
    fn summary_with_tools(tools: &[(&str, usize)], input: u64, output: u64) -> TranscriptSummary {
        let mut s = TranscriptSummary::default();
        s.tool_counts = tools
            .iter()
            .map(|(t, n)| (t.to_string(), *n))
            .collect();
        s.total_tokens = TokenTotal {
            input,
            output,
            cache_read: 0,
            cache_created: 0,
        };
        s
    }

    #[test]
    fn ranking_sorts_desc_and_estimates() {
        // Bash 3 + Read 2 + Skill 1 = 6 calls；input 600k output 300k；
        // sonnet 价（3e-6/15e-6）→ 总成本 1.8 + 4.5 = 6.3 → per_call 1.05
        let s = summary_with_tools(&[("Bash", 3), ("Read", 2), ("Skill", 1)], 600_000, 300_000);
        let mut pricing = PricingTable::new();
        pricing.insert(
            "claude-sonnet-4-6".into(),
            PriceEntry {
                input: 3e-6,
                output: 15e-6,
                ..Default::default()
            },
        );
        let rows = tool_cost_ranking(&s, &pricing, "claude-sonnet-4-6")
            .expect("model priced");
        assert_eq!(rows.len(), 3);
        // 降序：Bash 3.15 > Read 2.10 > Skill 1.05
        assert_eq!(rows[0].0, "Bash");
        assert_eq!(rows[0].1, 3);
        assert!((rows[0].2 - 3.15).abs() < 1e-9);
        assert_eq!(rows[1].0, "Read");
        assert!((rows[1].2 - 2.10).abs() < 1e-9);
        assert_eq!(rows[2].0, "Skill");
        assert!((rows[2].2 - 1.05).abs() < 1e-9);
    }

    #[test]
    fn ranking_unknown_model_returns_none() {
        let s = summary_with_tools(&[("Bash", 3)], 600_000, 300_000);
        let rows = tool_cost_ranking(&s, &PricingTable::new(), "deepseek-v4-flash");
        assert!(rows.is_none());
    }

    #[test]
    fn ranking_zero_calls_returns_empty() {
        let s = summary_with_tools(&[], 600_000, 300_000);
        let rows = tool_cost_ranking(&s, &builtin_pricing(), "claude-sonnet-4-6");
        assert_eq!(rows, Some(vec![]));
    }

    #[test]
    fn ranking_zero_tokens_returns_empty() {
        let s = summary_with_tools(&[("Bash", 3)], 0, 0);
        let rows = tool_cost_ranking(&s, &builtin_pricing(), "claude-sonnet-4-6");
        assert_eq!(rows, Some(vec![]));
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test ranking 2>&1 | tail -6`
Expected: 编译错误 `cannot find function tool_cost_ranking`（RED）。

- [ ] **Step 3: 实现 tool_cost_ranking**（pricing.rs，realtime_cost 后）

```rust
/// ⑦ 工具级成本归因排行（估算路径：无逐工具 token）——
/// per_call = (input×in_p + output×out_p + cache_read×cr_p + cache_creation×cc_p)
/// ÷ 总调用数；tool[t] = per_call × calls[t]，成本降序。
/// 模型未命中定价 → None（该段 `—`）；零调用或零 token → Some(空)。
pub fn tool_cost_ranking(
    summary: &TranscriptSummary,
    pricing: &PricingTable,
    model_id: &str,
) -> Option<Vec<(String, usize, f64)>> {
    let price = pricing.get(model_id)?;
    let total_calls: usize = summary.tool_counts.values().sum();
    if total_calls == 0 {
        return Some(vec![]);
    }
    let t = &summary.total_tokens;
    let total_cost = price.input * t.input as f64
        + price.output * t.output as f64
        + price.cache_read * t.cache_read as f64
        + price.cache_creation * t.cache_created as f64;
    if total_cost <= 0.0 {
        return Some(vec![]);
    }
    let per_call = total_cost / total_calls as f64;
    let mut rows: Vec<(String, usize, f64)> = summary
        .tool_counts
        .iter()
        .map(|(tool, calls)| (tool.clone(), *calls, per_call * *calls as f64))
        .collect();
    rows.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    Some(rows)
}
```

- [ ] **Step 4: 运行确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test ranking 2>&1 | tail -4`
Expected: 4 个测试 PASS。

- [ ] **Step 5: 新 fixture tools.jsonl**（Create，不 stage）

```
{"type":"tool_use","name":"Bash","input":{},"timestamp":"2026-07-31T10:01:00Z"}
{"type":"tool_use","name":"Bash","input":{},"timestamp":"2026-07-31T10:01:30Z"}
{"type":"tool_use","name":"Bash","input":{},"timestamp":"2026-07-31T10:02:00Z"}
{"type":"tool_use","name":"Read","input":{},"timestamp":"2026-07-31T10:02:30Z"}
{"type":"tool_use","name":"Read","input":{},"timestamp":"2026-07-31T10:03:00Z"}
{"type":"tool_use","name":"Skill","input":{},"timestamp":"2026-07-31T10:03:30Z"}
{"type":"assistant","message":{"usage":{"input_tokens":600000,"output_tokens":300000}},"timestamp":"2026-07-31T10:04:00Z"}
```

（Bash 3 / Read 2 / Skill 1，input 600k output 300k → sonnet 价下 Bash ≈$3.15 居首。）

- [ ] **Step 6: 黑盒用例**（b2_cases 追加）：

```python
    pre_rank = [
        {"args": ["render"],
         "stdin": j(full_dict(**{"model": {"id": "claude-sonnet-4-6", "display_name": "Sonnet"},
                                  "transcript_path": fx("transcript/tools.jsonl")}))},
        {"args": ["render"],
         "stdin": j(full_dict(**{"transcript_path": "/b.jsonl"}))},
    ]
```

```python
        render_case(
            "B2-10", "⑦ 工具成本排行降序（tools fixture）", "batch2",
            {"exit": 0,
             "stdout_contains": ["Tools (est.):",
                                 "Bash · 3 calls · ≈$3.15",
                                 "Read · 2 calls · ≈$2.10"],
             "stdout_regex": r"Bash · 3 calls[\s\S]*Read · 2 calls"},
            args=["session", "1"], config=DEFAULT_CONFIG,
            pre_cmds=pre_rank,
            remove_db=True, remove_state=True,
            note="600k/300k ÷ 6 calls → per_call 1.05；Bash 3.15 居首（regex 断言顺序）"),
        render_case(
            "B2-11", "⑦ 零工具 transcript → 排行 —", "batch2",
            {"exit": 0,
             "stdout_contains": ["Tools (est.):", "—"],
             "stdout_not_contains": ["calls"]},
            args=["session", "1"], config=DEFAULT_CONFIG,
            pre_cmds=[
                {"args": ["render"],
                 "stdin": j(full_dict(**{"model": {"id": "claude-sonnet-4-6", "display_name": "Sonnet"},
                                          "transcript_path": fx("transcript/token_rate.jsonl")}))},
                {"args": ["render"],
                 "stdin": j(full_dict(**{"transcript_path": "/b.jsonl"}))},
            ],
            remove_db=True, remove_state=True,
            note="token_rate.jsonl 只有 assistant 无工具 → 零调用 → —"),
        render_case(
            "B2-12", "⑦ 模型未命中定价 → 排行 —", "batch2",
            {"exit": 0,
             "stdout_contains": ["Tools (est.):", "—"],
             "stdout_not_contains": ["calls"]},
            args=["session", "1"], config=DEFAULT_CONFIG,
            pre_cmds=[
                {"args": ["render"],
                 "stdin": j(full_dict(**{"model": {"id": "deepseek-v4-flash", "display_name": "DeepSeek"},
                                          "transcript_path": fx("transcript/tools.jsonl")}))},
                {"args": ["render"],
                 "stdin": j(full_dict(**{"transcript_path": "/b.jsonl"}))},
            ],
            remove_db=True, remove_state=True,
            note="deepseek-v4-flash 不在内置表 → 未命中 → —（诚实降级）"),
]
```

CASES 总数 177 → 180。

- [ ] **Step 7: 全量验证**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -2`
Expected: 全量 PASS（176 + 4 = 180）。
Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1 | tail -3 && python scripts/test_hud.py 2>&1 | tail -4`
Expected: 0 warning；黑盒 180/180 PASS。

---

## 收尾

- [ ] **Step 1: 文档同步**
  - `CHANGELOG.md` `[Unreleased]` 追加 1 条 bullet（批次 II：sessions 分页列表 + session <id> 详情（model/transcript_path 入库 migration + transcript 尾读明细）+ 工具成本归因排行（估算路径 ≈ 标注、未命中 `—`）+ 黑盒 180 例）。
  - `DEPLOY.md`：历史数据节后或「状态栏成本双轨」附近追加会话复盘说明（sessions/session 子命令 + 新列 + 排行口径）。
  - `COMPLETE.md`：✅ 段追加批次 II；路线图表追加一行；文末时间戳更新。

- [ ] **Step 2: 提交（AskUserQuestion 授权，不带 Co-Authored-By）**
  提交内容：`src/core/history.rs`、`src/core/pricing.rs`、`src/main.rs`、`locales/en.toml`、`locales/zh.toml`、`scripts/hudlib/cases.py`、`CHANGELOG.md`、`DEPLOY.md`、`COMPLETE.md`。
  不提交：`fixtures/`（tools.jsonl 未跟踪保持）、`reports/`、`docs/superpowers/`。

---

## 自审（writing-plans 要求）

- **Spec 覆盖**：⑤ 三验收点——有数据列表/分页（B2-01/02）、空库 `—`（B2-04）、zh 表头（B2-05）、history 回归（P4-01 等既有用例）；⑥ 三验收点——详情输出（B2-06 含模型/代理/token 明细）、不存在的 id 报错（B2-07）、空库（B2-08）、i18n（全量 tr 接线）；⑦ 两验收点——归因聚合单元测试（ranking_sorts_desc_and_estimates 等 4 个）、黑盒排行降序（B2-10 regex 顺序断言）+ 空数据（B2-11）+ 未命中 `—`（B2-12）。
- **spec 偏差（已拍板）**：spec 声称 "transcript_path 已入库" 与事实不符（schema 无该列且无 model 列）→ 本计划以 migration 补齐，属任务⑥ 方案点 2 的必要前置，不扩大范围；任务⑦ 的"dashboard 可选面板"按 YAGNI 跳过（验收未要求，session 详情段为唯一展示面）。
- **占位符**：无 TBD；所有代码块完整。`recent_sessions` 的 SELECT 变更与 `record_session` 的 INSERT 变更相互独立（互不影响既有 weekly_stats/daily_cost_trend——它们不 SELECT 这些列）。
- **类型一致性**：`tool_cost_ranking(&&TranscriptSummary, &PricingTable, &str) -> Option<Vec<(String, usize, f64)>>` 在 pricing.rs 定义与 main.rs 调用一致；`SessionRecord.model: String` / `transcript_path: Option<String>` 在 history.rs 查询映射与 main.rs 使用一致；i18n key 扁平点分风格一致（runtime.h_session_*、cli.session*）。
- **风险**：`?1 IS NULL OR started_at >= ?1` 的 NULL 语义在 rusqlite 绑定 None 时成立（SQLite 三值逻辑）；B2-03 日期过滤依赖真实 now（2026-08-04），`2099-01-01` 无条件空——稳定。
