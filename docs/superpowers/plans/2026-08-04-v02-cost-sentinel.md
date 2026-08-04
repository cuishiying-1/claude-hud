# v0.2 成本哨兵批次（⑲⑳㉑）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地 v0.2 成本哨兵三任务：⑲ 实时成本状态栏（`≈$0.42 · 12.3k/45.6k tok` 合并单组）、⑳ `[budget]` 预算告警（档位单调 + 跨进程冷却）、㉑ `history --weekly` 五指标周报 + serve 周趋势曲线。

**Architecture:** 纯函数 + 注入分离：`pricing::realtime_cost`（stdin 累计 token × in/out 单价，无 cache → ≈）供 render 实时路径，`effective_cost`（transcript 含 cache）保留给 dashboard；`check_budget` 复用现有 `AlertCooldown`（state.json 跨进程冷却），`state.budget_tier` 单调去重；`weekly_report` MAX 口径独立查询，`serve` 增 `trend` 字段 + 前端柱状曲线。

**Tech Stack:** Rust 2021 · serde · rusqlite · clap · 现有 harness（`scripts/test_hud.py`，130 用例 → 138）。

**环境约束（每条命令必须遵守）：**
- cargo 不在 PATH：每条 cargo 命令加前缀 `export PATH="$HOME/.cargo/bin:$PATH" &&`
- 禁止运行 `cargo fmt`
- 禁止自动 git add/commit/push：每个任务末尾的 commit 命令由**用户手动执行**
- 黑盒：`python scripts/test_hud.py`（env.py 已修复：自动用 target/debug 新构建；改动后先 `cargo build`）

**设计依据：** `docs/superpowers/specs/2026-08-04-v02-cost-sentinel-design.md`（已与用户确认）。

---

## 文件结构

| 文件 | 职责 |
|------|------|
| `src/core/pricing.rs` | ⑲ 新增 `realtime_cost` / `inject_cost_realtime`；两注入函数补 `pricing_configured` 键 |
| `src/compact.rs` | ⑲ 通知 + 注入切换实时路径（`:114` `:252`）；⑳ 预算档位接线 |
| `src/widgets/cost_display.rs` | ⑲ 合并单组格式 + `format_tokens` + `—` 降级；⑲ dashboard 未配置标注 |
| `src/core/config.rs` | ⑳ `BudgetConfig` + `AppConfig.budget` |
| `src/alert.rs` | ⑳ `AlertKind::Budget` + `fired_at`/`mark_fired` + `check_budget` |
| `src/core/state.rs` | ⑳ `budget_tier` 字段（向后兼容） |
| `src/notify.rs` | ⑳ `budget` 便捷函数（第 6 个） |
| `src/doctor.rs` | ⑳ `budget_check` 信息项（读 state.json 档位/冷却） |
| `src/core/history.rs` | ㉑ `WeeklyReport` + `weekly_report()` + `init_schema` 抽取（测试用） |
| `src/main.rs` | ㉑ `History { weekly: bool }` + `print_weekly_report` |
| `src/serve.rs` | ⑲ `pricing_configured`/`model_id` 字段 + 前端提示；㉑ `trend` 字段 + 前端曲线 |
| `scripts/hudlib/cases.py` | P2-05 断言更新 + P5-01..08（8 个新用例）+ 总数断言 138 |
| 文档 | DEPLOY.md / COMPLETE.md / CHANGELOG.md / README.md |

---

### Task 1: ⑲ realtime_cost 双轨计算 + compact 切换

**Files:**
- Modify: `src/core/pricing.rs`（新增函数 + 测试）
- Modify: `src/compact.rs:114`（通知成本）与 `src/compact.rs:252`（widget 注入）与 `render_with_data` 参数名
- Modify: `scripts/hudlib/cases.py`（P2-05 断言更新）

- [ ] **Step 1: 写失败测试**（pricing.rs 测试模块追加）

```rust
    fn session_with_tokens(model: &str, t_in: u64, t_out: u64, official: f64) -> SessionData {
        let json = format!(
            r#"{{"model":{{"id":"{model}","display_name":"{model}"}},
                "context_window":{{"used_percentage":1,"total_input_tokens":{t_in},
                "total_output_tokens":{t_out},"context_window_size":200000}},
                "cost":{{"total_cost_usd":{official},"total_duration_ms":1}}}}"#
        );
        SessionData::from_stdin_json(&json).unwrap()
    }

    #[test]
    fn realtime_hit_recomputes_and_marks_estimated() {
        let data = session_with_tokens("m1", 1_000_000, 500_000, 9.99);
        let mut pricing = PricingTable::new();
        pricing.insert(
            "m1".into(),
            PriceEntry { input: 1e-6, output: 2e-6, ..Default::default() },
        );
        let (cost, estimated) = realtime_cost(&data, &pricing);
        // 1.0 + 1.0（无 cache 项）
        assert!((cost - 2.0).abs() < 1e-9);
        assert!(estimated);
    }

    #[test]
    fn realtime_miss_passthroughs_official_cost() {
        let data = session_with_tokens("m2", 100, 100, 0.034);
        let (cost, estimated) = realtime_cost(&data, &PricingTable::new());
        assert_eq!(cost, 0.034);
        assert!(!estimated);
    }

    #[test]
    fn realtime_hit_without_tokens_passthroughs() {
        let data = session_with_tokens("m1", 0, 0, 0.034);
        let mut pricing = PricingTable::new();
        pricing.insert("m1".into(), PriceEntry { input: 1e-6, ..Default::default() });
        let (cost, estimated) = realtime_cost(&data, &pricing);
        assert_eq!(cost, 0.034);
        assert!(!estimated);
    }

    #[test]
    fn realtime_partial_prices_count_missing_as_zero() {
        let data = session_with_tokens("m1", 1000, 0, 9.99);
        let mut pricing = PricingTable::new();
        pricing.insert("m1".into(), PriceEntry { input: 1e-3, ..Default::default() });
        let (cost, estimated) = realtime_cost(&data, &pricing);
        assert!((cost - 1.0).abs() < 1e-12);
        assert!(estimated);
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test realtime_ 2>&1 | tail -5`
Expected: FAIL（`error[E0425]: cannot find function realtime_cost` 等）

