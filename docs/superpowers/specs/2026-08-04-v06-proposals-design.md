# Claude HUD v0.6 — 需求拆分任务清单

> 来源：2026-08-04 需求梳理 brainstorm（定位：先自用后发布）。本文档把全部提案**仔细拆分**为一个个可执行 task，每个 task 附现状证据、方案要点与验收标准。
> **优先级已拍板（2026-08-04）**：批次顺序 **III → I → V → II → IV → VI**（修坏为先 → 自用痛点 → 小 UX → 复盘 → Web → 引入类）。批次内任务顺序待批次计划时定。
> **发布时机已拍板（2026-08-04）**：**做完再发** — III+I+V+II+IV 全部完成才发布首个 release（解锁 ⑰⑱）；VI 批次在此之前休眠。
> 贯穿原则：诚实降级（`—`/`≈`）· 失败可见（stderr 警告 + doctor 可查）· 不留死代码 · 每批完成 `cargo test` + 黑盒套件 + COMPLETE.md 同步。

---

## 批次总览

| 批次 | 任务 | 主题 | 依赖 | 执行序 |
|------|------|------|------|--------|
| **III** | ⑧⑨⑩ | 紧凑布局补全（**实证为缺陷**：3/6 出厂 Mod 切换即报错） | 无（共享 layout_from_mod 扩展点） | **1（止血）** |
| **I** | ①②③④ | 成本准确性 + 压缩预测（共享实时成本路径） | 无（各自独立） | **2** |
| **V** | ⑮ | Agent 卡顿归因 | 无 | **3** |
| **II** | ⑤⑥⑦ | 会话复盘与浏览（CLI 命令族 + 成本归因） | ⑤→⑥→⑦ | **4** |
| **IV** | ⑪⑫⑬⑭ | TUI 趋势面板 + Web 面板升级 | ⑫⑬ 复用 history.db 查询 | **5** |
| **VI** | ⑰⑱⑳ | 引入类（延期项，各带前置条件） | 前置条件见各任务 | **6（解锁后）** |

---

# 批次 I — 成本与预测

## 任务 ①：内置模型价格库（O1a）

### 现状（证据）
- `[pricing]` 需用户手工维护（`src/core/pricing.rs`：`PricingTable = HashMap<String, PriceEntry>`）；未命中 → 透传官方 `total_cost_usd`（准确但无估算）。
- `PriceEntry` 已含 `cache_read`/`cache_creation` 单价字段（pricing.rs:13-22），内置表可直接复用结构。

### 方案要点
1. 内置默认价格表（常量）：覆盖主流模型（claude-opus-4-7 / claude-sonnet-4-6 / claude-haiku-4-5-20251001 等官方价目，含 cache 单价）。
2. 查询优先级：用户 `[pricing]` > 内置表 > 透传（未命中模型维持现状，诚实降级）。
3. 刷新机制：价格表随二进制编译发布，`update` 换二进制即自动刷新 — 无需额外机制（方案 A 拍板）。
4. doctor `pricing:` 检查项标注"内置表命中"状态。

### 涉及
- `src/core/pricing.rs`（内置表 + 查询优先级）· `src/doctor.rs` · 文档（DEPLOY.md 配置节）

### 验收标准
- [ ] 未配置 `[pricing]` 时 render 命中内置表 → `≈` 估算出现；配置后用户值优先
- [ ] 未知模型 → 透传（无 ≈）
- [ ] 单元测试：优先级三态 + doctor 输出；黑盒用例 1+ 条

## 任务 ②：实时成本 cache 权重修正（O1b）

### 现状（证据）
- stdin JSON 的 cache token 字段**已在解析**：`ContextWindow.cache_creation_input_tokens` / `cache_read_input_tokens`（`src/core/session.rs:42-44`）。
- 但 `realtime_cost` 只用了 `total_input_tokens`/`total_output_tokens`（pricing.rs:56-57）→ `≈` 必然低估（DEPLOY.md 已注明）。

