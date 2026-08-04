# 第二期设计：字段契约 + 真实时间戳 + 成本正确性

> 日期：2026-08-03
> 来源：TASKS.md 三轮拷打决策（批次 B：任务 ③ ④ ⑭）+ brainstorming 会话（两节设计确认）。
> 依赖：第一期（批次 A ①②⑧⑫⑬）已完成——state.json 5 段数据通路、跨进程游标累计、告警冷却、doctor 检查项全部就绪。
> 本设计为分期方案的第二期；完成后进入第三期（批次 C：⑤⑥⑦⑨⑩⑪⑮⑯⑰⑱）。

## 1. 范围

第二期包含三个任务：

| 任务 | 主题 | 关键产物 |
|------|------|---------|
| ③ | 字段契约未验证（subagentStatusLine 命名疑云） | session.rs alias + 双形态 rate_limits + `render --dump` + doctor 契约探针 + 双命名夹具 |
| ④ | 全部指标基于伪时间（行号代秒） | 真实 ISO8601 解析 + timestamps_reliable 降级 + 卡顿检测复活 + epoch 分桶 + 真实压缩预测 |
| ⑭ | 成本正确性（三处三种货币符号 + 三方价格失真） | currency_symbol 统一 + `[pricing]` 重算 + context_bar token 展示 + doctor 校验 |

实施顺序：③（零依赖）→ ④（transcript.rs 核心）→ ⑭（消费④的 token 数据与 ≈ 原则）。每任务独立 commit，同第一期粒度。

## 2. 架构总览

三个任务不改共享层结构（state.json 5 段不变），各自强化数据进入管线的**输入契约**与**输出真实性**：

```
Claude Code stdin JSON（契约未验证）──③──▶ SessionData 双命名兼容 + 探针
transcript JSONL（时间戳是死的）   ──④──▶ TranscriptSummary 真实墙钟 + ≈ 诚实降级
USD 成本（符号乱 + 价格失真）      ──⑭──▶ effective_cost 三态 + $ 统一 + [pricing]
```

- ③ 是输入层修复：alias 让两种命名都进得来，探针让真实契约一次核对清楚。
- ④ 是数据层修复：transcript 的 `timestamp` 字段从"声明未用"变成"主时间轴"；`timestamps_reliable` 标志贯穿展示层，伪精确数字一律让位于 `≈` 估算。
- ⑭ 是计算层修复：货币符号统一到 USD（`$`），`[pricing]` 精确匹配重算成本，重算结果以 `≈` 诚实标注；未命中透传官方价零破坏。

### 2.1 三态成本流（⑭ 核心数据流）

```
pricing::effective_cost(&data, &summary, &config.pricing) -> (f64, bool /*estimated*/)

[pricing] 命中 + transcript 有累计 token  → 重算（input×in + output×out
                                           + cache_read×cr + cache_creation×cc）→ ≈ 标注
[pricing] 未命中                          → 透传 data.cost.total_cost_usd（官方价含 cache）→ 无标注
[pricing] 命中但无 transcript/token       → 透传 → 无标注（无数据可算，不算估算）
```

结果 `(cost, estimated)` 注入 WidgetConfig（`effective_cost` / `cost_estimated` 键），compact.rs 与 dashboard.rs 两条管线各自计算、各自注入。widget 签名零改动（8 个 widget 不受影响）。notify 消息的成本数字用同一 effective 值（`send_notifications` 增加 cost 参数）。

## 3. 模块布局

| 文件 | 变更 | 职责 |
|------|------|------|
| `src/core/pricing.rs` | **新建** | `PriceEntry`（input/output/cache_read/cache_creation，全 `#[serde(default)]`）+ `PricingTable = HashMap<String, PriceEntry>` + `effective_cost` 纯函数 |
| `src/core/session.rs` | 修改 | `subagent_status_line` 加 `alias = "subagentStatusLine"`；`RateLimits` 改 untagged 双形态（嵌套对象 + 扁平 `five_hour_pct`/`seven_day_pct`） |
| `src/core/transcript.rs` | 修改 | ISO8601 解析、首行可靠判定、`timestamps_reliable` 持久化、`last_tool_call_secs` 赋值、epoch 60s 分桶、删除 `base_time_secs`（AgentRecord 存绝对 epoch 后不再需要） |
| `src/core/config.rs` | 修改 | `currency_symbol: String`（默认 `"$"`）+ `[pricing]` 段（`HashMap<String, PriceEntry>`，全 default） |
| `src/core/state.rs` | 修改 | `TranscriptSegment` 加 `timestamps_reliable: bool`（from_state/to_state 透传） |
| `src/compact.rs` | 修改 | 管线计算 effective cost 并注入 WidgetConfig |
| `src/dashboard.rs` | 修改 | 同 compact 的注入逻辑 |
| `src/main.rs` | 修改 | `render --dump` 标志（输出原始 stdin + recognized/unknown 顶层键分类） |
| `src/doctor.rs` | 修改 | 契约探针检查项（内置双命名样例）+ `[pricing]` 校验 + 信息项 |
| `src/widgets/agent_detail.rs` | 修改 | 不可靠会话 elapsed 显示 `≈n` + 估算注明 |
| `src/widgets/context_bar.rs` | 修改 | compact 追加 `12.3k/45.6k tok`（in/out，k 缩写） |
| `src/widgets/cost_display.rs` | 修改 | 读 `currency_symbol` + effective_cost 优先 |
| `src/widgets/alerts.rs` | 修改 | 读 `currency_symbol` + effective cost 显示 |
| `src/notify.rs` | 修改 | 读 `currency_symbol` + effective cost 参数 |
| `fixtures/json/camel_contract.json` | **新建** | camelCase + 扁平 rate_limits 双命名夹具 |
| `fixtures/transcript/timestamps.jsonl` | **新建** | 全行真实 ISO8601（固定 ts，可精确断言） |
| `fixtures/transcript/no_ts.jsonl` | **新建** | 无 timestamp（降级路径） |
| `scripts/hudlib/cases.py` | 修改 | P2-01..P2-0N 黑盒用例 |

