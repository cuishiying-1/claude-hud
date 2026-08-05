# v0.6 批次 IV（⑪⑫⑬⑭）— TUI 趋势面板与 Web 升级 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完成 v0.6 批次 IV 四项任务：⑪ TUI 历史趋势面板、⑫ Web SVG 成本趋势图、⑬ Web 会话列表与明细、⑭ 周环比，使 dashboard 与 Web 仪表盘具备历史成本视图。

**Architecture:** ⑪ 新增 dashboard-only widget `tui_trend`（复用 `HistoryStore::daily_cost_trend`，历史库不可用 → `—`），并在 dashboard 增加非 TTY 单帧模式使黑盒可断言；⑫⑬⑭ 全部在 serve.rs 服务端实现（SVG 服务端渲染零依赖、`/api/sessions` 分页端点复用 `sessions_page`、双周聚合复用 history.db），前端零构建链，JS 用 `textContent` 防 XSS。历史库造数由黑盒 harness 新增 `prepare_db_sql` 机制支持。

**Tech Stack:** Rust (ratatui 0.29 / tiny_http / rusqlite), Python harness (sqlite3), serde_json.

---

## 事实基线（执行者必读）

- **用户约束**：绝不自动 `git add`/`commit`/`push`（提交经 AskUserQuestion 批量授权，不带 Co-Authored-By）；绝不运行 `cargo fmt`；cargo 不在 PATH，所有 cargo 命令加前缀 `export PATH="$HOME/.cargo/bin:$PATH" &&`；绝不 stage 未跟踪的 `fixtures/`、`reports/`、`docs/superpowers/`。
- **黑盒计数现状**：`scripts/hudlib/cases.py` 末尾 `assert len(CASES) == 180`。D6 = serve 用例（D6-01..06），D7 = dashboard 用例（D7-01 仅一条：非 TTY 超时）。用例 id 全局唯一。
- **serve.rs 现状**（含用户未提交的 Web 面板健壮性重构）：`cached_history()` 返回 `(Value, Value)` 二元组（weekly, trend），`HISTORY_CACHE` 30s TTL；`build_dashboard_html` 用 `.replace("{web_*}", ...)` 替换链 + JS `T` 表（`T_PRICING_NOTE`/`T_NOT_FOUND` 替换）；趋势卡片当前由 JS 用 flex div 画柱（`#trend-bars`），⑫ 将删除该逻辑；路由 match `url.as_str()`（未拆 query）。
- **dashboard.rs 现状**：面板分配 = `config.compact_layout` 顺序映射（超出取 `context_bar` 兜底），tabbed 布局 tab 列表同源（dashboard.rs:301-315, 406-440）；`run()` 先 `enable_raw_mode()`；D7-01 显示非 TTY 下会一直运行到 10s 超时。
- **history.rs 现有查询**：`weekly_stats`/`daily_cost_trend`（近 7 天滚动窗口）/`weekly_report`/`sessions_page(limit, offset, date_from)`/`session_by_id(id)`；`HistoryStore { conn }` 字段模块内可访问（测试可直接 SQL）。
- **黑盒 harness**：`runner.py` 无数据库写入机制（只有 `remove_db`）；`run_serve` 已捕获响应 body 但无内容断言；`run_one` 中 `case.get("remove_db")` 在 pre_cmds 之前处理（test_hud.py:200-211）。
- **i18n**：en.toml 全量基准 / zh.toml 子集；扁平 dotted key；`tr(lang, key)` + `{placeholder}` replace；JS T 表走 `"T_XXX"` 字符串 replace。`[widget]` 段已有各 widget 显示名（`tr_dyn` 用于 tab 名）。
- **⑬ 详情复用**：`run_session`（main.rs:1023-1108）的详情逻辑 = `session_by_id` + transcript 尾读（`TranscriptReader::read_updates`）+ `tool_cost_ranking(s, merged_pricing(config), &r.model)` 前 5 行；Web 侧同逻辑但输出 JSON。
- **内置定价**：`pricing::merged_pricing(config)` 唯一入口；黑盒造数用 `model = 'claude-sonnet-4-6'`（在表内）。
- **用例辅助**：`serve_case(...)` 定义于 cases.py:527；`dash_case(...)` 定义于 cases.py:555；`fx()`/`j()`/`full_dict()`/`DEFAULT_CONFIG` 已有；黑盒 suite 入口 `python scripts/test_hud.py`（全量）/ `--case <id>`（单个）。

---

## Task 0: 黑盒 harness 基建 — `prepare_db_sql` 造库 + serve body 断言

**Files:**
- Modify: `scripts/test_hud.py`（`_prepare_db` 函数 + `run_one` 调用 + `run_serve` 断言）
- Modify: `scripts/hudlib/cases.py`（`serve_case`/`dash_case` 签名扩展）

- [ ] **Step 1: `test_hud.py` 加 `_prepare_db` 与 `run_one` 接线**

在 `prepare_case` 函数之前新增（`import sqlite3` 放函数内，避免顶层导入干扰）：

```python
def _prepare_db(exe_path, sqls):
    """⑪⑫⑬⑭ 预置 history.db：先跑一次 sessions 触发 init_schema 建表，
    再按序执行 SQL。SQLite 异常只告警不中断（避免污染全套件）。"""
    import sqlite3
    runner.run_exe(exe_path, ["sessions"], timeout_s=10)
    db_path = os.path.join(runner.HUD_DIR, "history.db")
    conn = sqlite3.connect(db_path)
    try:
        for sql in sqls:
            conn.execute(sql)
        conn.commit()
    except sqlite3.Error as e:
        print(f"  [WARN] prepare_db_sql failed: {e}")
    finally:
        conn.close()
```

在 `run_one` 中 `remove_db` 处理块（现 test_hud.py:200-211）之后、pre_cmds 循环之前插入：

```python
    # ⑪⑫⑬⑭：可选预置历史库数据（依赖 remove_db 已清空 + 建表在前）
    if case.get("prepare_db_sql"):
        _prepare_db(exe_path, case["prepare_db_sql"])
```

在 `run_serve` 中 `expect_json_fields` 检查之后（现 test_hud.py:140-142）追加：

```python
                for want in case.get("expect_body_contains", []):
                    if want not in body:
                        fails.append(f"body missing {want!r}")
                for want in case.get("expect_body_not_contains", []):
                    if want in body:
                        fails.append(f"body should not contain {want!r}")
```

- [ ] **Step 2: `cases.py` 扩展 `serve_case` / `dash_case` 签名**

将 `serve_case`（现 cases.py:527-536）整体替换为：

```python
def serve_case(cid, name, path, expect_status, expect_ct=None,
               expect_json=False, expect_json_fields=None, post_free=False,
               expect_body_contains=None, expect_body_not_contains=None,
               remove_db=False, prepare_db_sql=None, note=None):
    return {"id": cid, "name": name, "dim": "D6", "args": ["serve"],
            "run_kind": "serve", "path": path,
            "expect_status": expect_status, "expect_ct": expect_ct,
            "expect_json": expect_json,
            "expect_json_fields": expect_json_fields or [],
            "expect_body_contains": expect_body_contains or [],
            "expect_body_not_contains": expect_body_not_contains or [],
            "remove_db": remove_db,
            "prepare_db_sql": prepare_db_sql,
            "post_free": post_free,
            "spec": {"exit": None}, "note": note}
```

将 `dash_case`（现 cases.py:555-557）整体替换为：

```python
def dash_case(cid, name, spec, config=None, remove_db=False,
              prepare_db_sql=None, note=None):
    return {"id": cid, "name": name, "dim": "D7", "args": ["dashboard"],
            "run_kind": "dashboard", "spec": spec, "config": config,
            "remove_db": remove_db, "prepare_db_sql": prepare_db_sql,
            "note": note}
```

- [ ] **Step 3: 验证无回归（计数仍 180）**

Run: `python scripts/test_hud.py --case D6-01 && python scripts/test_hud.py --case D6-04 && python scripts/test_hud.py --case D7-01`
Expected: 三个用例 PASS（D7-01 仍为 timed_out，本任务不改行为）；`CASES == 180` 断言不受影响。

- [ ] **Step 4: 提交（经用户授权）**

```bash
git add scripts/test_hud.py scripts/hudlib/cases.py
git commit -m "test: harness 支持 prepare_db_sql 造历史库与 serve body 内容断言"
```