### 方案要点
1. `realtime_cost` 追加 `cache_read × 0.1 × p_in + cache_creation × 2 × p_in`（官方价目口径）。
2. cache 字段为 0/缺失 → 结果不变，维持现状。
3. 保留 `≈` 标注（仍无完整 cache 链路）。

### 验收
- [ ] 单元测试：有 cache 字段时估算值变化正确（对照官方价目口径）；无 cache 字段时回归不变
- [ ] 黑盒用例更新：含 cache 字段的 fixture

## 任务 ③：成本速率显示（O1c）

### 现状（证据）
- `cost_display` widget 显示总成本 + token 数（`src/widgets/cost_display.rs`）；stdin 已有 `total_duration_ms`。

### 方案要点
1. 成本 ÷ 活跃时长（小时）→ `≈$X/h` 追加 cost_display 组尾。
2. 零时长/无成本数据 → 不显示该段（诚实降级）。

### 验收
- [ ] 黑盒用例：含 duration 场景显示速率；duration=0 场景不显示
- [ ] 宽度感知：超宽时该组随 fit_line 正常丢弃

## 任务 ④：上下文趋势 + 压缩预警雷达（O2b + N1，合并：同源斜率）

### 现状（证据）
- `token_timeline`（v0.4，360 桶 6h 窗口）与 `token_rate` widget 的尾桶速率已有（`src/core/transcript.rs:76-84`）；alerts 冷却/去重机制已有（`[alerts].cooldown_minutes` + state 去重，⑳ 批次）。

### 方案要点
1. 斜率计算模块：首尾桶 token 增量 ÷ 时间 → 增长速率（与 token_rate 同源，提取复用）。
2. 线性外推：`ctx 68% · 压缩≈22m`（预计压缩时间点 = (100%−used%) ÷ 速率）。
3. 展示：紧凑 context_bar 组尾文本 + dashboard 上下文卡片标注。
4. 压缩临近（如 <15m）→ 桌面通知（复用冷却 + 跨进程去重；与预算告警并存）。
5. 数据不足（<2 桶 / 速率为 0）→ 不显示预测（`—`）。

### 验收
- [ ] 单元测试：外推数学三态（增长/平稳/数据不足）
- [ ] 黑盒用例：外推文本 + 相位固定断言
- [ ] 通知触发 + 冷却去重复用验证

---

# 批次 II — 会话复盘与浏览

## 任务 ⑤：`claude-hud sessions` 列表命令（N5 方案 A）

### 现状（证据）
- history.db 已有会话表（`src/core/history.rs`，HistoryStore）；现有 `history` 命令输出周报/近期/每日三块统计。

### 方案要点
1. 新子命令 `sessions`：分页（`--limit`/`--offset`）+ 可选日期过滤。
2. 输出口径与 `history`（统计）区分：纯会话列表（id/时间/成本/时长/代理/token）。
3. 空库显示 `—`；i18n zh/en 全量接入。

### 验收
- [ ] 黑盒用例：有数据列表、分页、空库 `—`、zh 表头
- [ ] `history` 现有输出不变（回归）

## 任务 ⑥：`claude-hud session <id>` 单会话详情（N5 方案 A）

### 现状（证据）
- 会话记录字段：成本/时长/代理数/token（`SessionRecord`）；transcript_path 已入库。

### 方案要点
1. 详情视图：模型/成本/时长/token 分解/代理列表。
2. transcript_path 存在时尾读补充：工具调用明细（复用增量解析器）。
3. 未找到 id → 明确报错（exit 1 + stderr）；`session -` 无此语义（与 mod use 区分）。

### 验收
- [ ] 黑盒用例：详情输出、不存在的 id 报错、空库
- [ ] i18n 接入

## 任务 ⑦：工具级成本归因排行（N3）

### 现状（证据）
- `tool_counts: HashMap<String, usize>` 已有计数（transcript.rs:13）；`token_attribution` P2 widget 存在；但**无逐工具 token 统计**。