- [ ] **Step 3: 实现 realtime_cost + inject_cost_realtime**（pricing.rs，`effective_cost` 之后插入）

```rust
/// ⑲ 实时路径成本：stdin 会话累计 token（input/output）× 单价。
/// 实时路径无 cache 数据 → 必然低估 → 命中返回 (估算值, true)；
/// 未命中 [pricing] → 透传官方 total_cost_usd（含 cache，准）；
/// 命中但 token 全 0 → 无数据可算 → 透传。
pub fn realtime_cost(data: &SessionData, pricing: &PricingTable) -> (f64, bool) {
    if let Some(price) = pricing.get(&data.model.id) {
        let t_in = data.context_window.total_input_tokens as f64;
        let t_out = data.context_window.total_output_tokens as f64;
        if t_in > 0.0 || t_out > 0.0 {
            return (price.input * t_in + price.output * t_out, true);
        }
    }
    (data.cost.total_cost_usd, false)
}

/// ⑲ 实时注入（compact/render 路径）：与 inject_cost 同组键，widget 签名零改动。
pub fn inject_cost_realtime(
    data: &SessionData,
    config: &AppConfig,
    widget_config: &mut WidgetConfig,
) {
    let (cost, estimated) = realtime_cost(data, &config.pricing);
    widget_config
        .values
        .insert("effective_cost".into(), cost.to_string());
    widget_config
        .values
        .insert("cost_estimated".into(), estimated.to_string());
    widget_config
        .values
        .insert("currency_symbol".into(), config.currency_symbol.clone());
}
```

- [ ] **Step 4: 运行确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test realtime_ 2>&1 | tail -5`
Expected: PASS（4 个用例，`test result: ok`）

- [ ] **Step 5: compact.rs 切换实时路径（两处）**

`src/compact.rs:114`：
```rust
    let (effective_cost, _) = pricing::realtime_cost(data, &config.pricing);
```
`src/compact.rs:252`：
```rust
                pricing::inject_cost_realtime(data, config, &mut widget_config);
```
`render_with_data` 签名参数 `summary: Option<&TranscriptSummary>` → `_summary: Option<&TranscriptSummary>`（切换后该参数无使用点，防 unused 警告；doc 注释保留）。

- [ ] **Step 6: 更新 P2-05 断言**（cases.py，`≈$0.56` → `≈$16.80`，移除 transcript_copy，更新 note）

```python
    render_case("P2-05", "[pricing] 命中重算 ≈$（实时路径）", "P2",
                {"exit": 0, "stdout_contains": ["≈$16.80"], "stderr_empty": True},
                stdin=j(full_dict()), config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"cost_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"
                    "[pricing]\n"
                    "\"deepseek-v4-flash\" = { input = 0.001, output = 0.002 }\n"),
                note="任务⑲：双轨切换后 cost_display 走实时路径 — stdin 累计 6800/5000 × 单价 = 16.8，≈ 标注（transcript 不再参与状态栏成本）"),
```

- [ ] **Step 7: 构建 + 回归**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1 | tail -3 && python scripts/test_hud.py --case P2-05 --case P2-06 --case P2-07 --case P2-08 --case P3-15 2>&1 | tail -8`
Expected: 构建无 warning（unused `summary` 已消）、5 个用例全 PASS

- [ ] **Step 8: Commit（用户手动执行）**

```bash
git add src/core/pricing.rs src/compact.rs scripts/hudlib/cases.py && git commit -m "feat: ⑲ 实时成本双轨 — realtime_cost + inject_cost_realtime + compact 切换"
```

---

### Task 2: ⑲ cost_display 合并单组 + k 缩写 + — 降级

**Files:**
- Modify: `src/widgets/cost_display.rs`

- [ ] **Step 1: 写失败测试**（cost_display.rs 底部追加测试模块；`format_tokens` 为纯函数无需 Theme）

```rust
#[cfg(test)]
mod tests {
    use super::format_tokens;

    #[test]
    fn tokens_k_abbreviation() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1000), "1.0k");
        assert_eq!(format_tokens(6800), "6.8k");
        assert_eq!(format_tokens(5000), "5.0k");
        assert_eq!(format_tokens(12345), "12.3k");
        assert_eq!(format_tokens(100_000), "100k");
        assert_eq!(format_tokens(450_000), "450k");
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test tokens_k_abbreviation 2>&1 | tail -5`
Expected: FAIL（`cannot find function format_tokens`）

- [ ] **Step 3: 实现 format_tokens + 重写 render_compact**

```rust
/// ⑲ k 缩写（spec 样例口径）：≥100k 去小数防溢出；≥1k 一位小数；否则原数。
pub fn format_tokens(n: u64) -> String {
    if n >= 100_000 {
        format!("{}k", n / 1000)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}
```

render_compact 整体替换为：

```rust
    fn render_compact(&self, data: &SessionData, theme: &Theme, config: &WidgetConfig) -> String {
        let symbol = config.get_str("currency_symbol", "$");
        let cost = config.get_f64("effective_cost", data.cost.total_cost_usd);
        let estimated = config.get_bool("cost_estimated", false);
        let t_in = data.context_window.total_input_tokens;
        let t_out = data.context_window.total_output_tokens;
        // ⑲ 诚实降级：无任何成本/用量数据 → —（网关无 usage/cost，不显示 $0.00 假精确）
        if cost == 0.0 && t_in == 0 && t_out == 0 && !estimated {
            return "—".to_string();
        }
        let warn = config.get_f64("warn_threshold_usd", 10.0);
        let color = if cost >= warn { &theme.warning } else { &theme.success };
        let prefix = if estimated { "≈" } else { "" };
        let group = format!(
            "{}{}{:.2} · {}/{} tok",
            prefix,
            symbol,
            cost,
            format_tokens(t_in),
            format_tokens(t_out)
        );
        ansi::ansi_fg(&group, color)
    }
```

- [ ] **Step 4: 运行确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test tokens_k_abbreviation 2>&1 | tail -5`
Expected: PASS

- [ ] **Step 5: 构建 + 回归**（P3-15 整段上色正则在新格式下必须仍匹配）

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1 | tail -3 && python scripts/test_hud.py --case P3-15 --case P2-05 --case P2-06 2>&1 | tail -6`
Expected: 3 个用例全 PASS（P3-15 正则 `\$[0-9.]+[^\x1b]*` 匹配 `$0.03 · 6.8k/5.0k tok`）

