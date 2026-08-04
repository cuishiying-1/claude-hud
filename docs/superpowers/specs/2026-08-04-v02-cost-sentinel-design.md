# v0.2 成本哨兵批次（⑲⑳㉑）设计规格

> 源自 TASKS.md 第三轮拷打（2026-07-31 grill-me Q1-Q6 拍板）。主线 = **成本哨兵**：
> 实时成本状态栏（⑲）、预算告警下沉 render 进程（⑳）、成本周报增强已有出口（㉑）。
> 本文档落地为具体实现决策（代码级），作为实施计划的唯一依据。

## 1. 目标与范围

| 任务 | 目标 | 出口 |
|------|------|------|
| ⑲ | 状态栏实时展示会话累计 token + 成本（`≈$0.42 · 12.3k/45.6k tok` 合并单组） | compact 状态栏 + serve/dashboard 未配置标注 + DEPLOY.md |
| ⑳ | `[budget]` 配置段：cap + 渐进档位告警，跨进程去重，不开 dashboard 也能收到预警 | render 5s 管线 + state.json + doctor |
| ㉑ | `history --weekly` 五指标周报 + serve 周趋势曲线（增强既有出口，无新架构） | CLI + Web 面板 |

**不做**（拍板明确）：模型级归因管线（混合模型只标注不重算）；预算与 [alerts].cost_threshold 合并语义；agent 维度周报；趋势预测；任何推送通道。

## 2. 现状盘点（代码事实，实施前可复核）

- **render 管线已有 ⑦ 跨进程告警**（src/compact.rs:110-122）：`check_alerts` 加载 → 判定 → 回写 `state.alerts`（`HashMap<AlertKind, u64>` 冷却时间戳）。⑳ 的"下沉 render 进程"设施已就绪，只缺 budget 本体。
- **现有成本计算双源**：`pricing::effective_cost`（src/core/pricing.rs:30）基于 **transcript 累计 token**（含 cache_read/cache_creation，精确但滞后）；`inject_cost`（pricing.rs:52）在 compact 与 dashboard 两条管线共用注入 `effective_cost`/`cost_estimated`/`currency_symbol`。
- **stdin 数据**（src/core/session.rs）：`context_window.total_input_tokens`/`total_output_tokens` 为**会话累计**（Claude Code statusLine 自带）；`cost.total_cost_usd` 官方透传值；无累计 cache token。
- **cost_display widget**（src/widgets/cost_display.rs）：现显示 `≈$X.XX`（有效成本 + 估算前缀 + 阈值上色），无 token。
- **config**（src/core/config.rs）：`[alerts]`（AlertsConfig，4 字段含 cooldown_minutes=10）；`[pricing]` 为 `HashMap<String, PriceEntry>`；AppConfig 顶层字段直接 serde 反序列化。
- **state.json**（src/core/state.rs）：5 段 + `alerts: HashMap<AlertKind, u64>` + `previous_mod`；新字段加 `#[serde(default)]` 即可向后兼容。
- **history**（src/core/history.rs）：`sessions` 表字段齐全（duration_secs/total_cost_usd/total_tokens/...）；已有 `weekly_stats()`（avg 口径）与 `daily_cost_trend()`（日聚合 7 天）。main.rs:706 `run_history` 三块输出；serve.rs:90 `weekly_json()` + 前端 This Week 卡片。
- **notify.rs**：5 个便捷函数（context_critical/agents_complete/cost_threshold/rate_limit_warning/agent_stalled）。无桌面环境静默失败。

## 3. 任务⑲：实时成本状态栏

### 3.1 双轨计算（拍板落地）

新增**实时路径**计算函数（src/core/pricing.rs），与 transcript 精确路径并存：

```rust
/// 实时路径成本：stdin 会话累计 token（input/output）× 单价。
/// 实时路径无 cache 数据 → 必然低估 → 命中返回 (估算值, true)。
/// 未命中 [pricing] → 透传官方 total_cost_usd（含 cache，准）。
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
```

- **compact/render 路径**（run_pipeline + render_with_data）：注入与通知改用 `realtime_cost`；`effective_cost`（transcript 精确、含 cache）保留给 dashboard 复盘场景。
- `inject_cost` 保持原签名不动（dashboard 用）；新增 `inject_cost_realtime(data, config, widget_config)`（compact 用），注入同一组键（`effective_cost`/`cost_estimated`/`currency_symbol`）→ **widget 签名零改动**。
- compact.rs:114 通知路径的 `effective_cost` 计算同步换 `realtime_cost`（⑳ 预算亦基于此值）。