---

## Task 1: ⑪ TUI 历史趋势面板 + dashboard 非 TTY 单帧

**Files:**
- Create: `src/widgets/tui_trend.rs`
- Modify: `src/widgets/mod.rs`（`pub mod tui_trend;` + `register_all` 注册）
- Modify: `src/dashboard.rs`（`run()` 非 TTY 分支 + `render_single_frame`）
- Modify: `locales/en.toml`、`locales/zh.toml`（`[widget] tui_trend`）
- Modify: `scripts/hudlib/cases.py`（D7-01 更新 + D7-02/03/04 + `D7_TREND_CFG` + `trend_db`）

- [ ] **Step 1: 写失败测试（tui_trend.rs 测试模块）**

新建 `src/widgets/tui_trend.rs`，先写测试模块（文件其余部分留待 Step 3）：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trend_lines_empty_placeholder() {
        let lines = trend_lines(&[], 60);
        assert_eq!(lines, vec!["—".to_string()]);
    }

    #[test]
    fn trend_lines_bars_and_labels() {
        let days = vec![
            ("2026-08-01".to_string(), 1.0),
            ("2026-08-02".to_string(), 3.0),
            ("2026-08-03".to_string(), 2.0),
        ];
        let lines = trend_lines(&days, 30);
        assert_eq!(lines.len(), 9); // 8 柱行 + 1 标签行
        assert!(lines.iter().any(|l| l.contains('█')));
        assert!(lines[8].contains("08-01"));
        assert!(lines[8].contains("08-03"));
    }

    #[test]
    fn trend_lines_zero_cost_no_bars() {
        let days = vec![
            ("2026-08-01".to_string(), 0.0),
            ("2026-08-02".to_string(), 0.0),
        ];
        let lines = trend_lines(&days, 30);
        assert_eq!(lines.len(), 9);
        assert!(lines.iter().all(|l| !l.contains('█')));
    }

    #[test]
    fn trend_lines_single_day_full_bar() {
        let days = vec![("2026-08-01".to_string(), 5.0)];
        let lines = trend_lines(&days, 30);
        assert_eq!(lines.len(), 9);
        assert!(lines[0].contains('█'));
        assert!(lines[8].contains("08-01"));
    }
}
```

- [ ] **Step 2: 运行测试确认失败（trend_lines 未定义）**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test trend_lines 2>&1 | tail -15`
Expected: 编译错误 `cannot find function trend_lines`（RED）。

- [ ] **Step 3: 实现 `trend_lines` 纯函数 + Widget（tui_trend.rs 完整文件）**

```rust
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::core::ansi;
use crate::core::history::HistoryStore;
use crate::core::i18n::tr;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::widget::{Widget, WidgetConfig};

pub struct TuiTrend;

/// ⑪ 趋势面板文本行：近 7 天成本柱状（固定 8 行柱区 + 1 行日期标签，
/// 标签取首/中/尾三日去重）。空输入 → 占位「—」；全零成本 → 无柱（仅标签行）。
pub fn trend_lines(days: &[(String, f64)], width: u16) -> Vec<String> {
    if days.is_empty() {
        return vec!["—".to_string()];
    }
    let n = days.len();
    let max = days.iter().map(|(_, c)| *c).fold(0.0, f64::max).max(0.0001);
    let bar_rows = 8usize;
    let col_w = ((width as usize).saturating_sub(1) / n).max(1);
    let cols = col_w * n;
    let mut grid: Vec<Vec<char>> = vec![vec![' '; cols]; bar_rows];
    for (i, (_, cost)) in days.iter().enumerate() {
        let h = ((cost / max) * bar_rows as f64).round() as usize;
        for r in 0..h.min(bar_rows) {
            let row = bar_rows - 1 - r;
            for c in 0..col_w {
                grid[row][i * col_w + c] = '█';
            }
        }
    }
    let mut lines: Vec<String> = grid
        .into_iter()
        .map(|row| row.into_iter().collect())
        .collect();
    let mut label_row: Vec<char> = vec![' '; cols];
    let mut last: Option<usize> = None;
    for idx in [0usize, n / 2, n - 1] {
        if last == Some(idx) {
            continue;
        }
        last = Some(idx);
        let short = days[idx].0.get(5..).unwrap_or(&days[idx].0);
        let x = idx * col_w + col_w.saturating_sub(short.chars().count()) / 2;
        for (k, ch) in short.chars().enumerate() {
            if x + k < cols {
                label_row[x + k] = ch;
            }
        }
    }
    lines.push(label_row.into_iter().collect());
    lines
}

impl Widget for TuiTrend {
    fn id(&self) -> &str {
        "tui_trend"
    }

    fn display_name(&self) -> &str {
        "Trend"
    }

    /// dashboard-only：紧凑模式输出空串（用户若将其加入 compact_layout 不会报错）。
    fn render_compact(
        &self,
        _data: &SessionData,
        _theme: &Theme,
        _config: &WidgetConfig,
    ) -> String {
        String::new()
    }

    fn render_dashboard(
        &self,
        _data: &SessionData,
        area: Rect,
        frame: &mut Frame,
        theme: &Theme,
        config: &WidgetConfig,
    ) {
        let days = HistoryStore::open()
            .ok()
            .and_then(|h| h.daily_cost_trend().ok())
            .unwrap_or_default();
        let mut text = Text::default();
        text.push_line(Line::from(Span::styled(
            tr(config.lang, "web.cost_trend"),
            Style::default().fg(ansi::parse_ratatui_color(&theme.accent)),
        )));
        for line in trend_lines(&days, area.width.saturating_sub(2)) {
            text.push_line(Line::from(line));
        }
        frame.render_widget(Paragraph::new(text), area);
    }
}
```

（Step 1 的测试模块保留在文件末尾。）

- [ ] **Step 4: 运行测试确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test trend_lines 2>&1 | tail -15`
Expected: 4 个测试 PASS。

- [ ] **Step 5: 注册 widget + i18n**

`src/widgets/mod.rs`：`pub mod agent_detail;` 之后加 `pub mod tui_trend;`（保持字母序，插在 `token_rate` 前）；`register_all` 的 `// Phase 2` 块末尾（`token_rate` 注册行之后）加：

```rust
    registry.register(Box::new(tui_trend::TuiTrend));
```

`locales/en.toml` 的 `[widget]` 段末尾加：

```toml
tui_trend = "Trend"
```

`locales/zh.toml` 的 `[widget]` 段末尾加：

```toml
tui_trend = "趋势"
```

- [ ] **Step 6: 写 dashboard 非 TTY 单帧（行为变更：D7-01 从 10s 超时改为立即退出）**

`src/dashboard.rs`：`use crossterm::event::{...}` 之后加 `use crossterm::tty::IsTty;`；`run()` 函数在 `enable_raw_mode()` 之前插入分支：

```rust
    // ⑪ 非 TTY（黑盒/管道）：固定视口画一帧即退出，不进入 raw mode /
    // alt screen（原行为会一直运行到外部超时）。不 record session。
    if !io::stdout().is_terminal() {
        return render_single_frame(registry, config, theme);
    }
```

`persist_layout` 函数之前新增：

```rust
/// ⑪ 非 TTY 单帧：固定 100x30 视口（crossterm size() 在非 TTY 报错，
/// 用 Viewport::Fixed 兜底）画一次当前布局，供黑盒断言 dashboard 内容。
fn render_single_frame(
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> Result<(), String> {
    use ratatui::layout::Rect as RRect;
    use ratatui::TerminalOptions;
    use ratatui::Viewport;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(RRect::new(0, 0, 100, 30)),
        },
    )
    .map_err(|e| format!("single-frame terminal: {}", e))?;
    let data = state::read_current_data().unwrap_or_default();
    let layout_name = normalize_layout(&config.dashboard.default_layout);
    terminal
        .draw(|frame| {
            draw_dashboard(
                frame, registry, &data, theme, config, None, &layout_name, 0, false,
            );
        })
        .map_err(|e| format!("single-frame draw: {}", e))?;
    terminal.flush().map_err(|e| format!("single-frame flush: {}", e))
}
```

