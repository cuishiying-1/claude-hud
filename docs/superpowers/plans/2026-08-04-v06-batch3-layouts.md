# 批次 III 布局补全 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 `layout_from_mod` 只实现 minimal/activity 两个布局导致的 3 个出厂 Mod（obsidian-command / ember-night / noir-tabbed）切换即报 `layout not implemented` 的缺陷，补齐 agent-centric / kpi / contextual 三个布局。

**Architecture:** `src/compact.rs` 的 `layout_from_mod` 是纯函数布局解析（布局 ID → widget 集），新增三个常量 widget 集 + match 分支即可；contextual 需动态选择，给 `layout_from_mod`/`resolve_compact_layout` 增加 `active: bool` 参数（由 `render_with_data` 从 `data.subagent_status_line` 计算，与 agent_overview widget 同一数据源，黑盒可确定）。渲染管线（行数分层、chunking、fit_line、i18n）零改动。

**Tech Stack:** Rust · 现有 Widget 注册表（model_display/context_bar/agent_overview/cost_display/skills_mcp/token_rate/alerts/git_status/rate_limits 均已注册）· 黑盒 harness（scripts/test_hud.py）。

**约束（本仓库既有）**：绝不运行 `cargo fmt`；cargo 不在 PATH（每条命令前缀 `export PATH="$HOME/.cargo/bin:$PATH" &&`）；**不自动 git add/commit** — 本批次提交在批次结束统一 AskUserQuestion 授权（沿用 v0.5 模式，不带 Co-Authored-By）；不 stage 未跟踪的 `fixtures/`、`reports/`。

**涉及文件**：
- Modify: `src/compact.rs`（常量 + match 分支 + 签名 + 单测）
- Modify: `scripts/hudlib/cases.py`（P7 黑盒批次，152 → 156）
- Modify: `CHANGELOG.md`、`DEPLOY.md`、`COMPLETE.md`（docs 同步）

---

## 事实基线（已核实，实施时不必复查）