- [ ] **Step 6: Commit（用户手动执行）**

```bash
git add src/widgets/cost_display.rs && git commit -m "feat: ⑲ cost_display 合并单组 ≈\$X · Xk/Xk tok + k 缩写 + 零数据 — 降级"
```

---

### Task 3: ⑲ 未配置单价标注（serve/dashboard）+ pricing_configured 键

**Files:**
- Modify: `src/core/pricing.rs`（两个注入函数补 `pricing_configured`）
- Modify: `src/serve.rs`（api 字段 + HTML 提示行 + JS）
- Modify: `src/widgets/cost_display.rs`（render_dashboard 行尾标注）
- Modify: `scripts/hudlib/cases.py`（D6-02 expect_json_fields 扩展）

- [ ] **Step 1: 写失败测试**（pricing.rs 测试模块追加）

```rust
    #[test]
    fn inject_cost_adds_pricing_configured_flag() {
        let data = session("m1", 0.5);
        let mut pricing = PricingTable::new();
        pricing.insert("m1".into(), PriceEntry::default());
        let mut config = AppConfig::default();
        config.pricing = pricing;
        let mut wc = WidgetConfig::default();
        inject_cost(&data, None, &config, &mut wc);
        assert!(wc.get_bool("pricing_configured", false));
        let mut wc2 = WidgetConfig::default();
        inject_cost(&data, None, &AppConfig::default(), &mut wc2);
        assert!(!wc2.get_bool("pricing_configured", true));
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test inject_cost_adds_pricing_configured 2>&1 | tail -5`
Expected: FAIL

- [ ] **Step 3: 实现**（pricing.rs，两个注入函数 body 末尾各加一行；注入键与 dashboard 共用）

```rust
    widget_config
        .values
        .insert(
            "pricing_configured".into(),
            config.pricing.contains_key(&data.model.id).to_string(),
        );
```

- [ ] **Step 4: 运行确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test inject_cost_adds_pricing_configured 2>&1 | tail -5`
Expected: PASS

- [ ] **Step 5: serve.rs 加字段 + 前端提示**

`build_api_json`（src/serve.rs:55）：
```rust
    let pricing_configured = config.pricing.contains_key(&data.model.id);
    format!(
        r#"{{"model":"{}","model_id":"{}","pricing_configured":{},"context_pct":{},"cost_usd":{},"duration_ms":{},"weekly":{},"widgets":[{}]}}"#,
        data.model.display_name,
        data.model.id,
        pricing_configured,
        data.context_window.used_percentage,
        data.cost.total_cost_usd,
        data.cost.total_duration_ms,
        weekly_json(),
        widgets_json.join(","),
    )
```

HTML：`<div class="grid" id="dashboard-grid">` 之后（src/serve.rs:182）插入提示行：
```html
<div id="pricing-note" style="display:none;color:#d29922;font-size:11px;margin-bottom:12px;"></div>
```

JS（`refresh()` 内 `update-time` 赋值行之前，src/serve.rs:234）：
```js
    const note = document.getElementById('pricing-note');
    if (data.pricing_configured) {
      note.style.display = 'none';
    } else {
      note.textContent = '当前模型未配置单价 (model.id: ' + data.model_id + ') — 状态栏成本为官方透传值';
      note.style.display = 'block';
    }
```

- [ ] **Step 6: cost_display render_dashboard 行尾标注**

```rust
    fn render_dashboard(&self, data: &SessionData, area: Rect, frame: &mut Frame, _theme: &Theme, config: &WidgetConfig) {
        let dur = data.cost.total_duration_ms / 1000;
        let mut text = format!("Cost: ${:.4} | {}m {}s | +{}/-{} lines",
            data.cost.total_cost_usd, dur / 60, dur % 60,
            data.cost.total_lines_added, data.cost.total_lines_removed);
        // ⑲ 未命中 [pricing] → 完整数据视图标注（命中时省略）
        if !config.get_bool("pricing_configured", false) {
            text.push_str(&format!(" | 未配置单价 (model.id: {})", data.model.id));
        }
        frame.render_widget(Text::from(text), area);
    }
```

- [ ] **Step 7: D6-02 扩展**（cases.py:531）

```python
    serve_case("D6-02", "GET /api/data", "/api/data", 200, "application/json",
               expect_json=True,
               expect_json_fields=["weekly", "pricing_configured"],
               note="serve.rs 将 compact render（含 ANSI 码）嵌入 JSON 字段；harness 自动剥离 ANSI 后再 parse JSON；weekly 字段来自历史库（空库可用性标记）"),
```

- [ ] **Step 8: 构建 + 回归**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1 | tail -3 && python scripts/test_hud.py --case D6-02 2>&1 | tail -4`
Expected: D6-02 PASS（weekly + pricing_configured 均在）

- [ ] **Step 9: Commit（用户手动执行）**

```bash
git add src/core/pricing.rs src/serve.rs src/widgets/cost_display.rs scripts/hudlib/cases.py && git commit -m "feat: ⑲ 未配置单价标注 — pricing_configured 注入 + serve 字段/提示行 + dashboard 行尾"
```

---

### Task 4: ⑳ BudgetConfig + AlertKind::Budget + state.budget_tier + check_budget + notify

**Files:**
- Modify: `src/core/config.rs`
- Modify: `src/alert.rs`
- Modify: `src/core/state.rs`
- Modify: `src/notify.rs`

- [ ] **Step 1: 写失败测试**（三个文件分三组；先 alert.rs）