- [ ] **Step 7: 编译确认**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1 | tail -5`
Expected: 编译成功，0 warnings。

- [ ] **Step 8: 更新 D7 黑盒用例（D7-01 改断言 + 新增 3 条）**

`cases.py`：`D7 = [...]` 列表整体替换，并在 `D7` 定义之前加两个 helper：

```python
def trend_db():
    """⑫ 造 3 个不同日期各 1 条（-1/-2/-3 天，成本递增），供趋势面板/SVG。"""
    return [
        "INSERT INTO sessions (started_at, duration_secs, total_cost_usd, "
        "total_tokens, agent_count, model, transcript_path) "
        f"VALUES (datetime('now', '-{i} days'), 60, {1.0 + i * 0.5}, 500, 2, "
        "'claude-sonnet-4-6', '')" for i in (1, 2, 3)
    ]


D7_TREND_CFG = (
    "compact_layout = [\"tui_trend\"]\n"
    "[dashboard]\n"
    "refresh_interval_ms = 0\n"
    "default_layout = \"grid-2x2\"\n"
)

D7 = [
    dash_case("D7-01", "非 TTY 单帧退出",
              {"exit": 0, "stderr_empty": True},
              note="⑪：dashboard 非 TTY 检测 → 固定视口画一帧即退出（原 10s 超时行为废弃）"),
    dash_case("D7-02", "⑪ 趋势面板无历史库 —",
              {"exit": 0, "stdout_contains": ["—"]},
              config=D7_TREND_CFG, remove_db=True,
              note="⑪：空库 → 面板占位 —（单帧帧文本在 stdout）"),
    dash_case("D7-03", "⑪ 趋势面板有历史库柱形",
              {"exit": 0, "stdout_contains": ["█"]},
              config=D7_TREND_CFG, prepare_db_sql=trend_db(),
              note="⑪：3 天数据 → 柱形字符 █"),
    dash_case("D7-04", "⑪ sidebar 布局趋势面板",
              {"exit": 0, "stdout_contains": ["█"]},
              config=D7_TREND_CFG.replace("grid-2x2", "sidebar"),
              prepare_db_sql=trend_db(),
              note="⑪：sidebar 布局容纳趋势面板（三种非 tabbed 布局抽查）"),
]
```

- [ ] **Step 9: 全量单测 + 黑盒验证**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -8`
Expected: 全绿（既有 180 单测 + 新增 4）。
Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build && python scripts/test_hud.py --case D7-01 && python scripts/test_hud.py --case D7-02 && python scripts/test_hud.py --case D7-03 && python scripts/test_hud.py --case D7-04`
Expected: D7-01 exit 0 单帧（不再 timed_out）；D7-02 stdout 含 `—`；D7-03/04 stdout 含 `█`。

- [ ] **Step 10: 提交（经用户授权）**

```bash
git add src/widgets/tui_trend.rs src/widgets/mod.rs src/dashboard.rs locales/en.toml locales/zh.toml scripts/hudlib/cases.py
git commit -m "feat: ⑪ TUI 历史趋势面板 + dashboard 非 TTY 单帧模式"
```

---

## Task 2: ⑫ Web SVG 成本趋势图（服务端渲染）

**Files:**
- Modify: `src/serve.rs`（`trend_svg` + `trend_card_html` 纯函数 + 模板 `{web_trend_svg}` + 删 JS trend-bars + 替换链）
- Modify: `locales/en.toml`、`locales/zh.toml`（`web.trend_no_data`）
- Modify: `scripts/hudlib/cases.py`（D6-08 有数据 SVG / D6-13 空趋势占位）

- [ ] **Step 1: 写失败测试（serve.rs 测试模块追加）**

在 `serve.rs` tests 模块（`dashboard_html_respects_language` 之后）追加：

```rust
    #[test]
    fn trend_svg_two_points() {
        let days = vec![
            ("2026-08-01".to_string(), 1.0),
            ("2026-08-02".to_string(), 3.0),
        ];
        let svg = trend_svg(&days).expect("2+ points render");
        assert!(svg.contains("<svg"));
        assert!(svg.contains("<polyline points="));
        assert!(svg.contains("<circle"));
        assert!(svg.contains("08-01"));
        assert!(svg.contains("08-02"));
    }

    #[test]
    fn trend_svg_insufficient_points_none() {
        assert!(trend_svg(&[]).is_none());
        assert!(trend_svg(&[("2026-08-01".to_string(), 1.0)]).is_none());
    }

    #[test]
    fn trend_card_html_svg_or_placeholder() {
        use crate::core::i18n::Language;
        let trend = json!({"available": true, "days": [
            {"day": "2026-08-01", "cost": 1.0},
            {"day": "2026-08-02", "cost": 3.0},
        ]});
        let html = trend_card_html(&trend, Language::En);
        assert!(html.contains("<svg"));
        let empty = json!({"available": false, "days": []});
        let html2 = trend_card_html(&empty, Language::En);
        assert!(!html2.contains("<svg"));
        assert!(html2.contains("No trend data yet"));
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test trend_svg 2>&1 | tail -15`
Expected: 编译错误 `cannot find function trend_svg`（RED）。

- [ ] **Step 3: 实现 `trend_svg` / `trend_card_html`**

`serve.rs` 的 `build_dashboard_html` 函数之前新增：

```rust
/// ⑫ 服务端渲染 SVG 折线（零依赖）。数据点 <2 → None（调用方显示占位）。
/// 几何：viewBox 560x64，左右上边距 8/8/6，底部 16 留日期标签。
pub fn trend_svg(days: &[(String, f64)]) -> Option<String> {
    if days.len() < 2 {
        return None;
    }
    let (w, h) = (560.0_f64, 64.0_f64);
    let (ml, mt, mr, mb) = (8.0_f64, 6.0_f64, 8.0_f64, 16.0_f64);
    let max = days.iter().map(|(_, c)| *c).fold(0.0, f64::max).max(0.0001);
    let inner_w = w - ml - mr;
    let inner_h = h - mt - mb;
    let n = days.len();
    let xy = |i: usize| {
        let x = ml + inner_w * i as f64 / (n - 1) as f64;
        let y = mt + inner_h * (1.0 - days[i].1 / max);
        (x, y)
    };
    let mut points = String::new();
    let mut circles = String::new();
    for i in 0..n {
        let (x, y) = xy(i);
        points.push_str(&format!("{:.1},{:.1} ", x, y));
        circles.push_str(&format!(
            r#"<circle cx="{:.1}" cy="{:.1}" r="1.5" fill="#4c8dff"/>"#,
            x, y
        ));
    }
    let mut labels = String::new();
    let mut last: Option<usize> = None;
    for idx in [0usize, n / 2, n - 1] {
        if last == Some(idx) {
            continue;
        }
        last = Some(idx);
        let (x, _) = xy(idx);
        let short = days[idx].0.get(5..).unwrap_or(&days[idx].0);
        labels.push_str(&format!(
            r#"<text x="{:.1}" y="{:.1}" font-size="8" fill="#8b949e">{}</text>"#,
            x, h - 3.0, short
        ));
    }
    Some(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w:.0} {h:.0}" style="width:100%;height:64px;">
<polyline points="{points}" fill="none" stroke="#4c8dff" stroke-width="1.5"/>
{circles}
{labels}
</svg>"#,
        w = w,
        h = h,
        points = points.trim(),
        circles = circles,
        labels = labels,
    ))
}