- `layout_from_mod`（compact.rs:28-49）：只匹配 `"minimal"`/`"activity"`，其余返回 Err → `[hud err]` 上屏（main.rs:172/187）。
- 现有常量（compact.rs:18-24）：`MINIMAL_WIDGETS = [model_display, context_bar, cost_display, git_status]`（4 个）、`ACTIVITY_WIDGETS = [model_display, context_bar, agent_overview, git_status, skills_mcp, cost_display, rate_limits]`（7 个）。
- 三个 Mod 声明（src/presets/*.toml）：obsidian-command `compact = "agent-centric"` + `compact_lines = 3`；ember-night `compact = "kpi"` + `compact_lines = 2`；noir-tabbed `compact = "contextual"` + `compact_lines = 1`。三者均无 `compact_widgets` 快照 → 走布局 ID 映射路径。
- 空数据渲染（黑盒行数断言依据）：token_rate 无数据渲染 `—`（非空）；skills_mcp 恒渲染 `◇ 0 ◆ 0`；agent_overview 无 agent 渲染空串（被 filter_map 丢弃）；alerts 无告警渲染空串（同样被丢弃，无碍行数）。
- contextual 活跃判据：`data.subagent_status_line.as_ref().map_or(false, |s| !s.agents.is_empty())`（与 agent_overview.rs:19 同源；黑盒用 `j(full_dict(...))` 构造，D1-22 已有先例）。
- `resolve_compact_layout`（compact.rs:57-68）唯一调用点是 `render_with_data`（compact.rs:272）；`layout_from_mod` 另被 compact.rs 两个单测调用。
- 黑盒 spec 键：`exit` / `stdout_contains` / `stdout_not_contains` / `stdout_regex` / `stderr_contains` 均可用；`render_case(config=...)` 写入 HUD_DIR/config.toml（runner 会恢复）；输出 `trim_end()` 无尾换行。

---

### Task 1: agent-centric 布局（⑧）

**Files:**
- Modify: `src/compact.rs`（常量 + match 分支 + 单测）

- [ ] **Step 1: 写失败单测** — 在 `src/compact.rs` 的 `mod tests` 中（`layout_from_mod_minimal_maps` 测试附近）追加：

```rust
#[test]
fn layout_from_mod_agent_centric_maps() {
    let got = layout_from_mod(None, "agent-centric", Language::En, false).unwrap();
    assert_eq!(
        got,
        vec!["agent_overview", "model_display", "context_bar",
             "cost_display", "skills_mcp", "token_rate"]
    );
}
```

- [ ] **Step 2: 运行确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test layout_from_mod 2>&1 | tail -15`
Expected: FAIL — `layout_from_mod` 对 "agent-centric" 返回 Err（当前签名只有 3 参，编译错误即失败信号）。

- [ ] **Step 3: 实现** — 在 `ACTIVITY_WIDGETS` 常量后追加：

```rust
/// 出厂 agent-centric 布局（obsidian-command：重度代理三行，代理信息前置）。
pub const AGENT_CENTRIC_WIDGETS: [&str; 6] = [
    "agent_overview", "model_display", "context_bar",
    "cost_display", "skills_mcp", "token_rate",
];
```

`layout_from_mod` match 分支（`"activity" => ...` 后）：

```rust
"agent-centric" => &AGENT_CENTRIC_WIDGETS,
```

注意：Task 3 会把签名改为 4 参（加 `active: bool`）。为避免改两遍，**本步直接把签名改成 4 参**，`layout_from_mod` 各分支暂忽略 active（contextual 分支 Task 3 再接）：

```rust
pub fn layout_from_mod(
    compact_widgets: Option<&Vec<String>>,
    layout_compact: &str,
    lang: Language,
    active: bool,
) -> Result<Vec<String>, String> {
```

同时更新两个既有单测（`layout_from_mod_widgets_win` / `layout_from_mod_minimal_maps`）的调用，末尾追加参数 `false`。`resolve_compact_layout` 调用点同步传 `false`（Task 3 接真值）。

- [ ] **Step 4: 运行确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test layout_from_mod 2>&1 | tail -15`
Expected: PASS（agent-centric 单测 + 既有 2 个单测；unknown 布局仍 Err）。

---

### Task 2: kpi 布局（⑨）

**Files:**
- Modify: `src/compact.rs`（常量 + match 分支 + 单测）

- [ ] **Step 1: 写失败单测** — 追加：

```rust
#[test]
fn layout_from_mod_kpi_maps() {
    let got = layout_from_mod(None, "kpi", Language::En, false).unwrap();
    assert_eq!(
        got,
        vec!["model_display", "context_bar", "cost_display",
             "token_rate", "agent_overview", "alerts"]
    );
}
```

- [ ] **Step 2: 运行确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test layout_from_mod 2>&1 | tail -15`
Expected: FAIL — "kpi" 返回 Err。

- [ ] **Step 3: 实现** — `AGENT_CENTRIC_WIDGETS` 后追加：

```rust
/// 出厂 kpi 布局（ember-night：深夜编码双行，成本/token 速率优先）。
pub const KPI_WIDGETS: [&str; 6] = [
    "model_display", "context_bar", "cost_display",
    "token_rate", "agent_overview", "alerts",
];
```

match 分支追加：

```rust
"kpi" => &KPI_WIDGETS,
```

- [ ] **Step 4: 运行确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test layout_from_mod 2>&1 | tail -15`
Expected: PASS。

---

### Task 3: contextual 动态布局（⑩）

**Files:**
- Modify: `src/compact.rs`（match 分支 + `resolve_compact_layout` 签名 + `render_with_data` 计算 active + 单测）

- [ ] **Step 1: 写失败单测** — 追加两个：

```rust
#[test]
fn layout_from_mod_contextual_idle_maps_minimal() {
    let got = layout_from_mod(None, "contextual", Language::En, false).unwrap();
    assert_eq!(
        got,
        vec!["model_display", "context_bar", "cost_display", "git_status"]
    );
}

#[test]
fn layout_from_mod_contextual_active_maps_activity() {
    let got = layout_from_mod(None, "contextual", Language::En, true).unwrap();
    assert_eq!(
        got,
        vec!["model_display", "context_bar", "agent_overview",
             "git_status", "skills_mcp", "cost_display", "rate_limits"]
    );
}
```

- [ ] **Step 2: 运行确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test layout_from_mod 2>&1 | tail -15`
Expected: FAIL — "contextual" 返回 Err。

- [ ] **Step 3: 实现**

`layout_from_mod` match 分支追加（注意：minimal/activity/agent-centric/kpi 分支均为 `&…` 数组引用，contextual 分支需 `if active` 选数组，用块表达式）：

```rust
"contextual" => {
    if active { &ACTIVITY_WIDGETS } else { &MINIMAL_WIDGETS }
}
```

`resolve_compact_layout` 改签名并透传（compact.rs:57-68）：

```rust
pub fn resolve_compact_layout(config: &AppConfig, active: bool) -> Result<Vec<String>, String> {
    if !config.active_mod.is_empty() {
        if let Ok(pkg) = AppConfig::load_mod(&config.active_mod) {
            return layout_from_mod(
                pkg.compact_widgets.as_ref(),
                pkg.layout.as_ref().map(|l| l.compact.as_str()).unwrap_or(""),
                config.language(),
                active,
            );
        }
    }
    Ok(config.compact_layout.clone())
}
```

`render_with_data` 中计算 active 并传参（compact.rs:272）：

```rust
let active = data
    .subagent_status_line
    .as_ref()
    .map_or(false, |s| !s.agents.is_empty());
let layout = resolve_compact_layout(config, active)?;
```

- [ ] **Step 4: 运行确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test layout_from_mod 2>&1 | tail -15`
Expected: PASS（4 个新单测 + 既有单测）。

- [ ] **Step 5: 全量单测确认无回归**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -5`
Expected: `test result: ok. 152 passed`（147 + 5 新增；0 failed，0 warnings）。

---

### Task 4: 黑盒 P7 用例（⑧⑨⑩ 端到端）

**Files:**
- Modify: `scripts/hudlib/cases.py`

- [ ] **Step 1: 加 helper** — 在 `fx()` 函数（cases.py:48）附近追加：

```python
def mod_config(name: str) -> str:
    """启用指定出厂 Mod 的最小配置（无 compact_layout，走 Mod 布局 ID）。"""
    return f'active_mod = "{name}"\nseparator = " │ "\n'
```

- [ ] **Step 2: 加 P7 批次** — 在 `P6 = [...]` 之后、`CASES = ...` 行之前追加（缩进与 P6 一致）：

```python
P7 = [
    render_case("P7-01", "⑧ obsidian-command：agent-centric 三行", "P7",
                {"exit": 0, "stdout_regex": r"^[^\n]+\n[^\n]+\n[^\n]+$",
                 "stdout_contains": ["ctx", "deepseek-v4-flash"]},
                stdin=j(full_dict()),
                config=mod_config("obsidian-command"),
                note="⑧ 布局补全：agent-centric 不再报 layout not implemented，输出 3 行"),
    render_case("P7-02", "⑨ ember-night：kpi 双行", "P7",
                {"exit": 0, "stdout_regex": r"^[^\n]+\n[^\n]+$",
                 "stdout_contains": ["ctx"]},
                stdin=j(full_dict()),
                config=mod_config("ember-night"),
                note="⑨ 布局补全：kpi 输出 2 行"),
    render_case("P7-03", "⑩ noir-tabbed：contextual 空闲 → minimal 集", "P7",
                {"exit": 0, "stdout_contains": ["ctx"],
                 "stdout_not_contains": ["agents"]},
                stdin=j(full_dict()),
                config=mod_config("noir-tabbed"),
                note="⑩ 无 subagent → minimal 布局（无 agents 段）"),
    render_case("P7-04", "⑩ noir-tabbed：contextual 活跃 → activity 集", "P7",
                {"exit": 0, "stdout_contains": ["agents"]},
                stdin=j(full_dict(**{"subagent_status_line": {
                    "agents": [{"name": "a1", "model": "deepseek-v4-flash",
                                "is_active": True}]}})),
                config=mod_config("noir-tabbed"),
                note="⑩ 有 subagent → activity 布局（含 agents 段）"),
]
```

- [ ] **Step 3: 更新计数断言** — `CASES = ...` 行追加 `+ P7`，计数改 156：

```python
CASES = D1 + D2 + D3 + D4 + D5 + D6 + D7 + D8 + P1 + P2 + P3 + P4 + P5 + P6 + P7
# 152 + 4（P7-01..04）= 156（批次 III 布局补全）
assert len(CASES) == 156, f"expected 156 cases, got {len(CASES)}"
```

- [ ] **Step 4: 跑黑盒套件**

Run: `python scripts/test_hud.py 2>&1 | tail -15`
Expected: `PASS 156/156`（或等价汇总）；任何 P7 失败按输出修断言（先看实际行数与文本，勿改实现）。

- [ ] **Step 5: 全量单测 + 黑盒双绿确认**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -3 && python scripts/test_hud.py 2>&1 | tail -5`
Expected: 单测 `152 passed` + 黑盒 `156/156`。

---

### Task 5: docs 同步 + 全量回归

**Files:**
- Modify: `CHANGELOG.md` · `DEPLOY.md` · `COMPLETE.md` · `docs/superpowers/specs/2026-08-04-v06-proposals-design.md`

- [ ] **Step 1: CHANGELOG** — `## [Unreleased]` 下追加 bullet：

```markdown
- 批次 III 布局补全：agent-centric / kpi / contextual 三个出厂布局真实实现（此前 3 个出厂 Mod 切换即报 layout not implemented）
```

- [ ] **Step 2: DEPLOY.md** — 6 个 Mod 表（`## 6 个出厂预设 Mod`）无需改动（表中布局名本就与实际一致，此前是"名义有实无"）；在「故障排除」前补一句说明：

```markdown
> 布局 ID（`[layout] compact`）全部真实实现：activity / minimal / agent-centric / kpi / contextual；未知 ID 报 `layout not implemented` 上屏（doctor 可查）。
```

- [ ] **Step 3: COMPLETE.md** — 按既有惯例更新：§ 相关段落追加 `· 批次 III 布局补全（agent-centric/kpi/contextual 真实实现 + contextual 动态两态 + 黑盒 156 例）`；roadmap v0.6 行标注布局补全完成。

- [ ] **Step 4: 规格文档勾选** — `docs/superpowers/specs/2026-08-04-v06-proposals-design.md` 任务 ⑧⑨⑩ 标题追加 `✅ 已完成`。

- [ ] **Step 5: 全量回归 + doctor**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -3 && python scripts/test_hud.py 2>&1 | tail -5 && echo '{"model":{"id":"deepseek-v4-flash","display_name":"x"},"context_window":{"used_percentage":1,"total_input_tokens":1,"context_window_size":200000},"cost":{"total_cost_usd":0.1,"total_duration_ms":60000}}' | claude-hud doctor 2>&1 | tail -5`
Expected: 单测 152 passed · 黑盒 156/156 · doctor `All checks passed`。

- [ ] **Step 6: 提交** — 批次结束统一 AskUserQuestion 授权后提交（建议单笔 `feat: 批次 III 布局补全（agent-centric/kpi/contextual）`；文件：src/compact.rs + scripts/hudlib/cases.py + CHANGELOG.md + DEPLOY.md + COMPLETE.md + 规格文档；**不 stage** fixtures/、reports/）。

---

## Self-Review

**Spec coverage**（对照 v06 规格文档 ⑧⑨⑩）：⑧=Task1 · ⑨=Task2 · ⑩=Task3（含动态两态）· 验收标准（无错误标记 + 行数 + 黑盒）→ Task4 · docs → Task5。无缺口。

**Placeholder scan**：无 TBD；所有代码/命令/预期输出已给出；既有单测更新点（`layout_from_mod_widgets_win`/`layout_from_mod_minimal_maps`）与 Task 1 Step 3 的签名改动一致。

**Type consistency**：`layout_from_mod` 4 参签名在 Task 1 引入、Task 3 使用（contextual 分支引用 `active`）；`resolve_compact_layout(config, active)` 与 `render_with_data` 调用一一对应；P7 用例 config/断言与事实基线（空数据渲染、trim_end 无尾换行、D1-22 subagent 先例）一致。

**风险**：P7-01/02 行数正则依赖"每行至少一个非空 widget" — token_rate（`—`）/skills_mcp（`◇ 0 ◆ 0`）恒非空，已核实；若实测不符（如 fit_line 意外截断），Step 4 允许按实际输出微调断言，不改实现。