## 4. 数据流

### 4.1 任务③：双命名输入 + 探针

- `SessionData::from_stdin_json` 解析两种形态：`subagent_status_line`（snake_case，现状）与 `subagentStatusLine`（camelCase，alias）；`rate_limits` 嵌套对象（现状）与扁平 `five_hour_pct`/`seven_day_pct`（untagged 第二形态）。
- `render --dump`：stdout 输出原始 stdin JSON + 顶层键分类（recognized = SessionData 已知字段集合 / unknown = 其余）。
- doctor 契约探针：内置双命名样例各一份，解析后报告各顶层键识别状态（信息项，失败不算 failure——探针的目的就是暴露未知，不能反过来因未知而红）。

### 4.2 任务④：时间轴切换

**解析规则（拍板：首行带时间戳即可靠）**：

1. 从文件偏移 0 开始的新会话（`last_pos == 0` 且累计状态为空）：首条事件带有效 ISO8601 → `timestamps_reliable = true`，否则 `false`。
2. 从 state 恢复的会话（`from_state`）：沿用持久化的 `timestamps_reliable`（首条判定只适用于会话起点，增量场景不重判）。
3. 可靠会话内缺失/解析失败的行：用「最新已知真实 ts」推进（单调、不回退，连续缺失行共享同一 ts）。
4. 不可靠会话：维持现状行号递增（`current_secs += 1`），所有下游走估算路径。

**下游改造**：
- `last_tool_call_secs`：ToolUse 分支赋 `current_secs`（真实值），transcript.rs:338-340 空注释落实。
- `start_time_secs` / `end_time_secs`：真实 ts；`agent_detail` elapsed = 最新条目 ts − start ts。
- `stalled_agents`：真实触发（`is_active && current - last_tool_call > 30s`）。本期只保证该数据真实；卡顿通知接线属于任务⑪（第三期），本期不做。
- `token_timeline`：epoch 对齐 60s 桶（`current_secs / 60 * 60` 作桶键，跨进程稳定；进程 B 恢复后新行落入既有桶即合并，新桶才 push）。
- `compaction_prediction`：调用方传 `data.context_window.context_window_size`（去掉硬编码 200000）；`timestamps_reliable == false` 时返回 None（不显示伪精确）。

**降级展示**：不可靠会话下 `agent_detail` 的 elapsed 显示 `≈n`（估算标记，同拍板）。

### 4.3 任务⑭：成本计算

- `pricing::effective_cost` 纯函数：入参 data（透传源）、transcript summary（累计 token）、pricing 表；返回 `(f64, bool)`。
- compact.rs 管线：调用后 `widget_config` 注入 `effective_cost` / `cost_estimated`；cost_display 与 alerts widget 优先读注入值，未注入时回退 `data.cost.total_cost_usd` + `currency_symbol`。
- dashboard.rs 同法（用其内存累计 summary）。
- notify.rs：`send_notifications` 增加 `cost: f64` 参数，消息用管线 effective 值 + `currency_symbol`。
- context_bar：`ctx ████░░ 45% 12.3k/45.6k tok`（in/out 取 `context_window.total_input_tokens/output_tokens`，`k` 缩写：≥1000 时 `x.xk`）。

## 5. 并发与竞态规则

- 无新增共享状态：pricing 为纯函数，currency_symbol/pricing 表为只读配置。
- `timestamps_reliable` 并入 state.json `transcript` 段，写入方仍只有 render（现有规则不变）；dashboard 内存累计不持久化该标志（读 state 段恢复）。
- 会话切换（transcript_path 变化）重置：现有截断/换路径重置逻辑覆盖时间戳状态（AgentRecord 清空即丢弃旧时间轴）。

## 6. 错误处理矩阵