### 3.2 cost_display 合并单组形态

```rust
// 渲染结果（示例）：
//   ≈$0.42 · 12.3k/45.6k tok     [pricing] 命中，重算
//   $0.42 · 12.3k/45.6k tok       [pricing] 未命中，透传（token 照常显示）
//   —                             cost=0 且 in/out token=0 且未估算（网关无数据诚实降级）
```

- 格式：`{≈}{symbol}{cost:.2} · {in}/{out} tok`，k 缩写：`≥1000 → 1 位小数 + k`（`12345 → "12.3k"`；`450000 → "450k"`——≥100k 时去小数防溢出宽度）。
- 整组沿用现有上色逻辑（warn_threshold_usd → warning/success）与 `≈` 前缀（estimated）。
- **诚实降级 `—`**：cost==0 && in/out 全 0 && !estimated → 整组显示 `—`（不显示 `$0.00` 假精确；网关无 usage/cost 场景）。
- 宽度：整组为 1 组，超限由 ⑮ fit_line 组级截断处理；字段超 24 字符截断逻辑对成本组不适用（长度固定），不加。

### 3.3 三方模型三约束（拍板落地 + 一处澄清）

| 约束 | 落地 |
|------|------|
| 未命中可见 | **澄清**：验收标准第 1 条"未配置 [pricing] 时行为 = 现状透传"优先——状态栏**无 ≈**（透传官方值）；"可见性"由 serve/dashboard 承担：`/api/data` 新增 `pricing_configured: bool`（model.id 是否在 pricing 表），前端显示 `当前模型未配置单价 (model.id: xxx)` 提示行；dashboard cost_display 的 render_dashboard 行尾追加同文案（命中时省略）。 |
| 混合模型 | 命中时 `≈` 天然标注（重算 = 脏值）；DEPLOY.md 写明"混合模型会话重算不准确，建议固定模型或依赖透传"。不做模型级归因管线。 |
| 网关无 usage/cost | 3.2 的 `—` 降级。 |

### 3.4 doctor 信息项

`[..] pricing: N 个模型单价已配置`（恒 exit 0，N=0 也显示；不能假装校验模型存在性——拍板原话）。

## 4. 任务⑳：预算告警 [budget]

### 4.1 配置段（与 [alerts] 并存）

```toml
[budget]
cap_usd = 5.0              # 会话成本上限（0 = 关闭预算，默认）
warn_pcts = [50, 80, 100]  # 达到这些百分比时通知，每档一次（默认）
```

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BudgetConfig {
    #[serde(default = "default_budget_cap")]      pub cap_usd: f64,     // 0 = disabled
    #[serde(default = "default_budget_warn_pcts")] pub warn_pcts: Vec<f64>,
}
```

- AppConfig 加 `#[serde(default)] pub budget: BudgetConfig`。
- 冷却复用 `[alerts].cooldown_minutes`（拍板"10 分钟冷却跨进程生效"语义一致，不新增字段）。

### 4.2 状态与纯函数

- `AlertKind` 增 `Budget` 变体（state.alerts map 的键自动扩展，旧 state 兼容）。
- **StateFile 增 `#[serde(default)] pub budget_tier: usize`**——已触发的最高档位（1-based，单调递进，拍板"比每档独立冷却简单"）。

```rust
/// 预算档位检查（纯函数，无 OS 副作用）：
/// cost ≥ cap×pct/100 的最高档位 > 已触发档位 → 触发；冷却窗口内不重复发
/// （跨进程：A 进程 mark_fired 写入时间戳后 B 进程判定 now-last < window 不发）。
/// 与 check_alerts 同用 AlertCooldown（Budget 键），触发时内部 mark_fired。
pub fn check_budget(
    cost: f64,
    cfg: &BudgetConfig,
    cooldown_minutes: u64,
    last_tier: usize,
    cooldown: &mut AlertCooldown,
    now: u64,
) -> Option<usize> {
    if cfg.cap_usd <= 0.0 || cost <= 0.0 { return None; }
    let tier = cfg.warn_pcts.iter().enumerate()
        .filter(|(_, pct)| cost >= cfg.cap_usd * **pct / 100.0)
        .map(|(i, _)| i + 1).max().unwrap_or(0);
    if tier == 0 || tier <= last_tier { return None; }
    let window = cooldown_minutes.saturating_mul(60);
    if now.saturating_sub(cooldown.fired_at(AlertKind::Budget)) < window { return None; }
    cooldown.mark_fired(AlertKind::Budget, now);
    Some(tier)
}
```