alert.rs 测试模块追加（`use crate::core::config::BudgetConfig;`）：
```rust
    fn budget_cfg() -> BudgetConfig {
        BudgetConfig { cap_usd: 5.0, warn_pcts: vec![50.0, 80.0, 100.0] }
    }

    #[test]
    fn budget_tier_progression_fires_each_tier_once() {
        let mut cd = AlertCooldown::default();
        // 40% → 无档位
        assert!(check_budget(2.0, &budget_cfg(), 10, 0, &mut cd, 1000).is_none());
        // 60% ≥ 50% → tier 1
        assert_eq!(check_budget(3.0, &budget_cfg(), 10, 0, &mut cd, 1001), Some(1));
        // 同 tier（回落再升）→ 单调不发
        assert!(check_budget(3.0, &budget_cfg(), 10, 1, &mut cd, 1002).is_none());
        // 90% ≥ 80% → tier 2
        assert_eq!(check_budget(4.5, &budget_cfg(), 10, 1, &mut cd, 1003), Some(2));
        // 120% ≥ 100% → tier 3
        assert_eq!(check_budget(6.0, &budget_cfg(), 10, 2, &mut cd, 1004), Some(3));
    }

    #[test]
    fn budget_cooldown_window_blocks_refire() {
        let mut cd = AlertCooldown::default();
        assert_eq!(check_budget(6.0, &budget_cfg(), 10, 0, &mut cd, 1000), Some(3));
        // 窗口 600s 内：tier 未更高 → 不发（单调已挡）；改 last_tier 更低模拟竞态
        let mut cd2 = AlertCooldown::default();
        assert_eq!(check_budget(6.0, &budget_cfg(), 10, 0, &mut cd2, 1000), Some(3));
        assert!(check_budget(6.0, &budget_cfg(), 10, 0, &mut cd2, 1500).is_none());
        // 冷却过期（now - last >= 600）且档位更高 → 重发
        assert_eq!(check_budget(7.0, &budget_cfg(), 10, 0, &mut cd2, 2000), Some(3));
    }

    #[test]
    fn budget_disabled_when_cap_zero_or_cost_zero() {
        let mut cd = AlertCooldown::default();
        let off = BudgetConfig { cap_usd: 0.0, warn_pcts: vec![50.0] };
        assert!(check_budget(100.0, &off, 10, 0, &mut cd, 1).is_none());
        assert!(check_budget(0.0, &budget_cfg(), 10, 0, &mut cd, 1).is_none());
    }

    #[test]
    fn budget_warn_pcts_out_of_order_converges_to_highest() {
        let mut cd = AlertCooldown::default();
        let messy = BudgetConfig { cap_usd: 10.0, warn_pcts: vec![100.0, 50.0, 80.0] };
        // 60% ≥ 50%（index 1）→ tier 2
        assert_eq!(check_budget(6.0, &messy, 10, 0, &mut cd, 1), Some(2));
        // 100% ≥ 100%（index 0）→ tier 1（低于已触发 2，不发）
        assert!(check_budget(11.0, &messy, 10, 2, &mut cd, 2).is_none());
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test budget_ 2>&1 | tail -5`
Expected: FAIL（`cannot find function check_budget` / `cannot find type BudgetConfig`）

- [ ] **Step 3: 实现 config.rs**（`AlertsConfig` 之后追加）

```rust
/// [budget] 预算告警：cap_usd（0=关闭）+ warn_pcts 渐进档位（每档一次）。
/// 冷却复用 [alerts].cooldown_minutes；预算基于 ≈ 实时估算成本触发。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BudgetConfig {
    #[serde(default = "default_budget_cap")]
    pub cap_usd: f64,
    #[serde(default = "default_budget_warn_pcts")]
    pub warn_pcts: Vec<f64>,
}

fn default_budget_cap() -> f64 { 0.0 }
fn default_budget_warn_pcts() -> Vec<f64> { vec![50.0, 80.0, 100.0] }

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            cap_usd: 0.0,
            warn_pcts: vec![50.0, 80.0, 100.0],
        }
    }
}
```

AppConfig 字段（`alerts` 之后）：
```rust
    #[serde(default)]
    pub budget: BudgetConfig,
```

- [ ] **Step 4: 实现 alert.rs**（枚举 + 访问器 + check_budget；顶部 `use crate::core::config::BudgetConfig;`）

```rust
pub enum AlertKind {
    ContextCritical,
    CostThreshold,
    RateLimit,
    Budget,
}
```
AlertCooldown impl 内追加：
```rust
    /// Read the last-fired timestamp for a kind (0 = never fired).
    pub fn fired_at(&self, kind: AlertKind) -> u64 {
        self.last_fired.get(&kind).copied().unwrap_or(0)
    }

    /// Record a fire timestamp (跨进程冷却写入，随 state.alerts 持久化)。
    pub fn mark_fired(&mut self, kind: AlertKind, now: u64) {
        self.last_fired.insert(kind, now);
    }
```
文件末尾（send_notifications 之后）追加：
```rust
/// ⑳ 预算档位检查（纯函数，无 OS 副作用）：
/// cost ≥ cap×pct/100 的最高档位 > 已触发档位 → 触发；冷却窗口内不重复发
/// （档位单调 + 冷却双保险：单调防回落重发，冷却防跨进程竞态）。
/// 与 check_alerts 同用 AlertCooldown（Budget 键），触发时内部 mark_fired。
pub fn check_budget(
    cost: f64,
    cfg: &BudgetConfig,
    cooldown_minutes: u64,
    last_tier: usize,
    cooldown: &mut AlertCooldown,
    now: u64,
) -> Option<usize> {
    if cfg.cap_usd <= 0.0 || cost <= 0.0 {
        return None;
    }
    let tier = cfg
        .warn_pcts
        .iter()
        .enumerate()
        .filter(|(_, pct)| cost >= cfg.cap_usd * **pct / 100.0)
        .map(|(i, _)| i + 1)
        .max()
        .unwrap_or(0);
    if tier == 0 || tier <= last_tier {
        return None;
    }
    let window = cooldown_minutes.saturating_mul(60);
    if now.saturating_sub(cooldown.fired_at(AlertKind::Budget)) < window {
        return None;
    }
    cooldown.mark_fired(AlertKind::Budget, now);
    Some(tier)
}
```

- [ ] **Step 5: 实现 state.rs**（StateFile 加字段，`alerts` 之后）

```rust
    /// ⑳ 已触发的最高预算档位（1-based，单调递进；0 = 未触发）。
    #[serde(default)]
    pub budget_tier: usize,
```
测试模块追加：
```rust
    #[test]
    fn budget_tier_defaults_zero_for_old_state() {
        let old = r#"{"snapshot":{},"transcript":{},"cache":{},"alerts":{},"last_error":null}"#;
        let st: StateFile = serde_json::from_str(old).unwrap();
        assert_eq!(st.budget_tier, 0);
    }
```

- [ ] **Step 6: 实现 notify.rs**（第 6 个便捷函数）