### 方案要点
1. 数据源确认：若 token_attribution 已含逐工具 token → 直接 × 单价；若无 → `tool_counts × 单价 × 平均 token/调用` 估算（`≈` 标注，诚实）。
2. 展示：挂 `session <id>` 详情（任务⑥）+ dashboard 可选面板。
3. 依赖任务① 内置价格表（未命中模型 → 该段 `—`）。

### 验收
- [ ] 单元测试：归因聚合（估算路径 + 标注）
- [ ] 黑盒用例：排行输出降序 + 空数据

---

# 批次 III — 紧凑布局补全（O3b）

> **实证为缺陷，非锦上添花**：`layout_from_mod` 只实现 `minimal`/`activity` 两个布局 ID（`src/compact.rs:36-47`），其余返回 Err → render 错误标记上屏。6 个出厂 Mod 中 **3 个（obsidian-command=agent-centric、ember-night=kpi、noir-tabbed=contextual）切换即坏**。

## 任务 ⑧：agent-centric 三行布局 ✅ 已完成（2026-08-04 批次 III）

### 方案要点
1. `layout_from_mod` 增加 `"agent-centric"` 分支 → 新 widget 集（参考 ACTIVITY_WIDGETS：model_display/context_bar/agent_overview 前置，配 skills_mcp/token_rate/cost_display 等）。
2. 与 `compact_lines = 3` 联动（现有 chunking 渲染不变）。

### 验收
- [x] `mod use obsidian-command` 后 render 输出 3 行、无错误标记（黑盒用例 P7-01）

## 任务 ⑨：kpi 双行布局 ✅ 已完成（2026-08-04 批次 III）

### 方案要点
1. `layout_from_mod` 增加 `"kpi"` 分支 → KPI 优先 widget 集（cost_display/token_rate/model_display 等）。
2. `compact_lines = 2` 联动。

### 验收
- [x] `mod use ember-night` 后 render 无错误标记（黑盒用例 P7-02）

## 任务 ⑩：contextual 动态布局 ✅ 已完成（2026-08-04 批次 III）

### 方案要点
1. `layout_from_mod` 增加 `"contextual"` 分支：按会话活跃度切换 widget 集（空闲 → minimal 集；活跃 → activity 集）。
2. `compact_lines = 1` 联动。

### 验收
- [x] `mod use noir-tabbed` 后 render 无错误标记；空闲/活跃两态黑盒用例（P7-03 / P7-04）

---

# 批次 IV — TUI 趋势面板与 Web 升级

## 任务 ⑪：TUI 历史趋势面板（O3a）

### 现状（证据）
- serve `/api/data` 已有 `weekly`/`trend` 查询逻辑（serve 缓存 30s TTL）；dashboard 无历史视图。

### 方案要点
1. dashboard 新增趋势面板 widget（近 7 天成本柱状/折线，复用 history.db 查询）。
2. 历史库不可用 → `—` 占位；布局四种模式均可容纳（grid 占一格 / sidebar / focus / tabbed）。

### 验收
- [ ] 黑盒用例：有/无历史库两态 dashboard 输出（无交互断言）

## 任务 ⑫：Web SVG 成本趋势图（O4a）

### 现状（证据）
- `/api/data` 已返回 `trend` 字段；前端无图表（无构建链）。

### 方案要点
1. 服务端渲染 SVG 折线（零依赖，方案 A 拍板）→ HTML 内嵌。
2. 数据不足（<2 点）→ 占位文本。

### 验收
- [ ] curl 断言：HTML 含 `<svg` 与数据点；空趋势 → 占位

## 任务 ⑬：Web 会话列表 + 成本明细表（O4b）

### 方案要点
1. `/api/sessions` 新端点（分页，复用任务⑤查询逻辑）。
2. 前端表格：时间/成本/时长/代理/token；行点击 → 明细展开（复用任务⑥详情）。

### 验收
- [ ] curl 断言端点分页；前端 HTML 含表格标记（`{web_*}` 模板替换链无残留）