| 场景 | 行为 |
|------|------|
| timestamp 非 ISO8601 / 解析失败（非首条） | 视为缺失，用最新已知 ts 推进 |
| timestamp 缺失或解析失败（首条） | `timestamps_reliable = false`，估算路径 |
| 从 state 恢复且标志已持久化 | 沿用，不重判 |
| `[pricing]` TOML 非法 | `AppConfig::load()` 失败 → 现状 `unwrap_or_default` + stderr 警告（批次 C ⑤ 会加强，本次不扩） |
| `[pricing]` 命中但部分单价缺失 | 缺失按 0 计，重算值偏小 + `≈` 标注（诚实） |
| 单价为负 | doctor `[!!]` failure（含模型名定位） |
| 命中模型但无 transcript/token | 透传，不标 ≈ |
| `render --dump` 无 stdin | 走 render 正常错误路径（`[hud err]` + last_error，第一期行为不变） |

## 7. 测试计划

### 7.1 单元测试（cargo test）

- pricing：三态（命中重算 / 未命中透传 / 命中无 transcript）+ 边界（单价 0、cache 缺失、混合模型会话 ≈ 标注）。
- transcript：真实 ts 解析（elapsed 与 fixture 固定值吻合）；首条缺 ts → unreliable；增量恢复沿用标志；epoch 分桶（跨进程两段读取桶合并）；`last_tool_call_secs` 真实赋值；`stalled_agents` 真实触发。
- session：camelCase/snake_case 两种输入都解析；rate_limits 双形态解析。
- config：`currency_symbol` 默认 `$`；`[pricing]` 反序列化（缺 cache 字段补默认 0）。
- agent_detail：不可靠会话输出含 `≈`。

### 7.2 黑盒套件扩展（scripts/test_hud.py，P2-01..P2-0N）

| 用例 | 断言 |
|------|------|
| P2-01 | `camel_contract.json` stdin → agent 信息渲染（stdout 含代理模型名） |
| P2-02 | `render --dump` → stdout 含 recognized / unknown 分类与原始 JSON |
| P2-03 | `doctor` → 契约探针检查项输出（双命名样例识别结果） |
| P2-04 | `timestamps.jsonl` render → state.json `transcript.agents[].start_time_secs` 与 fixture 固定 ts 精确吻合 |
| P2-05 | 带 `[pricing]` 配置 render（命中模型 + transcript token）→ stdout 含 `≈$` 与重算值 |
| P2-06 | 无 `[pricing]`（或未命中）→ stdout 透传 `$` 原值，无 `≈` |
| P2-07 | `currency_symbol = "¥"` 配置 → cost_display 输出含 `¥`（四处接线验证取 compact 路径） |
| P2-08 | context_bar 输出含 `tok` 段与 k 缩写 |
| P2-09 | doctor `[pricing]` 校验：负单价 → `[!!]`；正常 → 信息项 N 个模型 |
| P2-10 | `no_ts.jsonl` render → state.json 无 reliable 标志或为 false（降级路径端到端） |

计数：`assert len(CASES) == 96` 更新为 96 + P2 数量。

## 8. 实施顺序（第二期内部）

```
③ 字段契约（session.rs + --dump + doctor 探针 + 夹具 + P2-01/02/03）
   └─ cargo test + 黑盒套件 + COMPLETE.md
④ 真实时间戳（transcript.rs + agent_detail + 夹具 + P2-04/10）
   └─ cargo test + 黑盒套件 + COMPLETE.md
⑭ 成本正确性（config + pricing.rs + compact/dashboard/notify + context_bar + P2-05..09）
   └─ cargo test + 黑盒套件 + COMPLETE.md
```

每任务一个 commit（`fix:` / `feat:` 前缀），批次完成后全量验证（cargo test + 全量黑盒 + doctor）。

## 9. 验收标准（汇总）

- [ ] camelCase 与 snake_case 两种 stdin 都能渲染代理信息；snake_case 向后兼容
- [ ] `render --dump` 输出 recognized/unknown 分类；doctor 契约探针检查项存在
- [ ] 带真实 timestamp 的 fixture：elapsed / 卡顿 / 压缩预测与真实时间吻合（单测精确断言 + 黑盒 state.json 断言）
- [ ] 无 timestamp 的 fixture：显示 `≈` 估算标记，不显示伪精确数字
- [ ] `stalled_agents` 真实触发（单测构造 >30s 无工具调用活跃代理）
- [ ] `compaction_prediction` 使用真实窗口大小；不可靠会话返回 None
- [ ] 默认显示 `$`；改 `currency_symbol` 后 compact/dashboard/notify/alerts 四处生效
- [ ] `[pricing]` 命中：按 transcript 累计 token 重算且带 `≈`；未命中：透传不变
- [ ] 坏单价（负数）→ doctor 报错并定位模型名
- [ ] 存量 config.toml 无 `currency_symbol`/`[pricing]` 时行为与现状一致（仅符号变 `$`）
- [ ] context_bar 状态栏可见 tokens in/out
- [ ] `cargo test` 全绿；黑盒套件全绿（96 + P2）
- [ ] COMPLETE.md 第 20/21 章状态回写（🟡 项更新、路线图加 Phase 2 行）