```rust
/// Convenience: budget tier reached (⑳; cost is the realtime estimate).
pub fn budget(pct: f64, cap: f64, symbol: &str) {
    send(
        "Budget Warning",
        &format!(
            "Session cost reached {:.0}% of budget {}{:.2}.",
            pct, symbol, cap
        ),
    );
}
```

- [ ] **Step 7: 运行全部新测试**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test budget_ 2>&1 | tail -5 && cargo test budget_tier 2>&1 | tail -5`
Expected: 两个命令均 PASS（4 + 1 个用例）

- [ ] **Step 8: 构建 + 全量单元**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1 | tail -3 && cargo test 2>&1 | tail -3`
Expected: 构建干净、全量单元 PASS（99 + 新增）

- [ ] **Step 9: Commit（用户手动执行）**

```bash
git add src/core/config.rs src/alert.rs src/core/state.rs src/notify.rs && git commit -m "feat: ⑳ [budget] 配置 + AlertKind::Budget + check_budget 档位单调/跨进程冷却 + state.budget_tier + notify::budget"
```

---

### Task 5: ⑳ render 接线 + doctor 读取 + P5-04/P5-08

**Files:**
- Modify: `src/compact.rs`（run_pipeline 预算档位块）
- Modify: `src/doctor.rs`（budget_check）
- Modify: `scripts/hudlib/cases.py`（P5-04、P5-08）

- [ ] **Step 1: compact.rs 接线**（`send_notifications` 之后、`state.alerts = cooldown.to_state();` 之前插入）

```rust
    // ⑳ 预算档位：基于实时估算成本（≈），档位单调 + 冷却跨进程去重
    let (rt_cost, _) = pricing::realtime_cost(data, &config.pricing);
    let budget_tier = alert::check_budget(
        rt_cost,
        &config.budget,
        config.alerts.cooldown_minutes,
        state.budget_tier,
        &mut cooldown,
        now,
    );
    if let Some(tier) = budget_tier {
        state.budget_tier = tier;
        crate::notify::budget(
            (rt_cost / config.budget.cap_usd) * 100.0,
            config.budget.cap_usd,
            &config.currency_symbol,
        );
    }
```
注意：Task 1 已把 `:114` 的成本计算改为 realtime_cost——本块复用同一 `rt_cost`，不要重复计算（若 Task 1 已执行，`rt_cost` 可复用 `effective_cost` 变量名处：把 Step 1 的 `let (rt_cost, _) = ...` 改为直接使用现有 `let (effective_cost, _) = pricing::realtime_cost(...)` 变量）。

- [ ] **Step 2: doctor.rs budget_check**（`pricing_check` 调用之后追加 `budget_check();`；文件末尾新增函数）

```rust
/// ⑳ 预算/告警冷却状态（信息项，恒 exit 0）：读 state.json 的
/// alerts 冷却记录 + budget_tier（单调最高档位）。
fn budget_check() {
    let state_path = match AppConfig::state_path() {
        Ok(p) => p,
        Err(_) => {
            println!("  [..] budget: state path unavailable");
            return;
        }
    };
    let state = StateFile::read(&state_path);
    if state.budget_tier == 0 && state.alerts.is_empty() {
        println!("  [..] budget: no alert records yet (render 后生效)");
        return;
    }
    let now = crate::core::state::now_secs();
    for (kind, ts) in &state.alerts {
        println!(
            "  [..] alerts: {:?} last fired {}s ago",
            kind,
            now.saturating_sub(*ts)
        );
    }
    if state.budget_tier > 0 {
        println!(
            "  [..] budget: tier {} reached (monotonic)",
            state.budget_tier
        );
    }
}
```

- [ ] **Step 3: 黑盒 P5-04 + P5-08**（cases.py P4 列表之后新增 P5 列表并接入 CASES）

```python
# --- v0.2（⑲⑳㉑ 成本哨兵批次）---
# P5-04 首次触发会发一次真实 OS 通知（budget 通知接线；可接受）。
P5 = [
    render_case("P5-01", "[pricing] 实时命中 ≈$ 合并组", "P5",
                {"exit": 0, "stdout_contains": ["≈$16.80 · 6.8k/5.0k tok"]},
                stdin=j(full_dict()), config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"cost_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"
                    "[pricing]\n"
                    "\"deepseek-v4-flash\" = { input = 0.001, output = 0.002 }\n"),
                note="⑲：stdin 累计 6800/5000 × 单价 = 16.8 → ≈$16.80 · 6.8k/5.0k tok（实时路径无 cache → ≈）"),
    render_case("P5-02", "无 [pricing] 透传合并组", "P5",
                {"exit": 0, "stdout_contains": ["$0.03 · 6.8k/5.0k tok"],
                 "stdout_not_contains": ["≈"]},
                stdin_file="json/full.json", config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"cost_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="⑲：未命中 → 透传官方 0.03 无 ≈；token 组照常显示"),
    render_case("P5-03", "网关零数据 cost_display — 降级", "P5",
                {"exit": 0, "stdout_contains": ["—"],
                 "stdout_not_contains": ["$0.00"]},
                stdin=j(full_dict(**{"cost.total_cost_usd": 0.0,
                                     "context_window.total_input_tokens": 0,
                                     "context_window.total_output_tokens": 0})),
                config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"cost_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="⑲：无任何成本/用量数据 → —（不显示 $0.00 假精确）"),
    render_case("P5-04", "[budget] 高成本触发档位单调", "P5",
                {"exit": 0,
                 "pre_state_json": {"equals": {"budget_tier": 3}},
                 "state_json": {"equals": {"budget_tier": 3}},
                 "state_json_same_as_pre": ["budget_tier"]},
                stdin=j(full_dict(**{"cost.total_cost_usd": 15.0})),
                config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"cost_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[alerts]\ncost_threshold_usd = 0\ncooldown_minutes = 10\n"
                    "[budget]\ncap_usd = 5.0\nwarn_pcts = [50, 80, 100]\n"
                    "[widgets]\n"),
                pre_render=True,
                note="⑳：15.0 ≥ 5.0×100% → tier 3；pre 触发（发一次真实 OS 通知）后二次 render 单调保持 3"),
    render_case("P5-08", "doctor 报告预算档位与冷却", "P5",
                {"stdout_contains": ["budget: tier 3", "alerts: Budget"]},
                args=["doctor"],
                stdin=j(full_dict(**{"cost.total_cost_usd": 15.0})),
                config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"cost_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[alerts]\ncost_threshold_usd = 0\ncooldown_minutes = 10\n"
                    "[budget]\ncap_usd = 5.0\nwarn_pcts = [50, 80, 100]\n"
                    "[widgets]\n"),
                pre_render=True,
                note="⑳：pre_render 触发 tier 3 → doctor 读 state.json 输出档位与冷却记录（exit 不断言——statusLine 检查依赖真实环境）"),
]
```