> `AlertCooldown` 增两个访问器：`fired_at(kind) -> u64`（读 last_fired）与
> `mark_fired(kind, now)`（写入），check_budget 与 check_alerts 共用同一冷却映射。
> 设计取舍：档位单调 + 冷却双保险。单调防"回落再升重复发"；冷却防跨进程竞态
> （两个 render 同时读到旧档位）。`warn_pcts` 乱序/重复时按"最高档位"收敛，
> 不额外排序（文档注明按数值语义，配置建议升序）。

### 4.3 render 管线接线（compact.rs）

在 ⑦ 告警块之后追加（同一 cooldown 对象）：

```rust
let (rt_cost, _) = pricing::realtime_cost(data, &config.pricing);
let budget_tier = alert::check_budget(
    rt_cost, &config.budget, config.alerts.cooldown_minutes,
    state.budget_tier, &mut cooldown, now,
);
if let Some(tier) = budget_tier {
    state.budget_tier = tier;                       // 单调记录，随 state 持久化
    let pct = (rt_cost / config.budget.cap_usd) * 100.0;
    crate::notify::budget(pct, config.budget.cap_usd, &config.currency_symbol);
}
// check_budget 内部 mark_fired（Budget 键）→ 随 state.alerts 持久化（cooldown.to_state）
```

- **dashboard 不接 budget**（拍板"预警下沉 render 进程"；dashboard 用 transcript 精确成本，与预算的 ≈ 实时语义冲突——文档注明）。
- 与 `[alerts].cost_threshold_usd` 并存：两者独立判定、先到者先发，不做特殊合并（拍板原话）。

### 4.4 notify 与 doctor

- notify.rs 新增 `pub fn budget(pct: f64, cap: f64, symbol: &str)`（第 6 个便捷函数）。
- doctor：读取 state.json 的 `alerts` 冷却记录 + `budget_tier`，输出信息项
  `[..] budget: cap $X, tier Y/N, last fired <时距>`（无记录/关闭时省略或显示关闭）。
- 诚实性：预算基于 ≈ 估算值触发，DEPLOY.md 注明"预算基于估算值（≈）"。

## 5. 任务㉑：成本周报

### 5.1 history --weekly（五指标，全来自现有表字段）

- clap：History 子命令加 `--weekly` flag（`claude-hud history --weekly`）。
- 新查询（src/core/history.rs）：

```rust
/// 周报五指标（与 weekly_stats 的 avg 口径不同，独立查询）：
/// 会话数 / 成本合计 / token 总量 / 最长会话时长 / 最高成本单会话。
pub fn weekly_report(&self) -> Result<WeeklyReport, String> {
    // SELECT COUNT(*), COALESCE(SUM(total_cost_usd),0), COALESCE(SUM(total_tokens),0),
    //        COALESCE(MAX(duration_secs),0), COALESCE(MAX(total_cost_usd),0)
    // FROM sessions WHERE started_at >= datetime('now','-7 days')
}

#[derive(Debug, Clone, Default)]
pub struct WeeklyReport {
    pub sessions: usize,
    pub total_cost: f64,
    pub total_tokens: u64,
    pub longest_duration_secs: u64,
    pub highest_cost_usd: f64,
}
```

- 输出（空库全 `—`，不显示 0）：

```
Weekly report (last 7 days):
  ≈$12.34 total | 18 sessions | 450k tok | longest 52m | top session $3.20
```

- `≈` 前缀恒带（结账记录成本可能为估算值——诚实标注；空库时 `—` 不带 ≈）。
- 时长/容量复用 `format_history_duration` / `format_history_tokens`。
- `history`（无 flag）现有三块输出不变。

### 5.2 serve 周趋势曲线

- `/api/data` 增 `trend` 字段：`{"available":true,"days":[{"day":"2026-08-04","cost":0.42},...]}`（数据源 = `daily_cost_trend()`；空 → `available:false`）。
- 前端 This Week 卡片下方渲染 7 天柱状条（div 宽度比例，复用现有卡片样式；无 `available:false` 时不渲染曲线区域——拍板"无数据时段不渲染空曲线"）。
- 无新数据采集、无推送。

## 6. 测试计划

### 单元测试（src 内嵌，全部纯函数）