/// ⑫ 趋势卡片内容：/api/data 的 trend JSON → 内嵌 SVG；数据 <2 点 → i18n 占位。
pub fn trend_card_html(trend: &Value, lang: crate::core::i18n::Language) -> String {
    let days: Vec<(String, f64)> = trend["days"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|d| {
                    Some((
                        d.get("day")?.as_str()?.to_string(),
                        d.get("cost")?.as_f64()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    match trend_svg(&days) {
        Some(svg) => svg,
        None => format!(r#"<div class="card-detail">{}</div>"#, tr(lang, "web.trend_no_data")),
    }
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test trend_svg 2>&1 | tail -15`
Expected: 3 个测试 PASS。

- [ ] **Step 5: 模板嵌入 + 删 JS trend-bars + 替换链**

`build_dashboard_html` 模板中 `#trend-card` 段（现含 `<div id="trend-bars" ...></div>`）整体替换为：

```html
  <div class="card" id="trend-card">
    <div class="card-title">{web_cost_trend}</div>
    {web_trend_svg}
  </div>
```

JS `refresh()` 中整个 trend 分支（从 `const trend = data.trend || {};` 到 `trendCard.style.display = 'none'; }` 的 `}` 为止）整体删除（trend-card 现在是服务端静态内容，JS 不再操作）。

替换链末尾（`.replace("T_NOT_FOUND", tr(lang, "web.not_found"))` 之后）追加：

```rust
        .replace("{web_trend_svg}", &trend_card_html(&cached_history().1, lang))
```

- [ ] **Step 6: i18n 占位文案**

`locales/en.toml` 的 `[web]` 段 `pricing_note` 行之后加：

```toml
trend_no_data = "No trend data yet"
```

`locales/zh.toml` 的 `[web]` 段 `pricing_note` 行之后加：

```toml
trend_no_data = "暂无趋势数据"
```

- [ ] **Step 7: 编译 + 全量单测**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -8`
Expected: 全绿（含 `dashboard_html_respects_language` 的 `!html.contains("{web_")` 断言——`{web_trend_svg}` 已在替换链中）。

- [ ] **Step 8: 黑盒用例（D6-08 有数据 / D6-13 空趋势）**

`cases.py` 的 `D6 = [...]` 列表末尾（D6-06 之后）追加：

```python
    serve_case("D6-08", "⑫ 趋势图 SVG 内嵌", "/", 200, "text/html; charset=utf-8",
               expect_body_contains=["<svg", "<polyline"],
               prepare_db_sql=trend_db(),
               note="⑫：3 个不同日期数据点 → 服务端渲染 SVG 折线内嵌 HTML"),
```

以及：

```python
    serve_case("D6-13", "⑫ 空趋势占位文本", "/", 200, "text/html; charset=utf-8",
               expect_body_contains=["No trend data yet"],
               expect_body_not_contains=["<svg"],
               remove_db=True,
               note="⑫：无历史库 → 占位（en 默认文案）"),
```

- [ ] **Step 9: 黑盒验证 + 计数更新**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build && python scripts/test_hud.py --case D6-08 && python scripts/test_hud.py --case D6-13`
Expected: 两个用例 PASS。
临时将 `assert len(CASES) == 180` 改为 182 再跑全套（验证无回归），确认后本任务保留 182（后续任务继续累加）。

- [ ] **Step 10: 提交（经用户授权）**

```bash
git add src/serve.rs locales/en.toml locales/zh.toml scripts/hudlib/cases.py
git commit -m "feat: ⑫ Web SVG 成本趋势图（服务端渲染零依赖）"
```

---

## Task 3: ⑬ Web 会话列表 + 成本明细表

**Files:**
- Modify: `src/serve.rs`（路由按 `?` 拆 query + `/api/sessions` 分页 + `/api/sessions/{id}` 详情 + `sessions_list_json`/`session_detail_json`/`query_param` + HTML 表格 + JS）
- Modify: `locales/en.toml`、`locales/zh.toml`（`[web]` 6 个表格 key + `load_more` + `week_compare` 提前加入 Task 4 用到——本任务只加表格 6 个 + load_more）
- Modify: `scripts/hudlib/cases.py`（D6-09..12 + `sessions_db`）

- [ ] **Step 1: 写失败测试（serve.rs 测试模块追加）**

```rust
    #[test]
    fn query_param_parses_and_defaults() {
        assert_eq!(query_param(Some("limit=1&offset=2"), "limit"), Some("1".to_string()));
        assert_eq!(query_param(Some("limit=1&offset=2"), "offset"), Some("2".to_string()));
        assert_eq!(query_param(Some("limit=1"), "date"), None);
        assert_eq!(query_param(None, "limit"), None);
        assert_eq!(query_param(Some(""), "limit"), None);
    }

    #[test]
    fn sessions_list_json_fields() {
        use crate::core::history::SessionRecord;
        let rows = vec![SessionRecord {
            id: 2,
            started_at: "2026-08-02 10:00:00".to_string(),
            duration_secs: 60,
            total_cost_usd: 1.25,
            total_tokens: 5000,
            agent_count: 1,
            model: "claude-sonnet-4-6".to_string(),
            transcript_path: None,
        }];
        let v = sessions_list_json(&rows);
        assert_eq!(v["sessions"][0]["id"], json!(2));
        assert_eq!(v["sessions"][0]["model"], json!("claude-sonnet-4-6"));
        assert_eq!(v["sessions"][0]["total_cost_usd"], json!(1.25));
        assert_eq!(v["sessions"][0]["total_tokens"], json!(5000));
    }

    #[test]
    fn session_detail_json_shapes() {
        use crate::core::history::SessionRecord;
        use crate::core::transcript::{AgentRecord, TokenTotal, TranscriptSummary};
        let record = SessionRecord {
            id: 7,
            started_at: "2026-08-01 10:00:00".to_string(),
            duration_secs: 60,
            total_cost_usd: 1.25,
            total_tokens: 5000,
            agent_count: 1,
            model: "claude-sonnet-4-6".to_string(),
            transcript_path: None,
        };
        let cfg: AppConfig = toml::from_str("").unwrap();
        // 无 transcript：transcript_detail available false + tools 空
        let v = session_detail_json(&record, None, &cfg);
        assert_eq!(v["id"], json!(7));
        assert_eq!(v["transcript_detail"]["available"], json!(false));
        assert_eq!(v["tools"].as_array().map(Vec::len), Some(0));
        // 有 transcript：tokens 分解 + agents + 排行（sonnet 在表内）
        let mut s = TranscriptSummary::default();
        s.total_tokens = TokenTotal {
            input: 1000,
            output: 2000,
            cache_created: 0,
            cache_read: 0,
        };
        s.tool_counts.insert("Bash".to_string(), 3);
        s.agents.push(AgentRecord {
            name: "alpha".to_string(),
            tool_calls: 3,
            ..Default::default()
        });
        let v2 = session_detail_json(&record, Some(&s), &cfg);
        assert_eq!(v2["transcript_detail"]["tokens_in"], json!(1000));
        assert_eq!(v2["transcript_detail"]["agents"][0]["name"], json!("alpha"));
        assert_eq!(v2["tools"][0]["tool"], json!("Bash"));
        assert_eq!(v2["tools"][0]["calls"], json!(3));
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test query_param 2>&1 | tail -15`
Expected: 编译错误 `cannot find function query_param`（RED）。

- [ ] **Step 3: 实现查询参数解析 + 列表/详情 JSON 纯函数**

`serve.rs` 的 `build_dashboard_html` 之前（`trend_card_html` 之后）新增：

```rust
/// ⑬ query 串参数（"limit=1&offset=0" → 值）；缺失 → None。
pub fn query_param(query: Option<&str>, key: &str) -> Option<String> {
    query?
        .split('&')
        .find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            (k == key).then(|| v.to_string())
        })
}

/// ⑬ 会话列表 JSON（分页行；字段与 sessions 表一一对应）。
pub fn sessions_list_json(rows: &[SessionRecord]) -> Value {
    json!({
        "available": true,
        "sessions": rows
            .iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "started_at": r.started_at,
                    "duration_secs": r.duration_secs,
                    "total_cost_usd": r.total_cost_usd,
                    "total_tokens": r.total_tokens,
                    "agent_count": r.agent_count,
                    "model": r.model,
                })
            })
            .collect::<Vec<_>>(),
    })
}

/// ⑬ 单会话详情 JSON：transcript 尾读（summary）+ 工具成本排行（前 5）。
/// 无 transcript → transcript_detail available false + tools 空（与 CLI ⑥ 一致）。
pub fn session_detail_json(
    record: &SessionRecord,
    summary: Option<&TranscriptSummary>,
    config: &AppConfig,
) -> Value {
    let tools: Vec<Value> = summary
        .as_ref()
        .and_then(|s| {
            crate::core::pricing::tool_cost_ranking(
                s,
                &crate::core::pricing::merged_pricing(config),
                &record.model,
            )
        })
        .unwrap_or_default()
        .iter()
        .take(5)
        .map(|(tool, calls, cost)| json!({"tool": tool, "calls": calls, "cost": cost}))
        .collect();
    json!({
        "id": record.id,
        "started_at": record.started_at,
        "model": record.model,
        "duration_secs": record.duration_secs,
        "total_cost_usd": record.total_cost_usd,
        "total_tokens": record.total_tokens,
        "agent_count": record.agent_count,
        "transcript_detail": match summary {
            Some(s) => json!({
                "available": true,
                "tokens_in": s.total_tokens.input,
                "tokens_out": s.total_tokens.output,
                "agents": s
                    .agents
                    .iter()
                    .map(|a| json!({"name": a.name, "tool_calls": a.tool_calls}))
                    .collect::<Vec<_>>(),
            }),
            None => json!({
                "available": false,
                "tokens_in": 0,
                "tokens_out": 0,
                "agents": [],
            }),
        },
        "tools": tools,
    })
}
```

`serve.rs` imports 增加：

```rust
use crate::core::history::{HistoryStore, SessionRecord};
use crate::core::transcript::TranscriptSummary;
```

- [ ] **Step 4: 运行测试确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test query_param 2>&1 | tail -15`
Expected: 3 个测试 PASS。

- [ ] **Step 5: 路由改造（拆 query + 两个新端点 + 404 详情）**

`run()` 的 match 前新增 query 拆分，并将 match 目标改为 `path`：

```rust
        // ⑬ 路由：按 '?' 拆分 query（serve 路径带参数）
        let (path, query) = match url.split_once('?') {
            Some((p, q)) => (p, Some(q)),
            None => (url.as_str(), None),
        };
        match path {
            "/" | "/index.html" => {
                // ... 原逻辑不变
            }
            "/api/data" => {
                // ... 原逻辑不变
            }
            "/api/sessions" => {
                let limit = query_param(query, "limit")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(10);
                let offset = query_param(query, "offset")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0);
                let body = match HistoryStore::open() {
                    Ok(h) => {
                        let rows = h.sessions_page(limit, offset, None).unwrap_or_default();
                        sessions_list_json(&rows).to_string()
                    }
                    Err(_) => json!({"available": false, "sessions": []}).to_string(),
                };
                let header = "Content-Type: application/json"
                    .parse::<tiny_http::Header>()
                    .unwrap();
                let _ = request.respond(Response::from_string(body).with_header(header));
            }
            _ if path.starts_with("/api/sessions/") => {
                let id_str = &path["/api/sessions/".len()..];
                let detail = id_str
                    .parse::<i64>()
                    .ok()
                    .and_then(|id| session_detail_body(id, config).ok());
                match detail {
                    Some(body) => {
                        let header = "Content-Type: application/json"
                            .parse::<tiny_http::Header>()
                            .unwrap();
                        let _ = request.respond(Response::from_string(body).with_header(header));
                    }
                    None => {
                        let body = Response::from_string(tr(lang, "web.not_found"))
                            .with_status_code(404);
                        let _ = request.respond(body);
                    }
                }
            }
            "/api/health" => {
                // ... 原逻辑不变
            }
            _ => {
                // ... 原逻辑不变（404）
            }
        }
```

`run()` 之外新增详情接线函数（`query_param` 之后）：

```rust
/// ⑬ 单会话详情接线：session_by_id → transcript 尾读 → 详情 JSON。
/// 未找到 / 库不可用 → Err（调用方 404）。
fn session_detail_body(id: i64, config: &AppConfig) -> Result<String, ()> {
    let store = HistoryStore::open().map_err(|_| ())?;
    let Some(record) = store.session_by_id(id).map_err(|_| ())? else {
        return Err(());
    };
    let summary = match record.transcript_path.as_deref() {
        Some(path) if std::path::Path::new(path).exists() => {
            Some(crate::core::transcript::TranscriptReader::new(path.into()).read_updates())
        }
        _ => None,
    };
    Ok(session_detail_json(&record, summary.as_ref(), config).to_string())
}
```

注意：原 match 分支体保持不变（仅 match 目标从 `url.as_str()` 变为 `path`，`/api/sessions` 与 `_ if` 分支为新增，`"/api/health"`、`_` 原样）。

- [ ] **Step 6: 编译确认**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1 | tail -5`
Expected: 编译成功。

- [ ] **Step 7: HTML 表格 + JS 会话列表/明细 + i18n**

`build_dashboard_html` 模板中 `#trend-card` 卡片之后（`<div id="widgets-area"` 之前）插入：

```html
  <div class="card" id="sessions-card">
    <div class="card-title">{web_sessions_title}</div>
    <table style="width:100%;border-collapse:collapse;font-size:11px;">
      <thead>
        <tr style="color:#8b949e;text-align:left;">
          <th>{web_col_time}</th><th>{web_col_cost}</th><th>{web_col_duration}</th>
          <th>{web_col_agents}</th><th>{web_col_tokens}</th>
        </tr>
      </thead>
      <tbody id="sessions-body"></tbody>
    </table>
    <button id="sessions-more" style="margin-top:8px;background:#21262d;border:1px solid #30363d;color:#c9d1d9;border-radius:4px;padding:4px 12px;font-size:11px;cursor:pointer;">{web_load_more}</button>
  </div>
```

`<style>` 段 `.realtime { ... }` 之前加：

```css
  .session-row { cursor:pointer; }
  .session-row:hover td { background:#161b22; }
  .session-detail td {
    background:#0d1117; font-size:10px; color:#8b949e;
    white-space:pre-wrap; border-top:1px dashed #21262d;
  }
```

JS `T` 表（`const T = {...};`）整体替换为：

```js
const T = {
  pricing_note: "T_PRICING_NOTE",
  not_found: "T_NOT_FOUND",
  load_more: "T_LOAD_MORE",
  h_model: "T_H_MODEL",
  h_tokens: "T_H_TOKENS",
  h_tokens_plain: "T_H_TOKENS_PLAIN",
  h_tools_title: "T_H_TOOLS_TITLE",
  h_tool_line: "T_H_TOOL_LINE",
};
```

`refresh()` 函数之后、`refresh(); setInterval(...)` 之前插入会话列表逻辑：

```js
let sessionOffset = 0;
function formatDur(secs) {
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return (m > 0 ? m + 'm ' : '') + s + 's';
}
function formatTok(tok) {
  return tok >= 1000 ? (tok / 1000).toFixed(1) + 'k' : String(tok);
}
async function loadSessions() {
  try {
    const resp = await fetch('/api/sessions?limit=10&offset=' + sessionOffset);
    const data = await resp.json();
    const tbody = document.getElementById('sessions-body');
    if (!data.available || !data.sessions || !data.sessions.length) {
      document.getElementById('sessions-more').style.display = 'none';
      if (sessionOffset === 0) {
        const tr = document.createElement('tr');
        const td = document.createElement('td');
        td.colSpan = 5;
        td.style.color = '#8b949e';
        td.textContent = '—';
        tr.appendChild(td);
        tbody.appendChild(tr);
      }
      return;
    }
    data.sessions.forEach(s => {
      const tr = document.createElement('tr');
      tr.className = 'session-row';
      [s.started_at,
       '$' + s.total_cost_usd.toFixed(2),
       formatDur(s.duration_secs),
       String(s.agent_count),
       formatTok(s.total_tokens)].forEach(text => {
        const td = document.createElement('td');
        td.textContent = text;
        tr.appendChild(td);
      });
      tr.addEventListener('click', () => toggleSessionDetail(tr, s.id));
      tbody.appendChild(tr);
    });
    sessionOffset += data.sessions.length;
  } catch(e) {
    console.error('sessions error:', e);
  }
}
async function toggleSessionDetail(tr, id) {
  const next = tr.nextElementSibling;
  if (next && next.className === 'session-detail') {
    next.remove();
    return;
  }
  const row = document.createElement('tr');
  row.className = 'session-detail';
  const td = document.createElement('td');
  td.colSpan = 5;
  td.textContent = '…';
  row.appendChild(td);
  tr.parentNode.insertBefore(row, tr.nextSibling);
  try {
    const resp = await fetch('/api/sessions/' + id);
    const d = await resp.json();
    const inout = (d.transcript_detail && d.transcript_detail.available)
      ? T.h_tokens
          .replace('{tok}', formatTok(d.total_tokens))
          .replace('{in}', d.transcript_detail.tokens_in)
          .replace('{out}', d.transcript_detail.tokens_out)
      : T.h_tokens_plain.replace('{tok}', formatTok(d.total_tokens));
    const tools = (d.tools || []).map(t => T.h_tool_line
      .replace('{tool}', t.tool)
      .replace('{n}', t.calls)
      .replace('{sym}', '$')
      .replace('{cost}', t.cost.toFixed(2)));
    td.textContent = [T.h_model.replace('{model}', d.model), inout].join(' · ')
      + (tools.length ? '\n' + T.h_tools_title + ': ' + tools.join('; ') : '');
  } catch(e) {
    td.textContent = T.not_found;
  }
}
loadSessions();
```

替换链末尾（`.replace("{web_trend_svg}", ...)` 之后）追加：

```rust
        .replace("{web_sessions_title}", tr(lang, "web.sessions_title"))
        .replace("{web_col_time}", tr(lang, "web.col_time"))
        .replace("{web_col_cost}", tr(lang, "web.col_cost"))
        .replace("{web_col_duration}", tr(lang, "web.col_duration"))
        .replace("{web_col_agents}", tr(lang, "web.col_agents"))
        .replace("{web_col_tokens}", tr(lang, "web.col_tokens"))
        .replace("{web_load_more}", tr(lang, "web.load_more"))
        .replace("T_LOAD_MORE", tr(lang, "web.load_more"))
        .replace("T_H_MODEL", tr(lang, "runtime.h_session_model"))
        .replace("T_H_TOKENS", tr(lang, "runtime.h_session_tokens"))
        .replace("T_H_TOKENS_PLAIN", tr(lang, "runtime.h_session_tokens_plain"))
        .replace("T_H_TOOLS_TITLE", tr(lang, "runtime.h_tools_title"))
        .replace("T_H_TOOL_LINE", tr(lang, "runtime.h_tool_line"))
```

- [ ] **Step 8: i18n 新增 keys**

`locales/en.toml` 的 `[web]` 段（`trend_no_data` 之后）加：

```toml
sessions_title = "Sessions"
col_time = "Time"
col_cost = "Cost"
col_duration = "Duration"
col_agents = "Agents"
col_tokens = "Tokens"
load_more = "Load more"
```

`locales/zh.toml` 的 `[web]` 段（`trend_no_data` 之后）加：

```toml
sessions_title = "会话列表"
col_time = "时间"
col_cost = "成本"
col_duration = "时长"
col_agents = "代理"
col_tokens = "Token"
load_more = "加载更多"
```

- [ ] **Step 9: 全量单测 + 黑盒用例**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -8`
Expected: 全绿（`dashboard_html_respects_language` 断言新占位符均已替换）。

`cases.py` 的 `D6 = [...]` 末尾（D6-13 之后）追加 `sessions_db` helper 与 4 条用例：

```python
def sessions_db(n: int):
    """⑬ 造 n 条会话（now / -1 / -2... 天，id 自增 1..n）。"""
    return [
        "INSERT INTO sessions (started_at, duration_secs, total_cost_usd, "
        "total_tokens, agent_count, model, transcript_path) "
        f"VALUES (datetime('now', '-{i} days'), {60 + i}, {0.5 + i * 0.25}, "
        f"{1000 + i * 100}, {1 + i % 3}, 'claude-sonnet-4-6', '')"
        for i in range(n)
    ]
```

（helper 放 `D7_TREND_CFG` 定义附近；D6 列表追加：）

```python
    serve_case("D6-09", "⑬ /api/sessions 列表", "/api/sessions", 200,
               expect_json=True, expect_json_fields=["sessions"],
               expect_body_contains=['"id":3', '"id":2', '"id":1'],
               prepare_db_sql=sessions_db(3),
               note="⑬：3 条会话 → id 降序 3/2/1"),
    serve_case("D6-10", "⑬ /api/sessions 分页 limit", "/api/sessions?limit=1", 200,
               expect_json=True, expect_json_fields=["sessions"],
               expect_body_contains=['"id":3'],
               expect_body_not_contains=['"id":2', '"id":1'],
               prepare_db_sql=sessions_db(3),
               note="⑬：limit=1 → 仅最新 #3"),
    serve_case("D6-11", "⑬ /api/sessions 详情", "/api/sessions/1", 200,
               expect_json=True,
               expect_json_fields=["model", "transcript_detail", "tools"],
               expect_body_contains=['"model":"claude-sonnet-4-6"'],
               prepare_db_sql=sessions_db(3),
               note="⑬：详情含 model/transcript_detail/tools（无 transcript → tools []）"),
    serve_case("D6-12", "⑬ /api/sessions 不存在 404", "/api/sessions/99", 404,
               prepare_db_sql=sessions_db(3),
               note="⑬：未找到 id → 404"),
```

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build && python scripts/test_hud.py --case D6-09 && python scripts/test_hud.py --case D6-10 && python scripts/test_hud.py --case D6-11 && python scripts/test_hud.py --case D6-12`
Expected: 4 个用例 PASS。计数临时改 186 跑全套确认无回归（本任务保留 186）。

- [ ] **Step 10: 提交（经用户授权）**

```bash
git add src/serve.rs locales/en.toml locales/zh.toml scripts/hudlib/cases.py
git commit -m "feat: ⑬ Web 会话列表与成本明细表（/api/sessions 分页 + 行展开详情）"
```

---

## Task 4: ⑭ 周环比（双周查询 + This Week 对比行）

**Files:**
- Modify: `src/core/history.rs`（`WeekAgg` + `weekly_compare` + 3 个单测）
- Modify: `src/serve.rs`（`pct_change`/`week_compare_json`/`week_compare_json_inner` + `cached_history` 三元组 + `/api/data` 字段 + JS 对比行 + 替换链）
- Modify: `locales/en.toml`、`locales/zh.toml`（`web.week_compare`）
- Modify: `scripts/hudlib/cases.py`（D6-07 有上周 / D6-14 无上周 + `week_db` + 最终计数 191）

- [ ] **Step 1: 写失败测试（history.rs 测试模块追加）**

```rust
    #[test]
    fn weekly_compare_both_weeks() {
        let store = mem_store();
        // 本周 2 条（started_at 默认 now）
        for _ in 0..2 {
            store.record_session(&session(1.0, 500, 500, 60), 2).unwrap();
        }
        // 上周 2 条（-8/-9 天，直接 SQL 指定 started_at）
        for i in (8..10).rev() {
            store
                .conn
                .execute(
                    "INSERT INTO sessions (started_at, duration_secs, total_cost_usd, \
                     total_tokens, agent_count, model, transcript_path) \
                     VALUES (datetime('now', '-? days'), 60, 2.0, 1000, 1, 'm', '')",
                    [],
                )
                .unwrap();
        }
        let (this, last) = store.weekly_compare().unwrap();
        let this = this.expect("this week present");
        let last = last.expect("last week present");
        assert_eq!(this.sessions, 2);
        assert_eq!(this.cost, 2.0);
        assert_eq!(this.tokens, 2000);
        assert_eq!(last.sessions, 2);
        assert_eq!(last.cost, 4.0);
        assert_eq!(last.tokens, 2000);
    }
```

（注：上述 SQL 的 `-? days` 写法不合法——`?` 不能做负数前缀；Step 3 实现时改为 `datetime('now', printf('-%d days', 8))` 或直接两段字面 SQL。执行者按 Step 3 的最终 SQL 修正本测试。）

- [ ] **Step 2: 运行测试确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test weekly_compare 2>&1 | tail -15`
Expected: 编译错误 `cannot find struct WeekAgg`（RED；若 Step 1 SQL 报错一并修正）。

- [ ] **Step 3: 实现 `WeekAgg` + `weekly_compare`**

`history.rs` 的 `WeeklyReport` struct 之后加：

```rust
/// ⑭ 单周聚合（周环比用）。
#[derive(Debug, Clone, PartialEq)]
pub struct WeekAgg {
    pub cost: f64,
    pub sessions: usize,
    pub tokens: u64,
}
```

`impl HistoryStore` 内 `weekly_report` 之后加：

```rust
    /// ⑭ 双周聚合：本周 vs 上周（SQLite %Y-%W 周键；上周 = now-7 天的周键，
    /// 跨年自动处理）。返回 (this_week, last_week)，无会话的周为 None。
    pub fn weekly_compare(&self) -> Result<(Option<WeekAgg>, Option<WeekAgg>), String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT strftime('%Y-%W', started_at) AS wk,
                        COUNT(*), COALESCE(SUM(total_cost_usd),0), COALESCE(SUM(total_tokens),0)
                 FROM sessions WHERE started_at >= datetime('now', '-14 days')
                 GROUP BY wk",
            )
            .map_err(|e| format!("prepare: {}", e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? as usize,
                    row.get::<_, f64>(2)?,
                    row.get::<_, i64>(3)? as u64,
                ))
            })
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect::<Vec<(String, usize, f64, u64)>>();
        let this_key: String = self
            .conn
            .query_row("SELECT strftime('%Y-%W', 'now')", [], |r| r.get(0))
            .map_err(|e| format!("this week key: {}", e))?;
        let last_key: String = self
            .conn
            .query_row("SELECT strftime('%Y-%W', 'now', '-7 days')", [], |r| r.get(0))
            .map_err(|e| format!("last week key: {}", e))?;
        let agg = |key: &str| {
            rows.iter()
                .find(|(wk, ..)| wk == key)
                .map(|(_, n, c, t)| WeekAgg {
                    cost: *c,
                    sessions: *n,
                    tokens: *t,
                })
        };
        Ok((agg(&this_key), agg(&last_key)))
    }
```

Step 1 的测试修正为（SQL 用字面量，不用 `?` 占位）：

```rust
    #[test]
    fn weekly_compare_both_weeks() {
        let store = mem_store();
        for _ in 0..2 {
            store.record_session(&session(1.0, 500, 500, 60), 2).unwrap();
        }
        for days in [8, 9] {
            let sql = format!(
                "INSERT INTO sessions (started_at, duration_secs, total_cost_usd, \
                 total_tokens, agent_count, model, transcript_path) \
                 VALUES (datetime('now', '-{} days'), 60, 2.0, 1000, 1, 'm', '')",
                days
            );
            store.conn.execute(&sql, []).unwrap();
        }
        let (this, last) = store.weekly_compare().unwrap();
        let this = this.expect("this week present");
        let last = last.expect("last week present");
        assert_eq!(this.sessions, 2);
        assert_eq!(this.cost, 2.0);
        assert_eq!(this.tokens, 2000);
        assert_eq!(last.sessions, 2);
        assert_eq!(last.cost, 4.0);
        assert_eq!(last.tokens, 2000);
    }

    #[test]
    fn weekly_compare_empty_db_none() {
        let store = mem_store();
        let (this, last) = store.weekly_compare().unwrap();
        assert_eq!(this, None);
        assert_eq!(last, None);
    }

    #[test]
    fn weekly_compare_no_last_week() {
        let store = mem_store();
        store.record_session(&session(1.0, 500, 500, 60), 2).unwrap();
        let (this, last) = store.weekly_compare().unwrap();
        assert!(this.is_some());
        assert_eq!(last, None);
    }
```

- [ ] **Step 4: 运行测试确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test weekly_compare 2>&1 | tail -15`
Expected: 3 个测试 PASS。

- [ ] **Step 5: 写 serve.rs 失败测试（pct_change / week_compare_json）**

serve.rs tests 模块追加：

```rust
    #[test]
    fn pct_change_up_down_flat_and_no_last() {
        assert_eq!(pct_change(2.0, 1.0), Some(100));
        assert_eq!(pct_change(1.0, 2.0), Some(-50));
        assert_eq!(pct_change(2.0, 2.0), Some(0));
        assert_eq!(pct_change(2.0, 0.0), None);
    }

    #[test]
    fn week_compare_json_with_and_without_last() {
        use crate::core::history::WeekAgg;
        let this = WeekAgg { cost: 2.0, sessions: 2, tokens: 2000 };
        let last = WeekAgg { cost: 4.0, sessions: 4, tokens: 1000 };
        let v = week_compare_json(Some(&this), Some(&last));
        assert_eq!(v["available"], json!(true));
        assert_eq!(v["cost_pct"], json!(-50));
        assert_eq!(v["session_pct"], json!(-50));
        assert_eq!(v["token_pct"], json!(100));
        assert_eq!(v["this_week"]["cost"], json!(2.0));
        let v2 = week_compare_json(Some(&this), None);
        assert_eq!(v2["last_week"], Value::Null);
        assert_eq!(v2["cost_pct"], Value::Null);
        let v3 = week_compare_json(None, None);
        assert_eq!(v3["this_week"], Value::Null);
    }
```

- [ ] **Step 6: 运行测试确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test pct_change 2>&1 | tail -15`
Expected: 编译错误 `cannot find function pct_change`（RED）。

- [ ] **Step 7: 实现 pct_change / week_compare_json / 接线**

serve.rs 的 `trend_json_inner` 之后加：

```rust
/// ⑭ 环比百分比：last ≤ 0（无上周）→ None；否则 (cur-last)/last*100 四舍五入。
pub fn pct_change(cur: f64, last: f64) -> Option<i64> {
    if last <= 0.0 {
        return None;
    }
    Some(((cur - last) / last * 100.0).round() as i64)
}

/// ⑭ 周环比 JSON：this/last 聚合 → 卡片数据。
/// this_week/last_week 为 null 表示该周无会话；cost_pct 等 null = 无上周可比
/// （前端显示 —）。
pub fn week_compare_json(
    this: Option<&WeekAgg>,
    last: Option<&WeekAgg>,
) -> Value {
    let week = |w: &WeekAgg| json!({"cost": w.cost, "sessions": w.sessions, "tokens": w.tokens});
    json!({
        "available": true,
        "this_week": this.map(week),
        "last_week": last.map(week),
        "cost_pct": this.zip(last).and_then(|(a, b)| pct_change(a.cost, b.cost)),
        "session_pct": this.zip(last).and_then(|(a, b)| pct_change(a.sessions as f64, b.sessions as f64)),
        "token_pct": this.zip(last).and_then(|(a, b)| pct_change(a.tokens as f64, b.tokens as f64)),
    })
}

/// ⑭ 周环比接线：库不可开/查询失败 → 全 None（available true 全 null）。
fn week_compare_json_inner() -> Value {
    let (this, last) = HistoryStore::open()
        .ok()
        .and_then(|h| h.weekly_compare().ok())
        .unwrap_or((None, None));
    week_compare_json(this.as_ref(), last.as_ref())
}
```

`cached_history` 改为三元组并引入 week_compare：

```rust
static HISTORY_CACHE: Mutex<Option<(Instant, Value, Value, Value)>> = Mutex::new(None);
// (fetched_at, weekly_json, trend_json, week_compare_json)

fn cached_history() -> (Value, Value, Value) {
    let mut guard = HISTORY_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    if let Some((at, weekly, trend, wc)) = guard.as_ref() {
        if ttl_fresh(*at, now, HISTORY_TTL) {
            return (weekly.clone(), trend.clone(), wc.clone());
        }
    }
    let weekly = weekly_json_inner();
    let trend = trend_json_inner();
    let wc = week_compare_json_inner();
    *guard = Some((now, weekly.clone(), trend.clone(), wc.clone()));
    (weekly, trend, wc)
}
```

`build_api_json` 中 `let (weekly, trend) = cached_history();` 改为 `let (weekly, trend, wc) = cached_history();`，json! 块 `"trend": trend,` 之后加 `"week_compare": wc,`。

`build_dashboard_html` 中 `.replace("{web_trend_svg}", &trend_card_html(&cached_history().1, lang))` 保持不变（`.1` 仍为 trend）。

- [ ] **Step 8: 运行测试确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test pct_change 2>&1 | tail -15`
Expected: 2 个测试 PASS。

- [ ] **Step 9: This Week 卡片对比行 + JS + i18n**

`build_dashboard_html` 模板 This Week 卡片（`{web_this_week}` 卡片）的 `metric-label` 行之后加：

```html
    <div class="card-detail" id="week-compare">--</div>
```

JS `refresh()` 中 This Week 处理块（`const wk = data.weekly || {};` ... `}`）之后加：

```js
    const wc = data.week_compare || {};
    const cmp = document.getElementById('week-compare');
    if (wc.available && wc.this_week && wc.cost_pct !== null && wc.cost_pct !== undefined) {
      const f = p => (p > 0 ? '+' : '') + p + '%';
      cmp.textContent = T.week_compare
        .replace('{cost}', f(wc.cost_pct))
        .replace('{sessions}', f(wc.session_pct))
        .replace('{tokens}', f(wc.token_pct));
    } else {
      cmp.textContent = '—';
    }
```

JS `T` 表（Step 3 已替换为多 key 版本）追加：

```js
  week_compare: "T_WEEK_COMPARE",
```

替换链末尾（`T_H_TOOL_LINE` 之后）追加：

```rust
        .replace("T_WEEK_COMPARE", tr(lang, "web.week_compare"))
```

`locales/en.toml` 的 `[web]` 段末尾（`load_more` 之后）加：

```toml
week_compare = "vs last week: cost {cost}% · sessions {sessions}% · tokens {tokens}%"
```

`locales/zh.toml` 的 `[web]` 段末尾（`load_more` 之后）加：

```toml
week_compare = "较上周：成本 {cost}% · 会话 {sessions}% · token {tokens}%"
```

- [ ] **Step 10: 全量单测 + 黑盒用例（D6-07 / D6-14）+ 最终计数 191**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -8`
Expected: 全绿。

`cases.py`：`sessions_db` helper 之后加 `week_db`：

```python
def week_db(has_last: bool):
    """⑭ 造双周数据：本周 2 条 $1.0/500tok；上周（可选）2 条 $2.0/500tok。"""
    sql = []
    for i in (0, 1):
        sql.append(
            "INSERT INTO sessions (started_at, duration_secs, total_cost_usd, "
            "total_tokens, agent_count, model, transcript_path) "
            f"VALUES (datetime('now', '-{i} days'), 60, 1.0, 500, 2, "
            "'claude-sonnet-4-6', '')"
        )
    if has_last:
        for i in (8, 9):
            sql.append(
                "INSERT INTO sessions (started_at, duration_secs, total_cost_usd, "
                "total_tokens, agent_count, model, transcript_path) "
                f"VALUES (datetime('now', '-{i} days'), 60, 2.0, 500, 1, "
                "'claude-sonnet-4-6', '')"
            )
    return sql
```

`D6 = [...]` 列表末尾追加（D6-12 之后）：

```python
    serve_case("D6-07", "⑭ 周环比有上周数据", "/api/data", 200,
               expect_json=True, expect_json_fields=["week_compare"],
               expect_body_contains=['"cost_pct":-50', '"last_week"'],
               prepare_db_sql=week_db(has_last=True),
               note="⑭：本周 2×$1.0 vs 上周 2×$2.0 → cost_pct -50（本周降 50%）"),
    serve_case("D6-14", "⑭ 周环比无上周数据", "/api/data", 200,
               expect_json=True, expect_json_fields=["week_compare"],
               expect_body_contains=['"last_week":null', '"cost_pct":null'],
               prepare_db_sql=week_db(has_last=False),
               note="⑭：只有本周 → last_week/cost_pct null（前端显示 —）"),
```

CASES 断言更新：

```python
# 180 + 8（D6-07..14 ⑫⑬⑭ Web 升级）+ 3（D7-02..04 ⑪ 趋势面板）= 191
assert len(CASES) == 191, f"expected 191 cases, got {len(CASES)}"
```

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build && python scripts/test_hud.py --case D6-07 && python scripts/test_hud.py --case D6-14`
Expected: 两个用例 PASS。
Run: `python scripts/test_hud.py`（全量 191）
Expected: 191 全 PASS（若 D6-06 端口 flakiness 出现，先 `taskkill //F //IM claude-hud.exe` 再重跑该用例）。

- [ ] **Step 11: 提交（经用户授权）**

```bash
git add src/core/history.rs src/serve.rs locales/en.toml locales/zh.toml scripts/hudlib/cases.py
git commit -m "feat: ⑭ 周环比（双周聚合 + This Week 对比行）"
```

---

## Task 5: 文档收尾（CHANGELOG / DEPLOY / COMPLETE）

**Files:**
- Modify: `CHANGELOG.md`（[Unreleased] 追加批次 IV 4 条）
- Modify: `DEPLOY.md`（新增"批次 IV"节）
- Modify: `COMPLETE.md`（§12/✅ 段/路线图/计数）

- [ ] **Step 1: CHANGELOG [Unreleased] 追加**

```markdown
- ⑪ TUI 历史趋势面板（dashboard 新 widget tui_trend，近 7 天成本柱状，历史库不可用显示 —；dashboard 非 TTY 单帧退出）
- ⑫ Web SVG 成本趋势图（服务端渲染零依赖，<2 点占位）
- ⑬ Web 会话列表与成本明细（/api/sessions 分页 + 行点击展开详情）
- ⑭ 周环比（本周 vs 上周成本/会话/token，This Week 卡片 +12%/−8%）
```

- [ ] **Step 2: DEPLOY.md 新增节**

参照既有"会话浏览（⑤⑥⑦，v0.6）"节格式，新增"历史趋势与 Web 升级（⑪⑫⑬⑭，v0.6）"节，记录：`tui_trend` widget 用法（compact_layout 加入后 dashboard 各布局显示）、serve 端点 `/api/sessions?limit=&offset=` 与 `/api/sessions/{id}`、双周口径（%Y-%W，上周 = now-7 天周键）、黑盒计数 191。

- [ ] **Step 3: COMPLETE.md 更新**

- §12 历史库能力表：`weekly_compare` 行
- ✅ 段追加批次 IV（4 项全 ✅）
- 路线图追加 2026-08-05 批次 IV 行
- CLI/widget 计数如有涉及同步更新（`tui_trend` widget 注册 → widget 计数 +1，若 COMPLETE 记录数量）
- 文末时间戳更新

- [ ] **Step 4: 提交（经用户授权）+ 记忆更新**

```bash
git add CHANGELOG.md DEPLOY.md COMPLETE.md
git commit -m "docs: v0.6 批次 IV（⑪⑫⑬⑭）文档收尾"
```

更新记忆文件 `C:\Users\admin\.claude\projects\D--workspace-claude-hud\memory\project_release.md`：批次进度 6 批次已完成 5/6（III I V II IV），剩余 VI（⑰⑱⑳）；黑盒计数 191；Cargo.toml 版本未对齐提示保留。

---

## 自审

**1. Spec 覆盖：**
- ⑪ spec 要求（dashboard 趋势面板、历史库不可用 → —、四种布局可容纳）→ Task 1（widget 化 → grid/sidebar/focus/tabbed 均通过 compact_layout 容纳；D7-02/03/04 覆盖 grid/sidebar；验收"黑盒两态"→ D7-02 无库 `—` + D7-03 有库柱形）
- ⑫ spec 要求（服务端渲染 SVG 零依赖、<2 点占位、curl 断言 HTML 含 `<svg` 与数据点、空趋势占位）→ Task 2（D6-08 `<svg`+`<polyline`、D6-13 占位 + 无 `<svg`）
- ⑬ spec 要求（/api/sessions 分页复用⑤、表格 5 列、行点击展开复用⑥、`{web_*}` 无残留）→ Task 3（D6-09/10 分页、D6-11/12 详情 404、`dashboard_html_respects_language` 断言无残留）
- ⑭ spec 要求（双周查询、+12%/−8%/— 对比行、有/无上周两态）→ Task 4（D6-07 有上周 cost_pct -50、D6-14 无上周 null）

**2. 占位符扫描：** 无 TBD/TODO；所有代码块完整；`?` 占位 SQL 问题已在 Task 4 Step 3 显式修正。

**3. 类型一致性：**
- `cached_history()` 从二元组 → 三元组（Task 4 Step 7 同步更新 `build_api_json` 解构；`build_dashboard_html` 的 `.1` 引用不变）
- `serve_case`/`dash_case` 新参数在 Task 0 定义，Task 1-4 全部用例使用相同键名（`prepare_db_sql`/`remove_db`/`expect_body_contains`/`expect_body_not_contains`/`config`）
- `session_detail_json` 签名 `(record, summary: Option<&TranscriptSummary>, config)` 在测试与接线函数中一致
- JS T 表 key（`week_compare`）与替换链 `T_WEEK_COMPARE` 对应
- D7-03/04 与 D6-08 共用 `trend_db()`；D6-07/14 用 `week_db()`；D6-09..12 用 `sessions_db(n)`

**4. 执行注意：** 每任务提交前经用户授权；`cargo` 命令一律带 `export PATH="$HOME/.cargo/bin:$PATH" &&` 前缀；不 stage `fixtures/`、`reports/`、`docs/superpowers/`；不跑 `cargo fmt`。