- [ ] **Step 4: 运行新用例 + 全量黑盒**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1 | tail -3 && python scripts/test_hud.py --case P5-04 --case P5-08 2>&1 | tail -6`
Expected: 2 个用例 PASS
然后全量：`python scripts/test_hud.py 2>&1 | tail -3` → Expected: `132/132 passed`（P5-04/08 先行，P5-01/02/03 与 Task 6/7 用例后置，因此本步总数 = 130 + 2）

- [ ] **Step 5: Commit（用户手动执行）**

```bash
git add src/compact.rs src/doctor.rs scripts/hudlib/cases.py && git commit -m "feat: ⑳ render 预算档位接线 + doctor budget_check 信息项 + P5-04/08 黑盒"
```

---

### Task 6: ㉑ weekly_report + history --weekly

**Files:**
- Modify: `src/core/history.rs`（init_schema 抽取 + WeeklyReport + weekly_report + 测试）
- Modify: `src/main.rs`（History 子命令 + print_weekly_report）
- Modify: `scripts/hudlib/cases.py`（P5-05、P5-07）

- [ ] **Step 1: 写失败测试**（history.rs 测试模块；需要 init_schema 可复用）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// 内存库 + 同一 schema（open() 抽取的 init_schema 复用）。
    fn mem_store() -> HistoryStore {
        let conn = Connection::open_in_memory().unwrap();
        let store = HistoryStore { conn };
        store.init_schema().unwrap();
        store
    }

    fn session(cost: f64, tokens_in: u64, tokens_out: u64, dur_ms: u64) -> SessionData {
        let json = format!(
            r#"{{"model":{{"id":"m","display_name":"m"}},
                "context_window":{{"used_percentage":1,"total_input_tokens":{tokens_in},
                "total_output_tokens":{tokens_out},"context_window_size":200000}},
                "cost":{{"total_cost_usd":{cost},"total_duration_ms":{dur_ms}}}}}"#
        );
        SessionData::from_stdin_json(&json).unwrap()
    }

    #[test]
    fn weekly_report_aggregates_five_metrics() {
        let store = mem_store();
        store.record_session(&session(1.0, 1000, 500, 60_000), 1, "glacier").unwrap();
        store.record_session(&session(3.5, 2000, 800, 3_600_000), 2, "glacier").unwrap();
        let r = store.weekly_report().unwrap();
        assert_eq!(r.sessions, 2);
        assert!((r.total_cost - 4.5).abs() < 1e-9);
        assert_eq!(r.total_tokens, 4300);
        assert_eq!(r.longest_duration_secs, 3600);
        assert!((r.highest_cost_usd - 3.5).abs() < 1e-9);
    }

    #[test]
    fn weekly_report_empty_db_is_all_zeros() {
        let store = mem_store();
        let r = store.weekly_report().unwrap();
        assert_eq!(r.sessions, 0);
        assert_eq!(r.total_tokens, 0);
        assert_eq!(r.longest_duration_secs, 0);
        assert!((r.highest_cost_usd - 0.0).abs() < 1e-9);
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test weekly_report 2>&1 | tail -5`
Expected: FAIL（`cannot find function weekly_report` / `cannot find type WeeklyReport`；`init_schema` 也缺失）

- [ ] **Step 3: 实现 history.rs**（open() 中 execute_batch 块抽取为 `fn init_schema(&self)`；追加 WeeklyReport + weekly_report）

```rust
/// ㉑ 周报五指标（MAX 口径，与 weekly_stats 的 AVG 口径独立）。
#[derive(Debug, Clone, Default)]
pub struct WeeklyReport {
    pub sessions: usize,
    pub total_cost: f64,
    pub total_tokens: u64,
    pub longest_duration_secs: u64,
    pub highest_cost_usd: f64,
}
```

open() 中 `conn.execute_batch(...)` 改为 `let store = Self { conn }; store.init_schema()?; Ok(store)`，并新增：

```rust
    /// Create the sessions table if missing（open 与内存测试共用）。
    fn init_schema(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS sessions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    started_at TEXT NOT NULL DEFAULT (datetime('now')),
                    duration_secs INTEGER NOT NULL DEFAULT 0,
                    total_cost_usd REAL NOT NULL DEFAULT 0.0,
                    total_tokens INTEGER NOT NULL DEFAULT 0,
                    agent_count INTEGER NOT NULL DEFAULT 0,
                    lines_added INTEGER NOT NULL DEFAULT 0,
                    lines_removed INTEGER NOT NULL DEFAULT 0,
                    mod_used TEXT NOT NULL DEFAULT ''
                );",
            )
            .map_err(|e| format!("create table: {}", e))
    }

    /// ㉑ 近 7 天周报聚合：会话数 / 成本合计 / token 总量 / 最长会话时长 / 最高成本单会话。
    pub fn weekly_report(&self) -> Result<WeeklyReport, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT COUNT(*), COALESCE(SUM(total_cost_usd),0), COALESCE(SUM(total_tokens),0),
                        COALESCE(MAX(duration_secs),0), COALESCE(MAX(total_cost_usd),0)
                 FROM sessions WHERE started_at >= datetime('now', '-7 days')",
            )
            .map_err(|e| format!("prepare: {}", e))?;
        let result = stmt
            .query_row([], |row| {
                Ok(WeeklyReport {
                    sessions: row.get(0)?,
                    total_cost: row.get(1)?,
                    total_tokens: row.get::<_, i64>(2)? as u64,
                    longest_duration_secs: row.get::<_, i64>(3)? as u64,
                    highest_cost_usd: row.get(4)?,
                })
            })
            .map_err(|e| format!("query: {}", e))?;
        Ok(result)
    }
```