## 任务 ⑭：周环比（O4c）

### 方案要点
1. history.db 双周查询（本周 vs 上周：成本/会话数/token）。
2. This Week 卡片旁对比行（`+12%` / `−8%` / `—` 无上周数据）。

### 验收
- [ ] 黑盒/curl 用例：有/无上周数据两态

---

# 批次 V — 状态栏 UX

## 任务 ⑮：Agent 卡顿归因（N2）（批次 V 唯一任务）

### 现状（证据）
- `AgentRecord.last_tool_call_secs: Option<u64>` 已有（transcript.rs:44，卡顿检测在用）；**最后工具名未存储**。

### 方案要点
1. 每 agent 记录最后工具名（复用 ToolUseEntry 解析）。
2. 卡顿标记扩展：`stalled 3m · bash`（卡住 > stall_threshold_sec 时显示最后工具）；无工具记录 → 维持现状标记。

### 验收
- [ ] 单元测试：工具名记录；黑盒用例卡顿归因文本（固定相位）

## 任务 ⑯：紧凑分组轮播（N4）— ❌ 已砍（2026-08-04 拍板：默认关 + 自用定位 → 过度设计，见砍除项）

---

# 批次 VI — 引入类（延期项）

## 任务 ⑰：插件市场 `mod install user/repo`（O-引入）
- **前置：首次 release 发布**（当前仓库无 release，update/install 链路均为此短路）。
- 方案：GitHub raw 拉取 Mod toml → 校验 → 落盘 mods/ 目录 + `mod use` 联动。

## 任务 ⑱：Homebrew tap
- **前置：release 稳定 ≥ 2 版本**。方案：tap 仓库 + formula（二进制 + version.txt 口径）。

## 任务 ⑲：多会话聚合监控 — ⏸ 已移出 v0.6（2026-08-04 拍板：前置的 per-project 配置反转影响全局配置语义，单独 brainstorm 讨论）

## 任务 ⑳：更多主题预设
- **无前置**。方案：扩充出厂主题（预设 + 文案）。

## 任务 ㉑：扩展语言 ja / fr — ❌ 已砍（2026-08-04 拍板：翻译工作量大、自用零价值，见砍除项）

---

## 砍除/否决项（记录在案，避免重复讨论）

- **hex-2x3 / freeform 布局**：与现有四布局体系重复，收益低（O3 讨论中砍除）。
- **运行时价格 API**（O1a 方案 B）：网络依赖 + 失败降级复杂，选用内置表 + 二进制随版刷新。
- **前端图表库**（O4a 方案 B）：引入构建链，选用服务端 SVG。
- **Web 内嵌会话浏览**（N5 方案 B）：交互成本高，选用 CLI 命令族。
- **⑯ 分组轮播**（2026-08-04）：默认关 + 自用定位下为过度设计，YAGNI 砍除。
- **㉑ ja/fr 扩展语言**（2026-08-04）：翻译工作量大、自用零价值（i18n 框架保留，后续按需加）。

## 移出项

- **⑲ 多会话聚合监控**（2026-08-04）：前置 per-project 配置决策反转影响全局配置语义，**移出 v0.6，单独 brainstorm 讨论**；决策反转后任务重新立项。

## 关联约束

- ①②④共享实时成本路径，改动集中在 pricing/transcript 模块，可一次接线。
- ⑧⑨⑩共享 `layout_from_mod` 扩展点，彼此独立可并行；均需黑盒用例防回归。
- ⑤⑥⑦⑫⑬复用 history.db 查询，查询层先行。
- ⑮⑯依赖相位系统（CLAUDE_HUD_PHASE 确定性），黑盒用例沿用固定相位断言。

## 后续流程

1. 用户审阅本清单（增删改任意任务）。
2. 单独决策优先级与批次顺序（当前未定）。
3. 选定后每批次走 brainstorming → spec → writing-plans 标准流程（或直接并入 TASKS.md 作为新批次）。