| 位置 | 用例 |
|------|------|
| pricing.rs | realtime_cost 三态：命中重算+≈（输入 1M×1e-6 + 输出 500k×2e-6 = 2.0）/ 未命中透传不标 ≈ / 命中但 token 全 0 透传 / 部分单价缺失仍标 ≈（偏小诚实） |
| cost_display.rs | 合并组格式：命中 ≈、未命中无 ≈、`—` 降级（0/0/0 且未估算）、k 缩写（12345→12.3k、450000→450k、999→999） |
| alert.rs | check_budget：档位递进（50%→tier1、80%→tier2）/ 单调不重复（tier2 后再到 80% 不发）/ 冷却窗口内不发、过期重发 / cap=0 关闭 / cost≤0 不发 / 乱序 warn_pcts 收敛最高档 |
| config.rs | [budget] 解析默认值（cap 0、warn_pcts [50,80,100]） |
| history.rs | weekly_report：空库全 0（→ `—`）/ 有记录五指标正确 / MAX 口径与 AVG 口径区分 |
| state.rs | budget_tier 新旧 state 兼容（旧文件缺失字段反序列化为 0） |

### 黑盒用例（scripts/hudlib/cases.py 增补）

| 用例 | 断言 |
|------|------|
| P5-01 | [pricing] 命中 render → 输出含 `≈` 与 `tok` 组（配置 fixture config，stdin 高 token 数据） |
| P5-02 | 无 [pricing] render → 透传 `$X.XX` 无 `≈` |
| P5-03 | 网关零数据 render → `—` |
| P5-04 | [budget] 配置 + 高成本 render 两次 → state.json `budget_tier` 从 0 → N 后保持不变（单调跨进程；首次触发会发一次真实 OS 通知，可接受） |
| P5-05 | `history --weekly` 有记录 → 五指标（pre_cmds 双 render 结账 seed，P4-01 模式） |
| P5-06 | serve `/api/data` 含 `trend` 字段（D6 扩展 expect_json_fields） |
| P5-07 | `history --weekly` 空库 → 全 `—` |
| P5-08 | doctor 读 state.json 输出预算档位（`budget: tier N` 信息项） |

> P2-05 断言随 ⑲ 双轨切换更新：`≈$0.56`（transcript 累计 300/130）→
> `≈$16.80`（stdin 累计 6800/5000 × 单价），transcript_copy 移除。

### 回归

- 全量黑盒 130 用例 + 新增 8（总数 138）+ `cargo test` 全绿。
- ⑮ 宽度截断对合并组无回归（组级截断覆盖）。

## 7. 文档与发布

- **DEPLOY.md**：⑲（≈ 语义 + model.id 指引 + 混合模型警告 + token 组格式）、⑳（[budget] 配置参考 + "预算基于估算值（≈）" + render 进程预警说明 + dashboard 不接预算的原因）、㉑（history --weekly + serve 周曲线）。
- **COMPLETE.md**：§20 表格（✅/🟡 更新）、§21 批次行（2026-08-04，⑲⑳㉑）、路线图标注 ㉑"使用价值待用户反馈验证"。
- **CHANGELOG.md**：[0.4.0] 段（Added ×N；Cargo.toml 版本按发布口径在 release 时 bump，本批次不动）。
- **README.md**：Usage 增 `history --weekly` 行。

## 8. 验收标准（TASKS.md 原文 → 落地对照）

| 验收 | 落地 |
|------|------|
| ⑲ 状态栏显示 `≈$X.XX · Xk/Xk tok`；未配置 [pricing] 时行为 = 现状透传 | §3.1/3.2 |
| [pricing] 命中/未命中/未配置三态输出正确；网关缺 usage → `—` | §3.2/3.3（P5-01/02/03） |
| DEPLOY.md 含 model.id 指引 + 混合模型警告 | §7 |
| 宽度超限时组级截断（⑮）正常生效 | §3.2（回归） |
| ⑳ 不开 dashboard，状态栏进程 cost 跨档发通知；10 分钟冷却跨进程生效 | §4.3（P5-04） |
| [budget] 三档渐进触发，每档一次；cap_usd=0 关闭 | §4.2（单元） |
| [alerts].cost_threshold_usd 与 [budget] 并存互不干扰 | §4.3（单元 + 回归） |
| state.json 冷却记录可被 doctor 读取检查 | §4.4（P5-04 复用） |
| ㉑ history --weekly 五指标输出正确（空库 `—`） | §5.1（P5-05） |
| serve 周曲线渲染正常；无数据不渲染空曲线 | §5.2（P5-06 + 手动） |
| 无新数据采集、无推送 | §5（范围约束） |