- [ ] **Step 4: 运行确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test weekly_report 2>&1 | tail -5`
Expected: PASS（2 个用例）

- [ ] **Step 5: main.rs 子命令 + 输出**（`History,` 变体改为带 flag；分发处同步）

```rust
    /// Cross-session usage history (weekly stats, recent sessions, daily cost)
    History {
        /// ㉑ 周报五指标：会话数/成本/token 总量/最长时长/最高单会话
        #[arg(long)]
        weekly: bool,
    },
```
分发（`Commands::History => run_history(&config),`）：
```rust
        Commands::History { weekly } => run_history(&config, weekly),
```
run_history 开头加 weekly 分支 + 新函数：
```rust
fn run_history(config: &AppConfig, weekly: bool) -> Result<(), String> {
    let store = HistoryStore::open()?;
    if weekly {
        return print_weekly_report(&store, &config.currency_symbol);
    }
    ...
}

/// ㉑ 周报输出：空库全 —（不显示 0）；成本带 ≈（结账值可能为估算）。
fn print_weekly_report(store: &HistoryStore, symbol: &str) -> Result<(), String> {
    let r = store.weekly_report()?;
    println!("Weekly report (last 7 days):");
    if r.sessions == 0 {
        println!("  —");
        return Ok(());
    }
    println!(
        "  ≈{}{:.2} total | {} sessions | {} tok | longest {} | top session {}{:.2}",
        symbol,
        r.total_cost,
        r.sessions,
        format_history_tokens(r.total_tokens),
        format_history_duration(r.longest_duration_secs),
        symbol,
        r.highest_cost_usd,
    );
    Ok(())
}
```

- [ ] **Step 6: 黑盒 P5-05 + P5-07**（cases.py P5 列表追加）

```python
    render_case("P5-05", "history --weekly 五指标", "P5",
                {"exit": 0, "stdout_contains": ["Weekly report",
                                                "1 sessions",
                                                "top session"]},
                args=["history", "--weekly"], config=DEFAULT_CONFIG,
                pre_cmds=[
                    {"args": ["render"],
                     "stdin": j(full_dict(**{"transcript_path": "/a.jsonl"}))},
                    {"args": ["render"],
                     "stdin": j(full_dict(**{"transcript_path": "/b.jsonl"}))},
                ],
                remove_db=True,
                note="㉑：双 render 切换结账 1 条 → 五指标输出 1 sessions + top session；成本带 ≈"),
    render_case("P5-07", "history --weekly 空库 —", "P5",
                {"exit": 0, "stdout_contains": ["Weekly report", "—"]},
                args=["history", "--weekly"], config=DEFAULT_CONFIG,
                remove_db=True,
                note="㉑：空库五指标位输出 —（不显示 0）"),
]
```

- [ ] **Step 7: 运行新用例 + 全量**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1 | tail -3 && python scripts/test_hud.py --case P5-05 --case P5-07 2>&1 | tail -5`
Expected: 2 个用例 PASS
全量：`python scripts/test_hud.py 2>&1 | tail -3` → Expected: `134/134 passed`

- [ ] **Step 8: Commit（用户手动执行）**

```bash
git add src/core/history.rs src/main.rs scripts/hudlib/cases.py && git commit -m "feat: ㉑ history --weekly 五指标周报（MAX 口径独立查询）+ P5-05/07 黑盒"
```

---

### Task 7: ㉑ serve trend 字段 + 前端周曲线

**Files:**
- Modify: `src/serve.rs`（trend_json + build_api_json + HTML/JS）
- Modify: `scripts/hudlib/cases.py`（P5-06）

- [ ] **Step 1: serve.rs trend_json**（weekly_json 之后追加）

```rust
/// ㉑ 近 7 天日成本趋势（供周曲线）：open/query 失败或无数据 → available:false。
fn trend_json() -> String {
    let trend = HistoryStore::open()
        .ok()
        .and_then(|h| h.daily_cost_trend().ok());
    match trend {
        Some(days) if !days.is_empty() => {
            let days_json: Vec<String> = days
                .iter()
                .map(|(day, cost)| format!(r#"{{"day":"{}","cost":{}}}"#, day, cost))
                .collect();
            format!(r#"{{"available":true,"days":[{}]}}"#, days_json.join(","))
        }
        _ => r#"{"available":false,"days":[]}"#.to_string(),
    }
}
```

- [ ] **Step 2: build_api_json 加字段**（Task 3 的 format 字符串中 `"weekly":{},` 之后加 `"trend":{},`，实参追加 `trend_json(),`）

- [ ] **Step 3: HTML 曲线卡片**（`<div id="widgets-area" ...>` 之前插入）

```html
  <div class="card" id="trend-card" style="display:none;">
    <div class="card-title">Weekly cost trend</div>
    <div id="trend-bars" style="display:flex;align-items:flex-end;gap:6px;height:64px;margin-top:8px;"></div>
  </div>
```

- [ ] **Step 4: JS 渲染**（weekly 块之后、`update-time` 赋值之前）

```js
    const trend = data.trend || {};
    const trendCard = document.getElementById('trend-card');
    if (trend.available && trend.days && trend.days.length) {
      const bars = document.getElementById('trend-bars');
      bars.innerHTML = '';
      const max = Math.max(...trend.days.map(d => d.cost), 0.0001);
      trend.days.forEach(d => {
        const bar = document.createElement('div');
        bar.style.width = '28px';
        bar.style.height = Math.max(2, Math.round(d.cost / max * 60)) + 'px';
        bar.style.background = '#4c8dff';
        bar.style.borderRadius = '2px';
        bar.title = d.day + ' $' + d.cost.toFixed(2);
        bars.appendChild(bar);
      });
      trendCard.style.display = 'block';
    } else {
      trendCard.style.display = 'none';
    }
```

- [ ] **Step 5: 黑盒 P5-06**（cases.py P5 列表追加，D6 列表后）

```python
    serve_case("P5-06", "/api/data 含 trend 字段", "/api/data", 200, "application/json",
               expect_json=True, expect_json_fields=["trend", "pricing_configured"],
               note="㉑：趋势字段（空库 available:false）+ ⑲ pricing_configured 存在性"),
```

- [ ] **Step 6: 构建 + 回归**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1 | tail -3 && python scripts/test_hud.py --case P5-06 2>&1 | tail -4`
Expected: P5-06 PASS
全量：`python scripts/test_hud.py 2>&1 | tail -3` → Expected: `135/135 passed`

- [ ] **Step 7: Commit（用户手动执行）**

```bash
git add src/serve.rs scripts/hudlib/cases.py && git commit -m "feat: ㉑ serve trend 字段 + 前端周趋势柱状曲线 + P5-06 黑盒"
```

---

### Task 8: 黑盒全量 + 文档回写

**Files:**
- Modify: `scripts/hudlib/cases.py`（`assert len(CASES) == 130` → 138）
- Modify: `DEPLOY.md` / `COMPLETE.md` / `CHANGELOG.md` / `README.md`

- [ ] **Step 1: 总数断言更新**（cases.py 文件尾）

```python
assert len(CASES) == 138
```

- [ ] **Step 2: 全量回归**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -3 && python scripts/test_hud.py 2>&1 | tail -3`
Expected: 单元测试全绿（99 + 约 16 新增 ≈ 115，以实测为准）；黑盒 `138/138 passed`

- [ ] **Step 3: DEPLOY.md**（四处补充）

1. CLI 表 `history` 行改为：`claude-hud history [--weekly]`（`--weekly` = 周报五指标：会话数/成本/token 总量/最长时长/最高单会话，空库 `—`）
2. 配置参考追加：
```toml
[budget]
cap_usd = 5.0              # 会话成本上限（0 = 关闭预算，默认关闭）
warn_pcts = [50, 80, 100]  # 达到这些百分比时通知，每档一次（单调递进）
```
3. 配置参考 `[pricing]` 附近追加说明：状态栏成本双轨 —— 命中 `[pricing]` 时按 stdin 会话累计 token（in/out）× 单价重算并带 `≈`（实时路径无 cache 数据，必然低估；混合模型会话重算不准确，建议固定模型或依赖透传）；未命中透传 Claude Code 官方 `total_cost_usd`（含 cache）。模型 ID 以 stdin 的 `model.id` 为准（`claude-hud render --dump` 可查）。预算基于 `≈` 估算值触发。
4. 故障排除/通知节：预算告警在 render 进程判定（不开 dashboard 也能收到）；dashboard 不接预算（transcript 精确成本与 ≈ 实时语义冲突）。

- [ ] **Step 4: COMPLETE.md**（§20 ✅ 追加两项 + §21 批次行 + 路线图标注）

✅ 项追加：`· v0.2 成本哨兵（realtime_cost 双轨 + cost_display 合并单组 ≈$X · Xk/Xk tok + 零数据 — 降级 + [budget] 档位单调/跨进程冷却 + doctor 档位读取 + history --weekly 五指标 + serve 周趋势曲线 + 黑盒用例 138 例）`
§21 批次行：`| 2026-08-04 | v0.2 成本哨兵（⑲⑳㉑） | 实时成本状态栏/预算告警/成本周报 | 138 cases + N units |`（N = Step 2 实测单元数）
路线图：㉑ 项标注 `使用价值待用户反馈验证`（无条件的 ⬜ 项删除原则不变）。

- [ ] **Step 5: CHANGELOG.md**（`[Unreleased]` 与 `[0.3.0]` 之间插入）

```markdown
## [0.4.0] - 2026-08-04 (v0.2 成本哨兵批次)

### Added
- ⑲ 实时成本双轨：realtime_cost（stdin 累计 token × in/out 单价，无 cache → ≈）注入 render 路径；effective_cost（transcript 含 cache）保留 dashboard
- cost_display 合并单组 `≈$X.XX · Xk/Xk tok`（k 缩写 + 零数据 `—` 降级）
- serve `/api/data` 增 `pricing_configured`/`model_id` + 前端未配置单价提示；dashboard cost_display 行尾标注
- ⑳ `[budget]` 配置段（cap_usd + warn_pcts）+ check_budget 档位单调/跨进程冷却（复用 [alerts].cooldown_minutes）+ state.budget_tier + notify::budget + doctor budget_check
- ㉑ `history --weekly` 五指标周报（MAX 口径独立查询）+ serve `trend` 字段 + 前端周趋势曲线
```

- [ ] **Step 6: README.md** Usage 增行

```markdown
claude-hud history --weekly  # weekly five-metric report (cost/sessions/tokens/longest/top session)
```

- [ ] **Step 7: 全量验证 + Commit（用户手动执行）**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -3 && python scripts/test_hud.py 2>&1 | tail -3`
Expected: 全绿（单元 ≈115、黑盒 138/138）

```bash
git add scripts/hudlib/cases.py DEPLOY.md COMPLETE.md CHANGELOG.md README.md && git commit -m "docs: v0.2 成本哨兵（⑲⑳㉑）交付回写 — DEPLOY/COMPLETE/CHANGELOG/README + 黑盒 138 例"
```

---

## 自审清单

1. **Spec 覆盖**：⑲（realtime_cost 双轨/合并组/— 降级/未配置标注/doctor 已有 pricing_check）→ T1-T3；⑳（[budget]/档位单调/冷却/并存/doctor 读取）→ T4-T5；㉑（--weekly 五指标/serve 曲线/空库 —）→ T6-T7；文档 → T8。⑲ 的 doctor 信息项由现有 `pricing_check`（`N model(s) configured`）满足，无需新代码（T3 注明）。
2. **占位符扫描**：无 TBD/TODO；每步含完整代码。
3. **类型一致性**：`check_budget` 签名（`&mut AlertCooldown`）在 T4 定义与 T5 接线一致；`run_history(config, weekly)` T6 定义与分发一致；`format_tokens` T2 定义与测试一致；`inject_cost_realtime` 键组与 `inject_cost` 同构（T1/T3 补 `pricing_configured` 两处都加）。
4. **既有用例影响**：P2-05（≈$0.56 → ≈$16.80，T1）；P3-15 正则兼容（T2 验证）；P2-06/07 断言 `$0.03`/`¥0.03` 子串在新格式下仍匹配（T1 回归覆盖）；D6-02 字段扩展（T3/T7 增量）。
5. **计数**：黑盒 130 → 138（P5-01..08；P2-05 为更新非新增）。单元 ≈115（T8 Step 2 实测后回写 COMPLETE.md N）。
