# Claude HUD — 设计整改任务文档

> 来源：2026-07-31 三轮 `/grill-me` 拷打会话（第一轮 11 项设计缺陷 + 第二轮 9 项功能/使用问题 + 第三轮 6 项未来方向决策，共 24 项决策，全部拍板）。
> 文档中的"拍板"即最终决策，实施时无需再确认；标 ⬜ 的为推迟项。
> 每批完成后：`cargo test` + 黑盒套件（`python scripts/test_hud.py`）+ 更新 `COMPLETE.md` 状态标注。

---

## 批次总览

| 批次 | 任务 | 主题 | 依赖 |
|------|------|------|------|
| **A** | ① ②⑧ ⑫ ⑬ | 数据通路 + 增量状态 + 错误可见 + 通知防轰炸（基础设施） | 无 |
| **B** | ③ ④ ⑭ | 字段契约 + 真实时间戳 + 成本正确性 | A（state.json） |
| **C** | ⑤ ⑥ ⑦ ⑨ ⑩ ⑪ ⑮ ⑯ ⑰ ⑱ | 配置/生态/清理/UX | A（state.json）、B（stalled 数据） |

> ⑫（通知防轰炸）必须与 ① 同批落地：① 修好数据通路后 check_alerts 立即激活，无 ⑫ 则通知风暴随之而至。
> ⑬（错误可见性）的 last_error 落盘与 ① 共用 state 目录，顺路实现。

**贯穿原则**：诚实降级优先于伪精确（数据不可用显示 `—`/`≈`）· 失败可见（静默回退一律改为 stderr 警告 + doctor 可查）· 不留死代码（未实现的分支/变量/函数全部处置）。

---

# 批次 A — 数据通路与增量状态

## 任务 ①：dashboard / serve 数据通路卡死（TTY 阻塞）

### 问题现象
- 用户手动启动 `claude-hud dashboard`：进入 raw mode + alternate screen 后**黑屏卡死**，`q`/`Esc` 无效，只能 kill 进程。
- `claude-hud serve` 启动后**第一个 HTTP 请求挂死**。
- 全屏仪表盘（核心差异化卖点）与 Web 面板在真实使用中 100% 不可用。

### 具体原因（证据）
- `src/dashboard.rs:174-179` 与 `src/serve.rs:228-233` 的 `read_current_data()` 均执行：
  ```rust
  std::io::stdin().read_to_string(&mut buf).ok()?;
  ```
- `read_to_string` 在 stdin 为 **TTY** 时阻塞至 EOF（Ctrl+D）才返回。
- dashboard/serve 是用户交互启动，stdin 继承终端 → 必然 TTY。
- 只有 `render` 能拿到数据：Claude Code 状态行机制用管道喂 JSON。两个面板命令拿不到任何数据。
- `dashboard.rs:63-66` 还在首次 tick 时从 stdin 读 `transcript_path`——读不到 → Transcript 分析也永远不启动。
- 无任何文档提示"需要重定向 stdin"（DEPLOY.md 无相关说明）。

### 修复方案（拍板：state.json + IsTerminal 回退 + transcript tail）

1. **新增 `AppConfig::state_path()`**：`~/.claude/plugins/claude-hud/state.json`。
2. **render 写入快照**（复用 `main.rs:177-184` 的 `write_atomic` 原子写模式）：
   ```rust
   fn persist_snapshot(data: &SessionData) -> Result<(), String> {
       let path = AppConfig::state_path()?;
       let snapshot = serde_json::json!({
           "timestamp": SystemTime::now()...,          // 关键：时间戳用于过期判断
           "model": data.model,
           "context_window": data.context_window,
           "cost": data.cost,
           "rate_limits": data.rate_limits,
           "subagent_status_line": data.subagent_status_line,   // 实时代理信息必须落盘
           "transcript_path": data.transcript_path,             // 告诉面板该 tail 哪个文件
       });
       write_atomic(&path, &serde_json::to_string_pretty(&snapshot)?)
   }
   ```
   每次 render（5s 一次）顺带写入，文件几 KB，开销可忽略。
3. **dashboard / serve 读取逻辑**（两处统一封装）：
   ```rust
   // 1. 先查 stdin 是否 TTY：std::io::IsTerminal（Rust 1.70+，跨平台）
   //    非 TTY（echo ... | claude-hud dashboard）→ 读 stdin，保持现状（向后兼容）
   //    是 TTY（正常交互启动）→ 跳过 stdin，改读 state.json
   // 2. 读 state.json → 还原 SessionData + transcript_path
   // 3. timestamp 超过 30s（或文件不存在/解析失败）→ 显示"无活跃会话"占位，事件循环照常启动
   // 4. serve：每个请求读一次 state（已有 2s 轮询，天然配合）
   ```
4. **dashboard 的 TranscriptReader 用 state.json 里的 transcript_path 初始化**（修复 `dashboard.rs:63-66` 读不到路径的问题）。

### 验收标准
- [ ] 终端直接运行 `claude-hud dashboard`：不再卡死，显示"无活跃会话"占位，`q` 可正常退出
- [ ] `echo '{...}' | claude-hud dashboard`：仍可工作（stdin 兼容路径）
- [ ] 先跑一次 `render`（喂 stdin JSON），再开 `dashboard`/`serve`：能看到该数据
- [ ] `state.json` 生成、内容含全部快照字段、原子写（无半截文件）
- [ ] 黑盒用例：dashboard 无 stdin 时 3s 内可退出（`timeout` + 注入 `q`）
- [ ] serve 在无 state.json 时返回占位数据而非挂死

---

## 任务 ②⑧：增量读取失效 + 状态语义错乱（合并设计，必须同时实施）

### 问题现象
- 每 5 秒进程重生 → Transcript 每次从 0 全量重读（长会话 O(n²) CPU）。
- Git 每次 spawn 4 个子进程（每小时 2880 次）。
- 脚本 Widget 的 `refresh_seconds` 节流在紧凑模式完全失效（HTTP 轮询配置 300s 实际每 5s 打一次）。
- **若单独实施"offset 落盘"而不处理状态语义，P2 Widget 数据会从"慢但正确"变成"快但全错"**。

### 具体原因（证据）
- `compact.rs:90`：`TranscriptReader::new(path)` —— `last_pos` 每次从 0 开始，进程内"增量"跨进程不存在。
- `transcript.rs:191-195`：`agent_map`/`skill_map`/`mcp_map`/`tool_counts`/`total_tokens` 均为 **read_updates() 内局部变量**，每次调用清零 → 返回的 summary 是"本次读取行"的统计，不是会话累计。
- 6 个 P2 Widget 的 `update_transcript` 全部是**替换语义**（如 `alerts.rs:100-102`：`**guard = Some(summary.clone())`）——widget 存的 = "最近一次 read_updates 的结果"。
- 现状下数据"碰巧正确"的原因：每次全量重读 → summary 恰是全量。修完 offset 落盘后 summary 变增量 → 替换后数据倒退。
- `transcript.rs:262-266`：`subagent_stop` 在**局部** `agent_map` 里找 `start`——增量场景下 start 在上一次读取里，找不到 → 代理永远显示运行中。
- `alerts.rs:60-61`：呼吸动画用进程内 `frame` 计数（`frame % 40`），进程 5s 重生 → 相位永远重置，紧凑模式动画永不触发。
- `git_status.rs:74`：`probe_git()` 无条件 spawn 4 个子进程。
- `script_widget.rs:38-44`：`last_refresh: Mutex<Option<Instant>>` 进程内节流，进程一死即失效。

### 修复方案（拍板：累计状态下沉 reader + state.json 恢复 + 时间相位动画 + 缓存 TTL）

1. **`TranscriptReader` 持有累计状态**（`transcript.rs` 重构）：
   ```rust
   pub struct TranscriptReader {
       path: PathBuf,
       last_pos: u64,
       // 累计状态从局部变量提升为 self 字段
       agents: HashMap<String, AgentRecord>,
       skill_calls: HashMap<String, SkillCall>,
       mcp_calls: HashMap<String, McpCall>,
       tool_counts: HashMap<String, usize>,
       total_tokens: TokenTotal,
       token_timeline: Vec<TokenSnapshot>,
   }
   pub fn read_updates(&mut self) -> TranscriptSummary {
       // 只解析增量行，但合并进累计状态
       // subagent_stop 在累计 map 中能找到 start → 正确关闭代理
       // 返回**累计** summary（widget 的替换语义保持正确，无需改动 widget）
   }
   ```
2. **state.json 扩展**：存 `transcript_offset` + 累计统计 JSON 快照；新进程 `from_snapshot()` 恢复。会话切换（transcript_path 变化）自动重置（按 path 为 key）。
3. **动画改时间相位驱动**（跨进程一致，紧凑模式动画复活）：
   ```rust
   // alerts.rs 呼吸闪烁：
   let phase = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() % 8;
   let color = if phase < 4 { &theme.danger } else { &theme.warning };
   ```
4. **Git 探测缓存 + 双层 TTL**（state.json 内嵌或独立 `cache.json`）：
   - `branch --show-current` + `status --porcelain`：TTL 30s（每 6 次刷新真跑一次）
   - `rev-list ahead/behind`：TTL 60s（最贵，最不常变）
5. **脚本 Widget 节流落盘**：`last_run` 时间戳存 state.json；`refresh_seconds` 在紧凑模式下真正生效（输出滞后 ≤ TTL 秒为文档写明行为）。

### 验收标准
- [ ] 同一 transcript 连续 render 3 次：第 2 次起仅读增量（日志/文件偏移可验证）
- [ ] 跨进程累计正确：`tool_counts` 递增不倒退、`agents` 完整、`subagent_stop` 能关闭代理
- [ ] 长会话 CPU 不随时间增长（O(n²) 消除）
- [ ] alerts 呼吸在紧凑模式下跨 5s 快照可见颜色交替
- [ ] shell widget `refresh_seconds=30`：30s 内不重复执行（埋日志验证）
- [ ] git 缓存：30s 内连续 render 只 spawn 1 次 git（日志验证）
- [ ] `cargo test` 全绿；黑盒套件全绿

---

# 批次 B — 字段契约与真实时间戳

## 任务 ③：字段契约未验证（subagentStatusLine 命名疑云）

### 问题现象
- 紧凑模式下 `⚡ 2/3 agents` 可能**永不显示**，且无任何报错（静默失效）。

### 具体原因（证据）
- `src/core/session.rs:14`：`pub subagent_status_line: Option<SubagentStatusLine>` —— serde 默认按字段名字面匹配 JSON 键，只认 `subagent_status_line`。
- `DESIGN.md:96`（作者自己的设计文档）记录 Claude Code 实际下发的字段是 **`subagentStatusLine`（camelCase）**。
- 若为真 → 字段永远解析为 `None` → `agent_overview` 返回空串（`agent_overview.rs:19` `if agents.is_empty() { return String::new(); }`）→ 代理总览静默隐藏。
- 黑盒测试**循环论证**：`scripts/hudlib/cases.py:172` 用 snake_case `"subagent_status_line"` 喂数据——夹具沿用实现字段名而非真实契约，测试永远绿灯。
- 同类未验证字段：`rate_limits.five_hour/seven_day`、`context_window.current_usage` 各键。

### 修复方案（拍板：alias 双兼容 + doctor 契约探针 + 双命名夹具）

1. **`session.rs` 加 alias**（同时支持两种命名）：
   ```rust
   #[serde(default, alias = "subagentStatusLine")]
   pub subagent_status_line: Option<SubagentStatusLine>,
   ```
   `RateLimits` 的 `five_hour`/`seven_day`、`CurrentUsage` 各字段同步评估加 `alias` 或 `rename_all`。
2. **doctor 契约探针**：解析 stdin 后报告"已识别字段 / 未知字段"（顶层键对比），让真实契约一次核对清楚。`render` 增加 `--dump` 选项输出原始 stdin JSON 亦可。
3. **黑盒夹具双命名**：camelCase 与 snake_case 各一份，断言两种输入都能渲染出代理信息。夹具从真实契约取样，不再从实现字段名反推。

### 验收标准
- [ ] `echo '{"subagentStatusLine":{...}}' | claude-hud render` 输出代理信息
- [ ] 原 snake_case 输入仍工作（向后兼容）
- [ ] doctor 输出契约探针结果
- [ ] 黑盒用例 D1-22 双命名版本通过

---

## 任务 ④：Phase 2 全部指标基于伪时间（行号代秒）

### 问题现象
- 代理"耗时"显示行号（代理在第 200 行启动显示 `200s`）；卡顿检测永远不触发；压缩预测基于虚构速率；时间线 x 轴是假秒。

### 具体原因（证据）
- `transcript.rs:202-203`：
  ```rust
  // Rough timestamp increment (50ms per entry as fallback)
  current_secs = current_secs.saturating_add(1);   // 实际是每行 +1 秒
  ```
- `transcript.rs:246`：注释写着 "Update agent last-tool-call timestamp"，**下方代码为空**——`last_tool_call_secs` 从未被赋值，永远 `None`：
  - `stalled_agents()`（`transcript.rs:334-344`）`map_or(false, ...)` 恒假 → 卡顿检测死代码
  - `agent_detail.rs:66-74` 的 is_stalled 同样恒假
- `agent_detail.rs:89`：`let elapsed = agent.start_time_secs;` —— 把**开始行号**当已运行秒数显示。
- `alerts.rs` 调用 `compaction_prediction(pct, 200000)` —— 窗口大小**硬编码 200000**，未用真实 `context_window.context_window_size`。
- `transcript.rs:97`：`ToolUseEntry.timestamp: Option<String>` **已声明、从未使用**——真实时间戳在数据里躺着。

### 修复方案（拍板：真实时间戳主路径 + 不可靠时 `≈n` 标注估算）

1. **解析真实 ISO8601 时间戳**：`ToolUseEntry.timestamp` 解析为单调秒；每条事件带真实 ts（含 `subagent_start`/`subagent_stop` 若含 timestamp 字段则同步解析）。
2. **`ToolUse` 分支用真实 ts 赋值 `last_tool_call_secs`** → 卡顿检测复活。
3. **代理耗时 = 最新条目 ts − start ts**（真实墙钟）。
4. **`token_timeline` 按真实 60s 窗口分桶** → 压缩预测基于真实速率。
5. **`compaction_prediction` 的 `window_size` 从 `data.context_window.context_window_size` 传入**，去掉硬编码。
6. **诚实降级**：`TranscriptSummary` 加 `timestamps_reliable: bool`。不可靠（时间戳缺失/解析失败）时：
   - 卡顿计数、耗时、压缩预测**不显示伪精确数字**；
   - `agent_detail` 耗时显示 `≈n` 并注明估算（拍板：显示 ≈ 带说明，用户知道代理在跑但不被精确数字误导）。

### 验收标准
- [ ] 带真实 timestamp 的 fixture：elapsed 与真实时间吻合
- [ ] 无 timestamp 的 fixture：显示 `≈n` 标记，不显示伪精确数字
- [ ] `stalled_agents` 能真实触发（fixture 构造 >30s 无工具调用的活跃代理）
- [ ] `compaction_prediction` 使用真实窗口大小
- [ ] `cargo test` 全绿

---

# 批次 C — 配置 / 生态 / 清理

## 任务 ⑤：主题配置契约——文档教的写法会把整个配置文件静默作废

### 问题现象
- 用户按文档写 `theme = "dracula"`：主题不生效，且**整个 config.toml（layout/widgets/分隔符）全部被静默丢弃**，换回出厂默认，无任何警告。
- Mod 的 `overrides` 微调层形同虚设；`theme import` 只校验不落盘。

### 具体原因（证据）
- `DESIGN.md:330-347` 教的三种写法（字符串 / 字符串+overrides / custom 全表）与实现不符：
  - `config.rs:29`：`pub theme: Option<Theme>` —— 期望完整 Theme **表**，字符串解析失败。
  - `theme.rs:6-17`：11 个颜色 token **无 `#[serde(default)]`** → 只写 `[theme] accent = "..."` 也解析失败。
  - 注：DESIGN.md 级别 2 写法本身是非法 TOML（字符串值 + `[theme.overrides]` 表键冲突），正确形态是 `[theme] preset = "..."` + `[theme.overrides]`。
- `main.rs:108`：`let config = AppConfig::load().unwrap_or_default();` —— **任何解析失败 → 整体静默换默认值**。无警告、无 stderr、doctor 不报。
- `config.rs:102-107`：`ModTheme.overrides` 字段已定义；`main.rs:152-166` `load_theme` 只取 `mod_theme.preset`，**overrides 从未被应用**。
- `main.rs:424-430`：`theme import` 仅解析校验，打印"导入成功"但不写任何文件。

### 修复方案（拍板：ThemeRef untagged + 颜色默认值 + 失败警告 + overrides 双来源 + import 落盘）

1. **`ThemeRef` untagged 枚举**（字符串与表两形态都接受）：
   ```rust
   #[derive(Deserialize)]
   #[serde(untagged)]
   pub enum ThemeRef {
       Preset(String),     // theme = "dracula"
       Full(Theme),        // [theme] ...（表）
   }
   ```
   `config.rs` 的 `theme` 字段改为此类型。
2. **`theme.rs` 11 个颜色 token 补 `#[serde(default)]`**（默认值 = `Theme::default()` 的 nord 色）→ partial 表可用。
3. **失败不再静默**：`AppConfig::load()` 失败时 `eprintln!` 警告 + 回退默认；**doctor 增加"config 解析健康"检查项**，输出 `[!!]`。
4. **overrides 真正生效**：`load_theme` 顺序 = preset → `[theme.overrides]` → `mod.theme.overrides`（后者优先级高，拍板）。`HashMap<String, toml::Value>` 遍历做类型化合并（或用 serde-value，约 40 行）。
5. **`theme import` 落盘**：`import <file>` 校验后写入 config.toml 的 `[theme]` 段并提示（拍板：直接落盘，不搞 `--apply`；`--check` 留给只想校验的人，可选）。
6. **DESIGN.md 修正**：三级配置深度按正确 TOML 形态重写。

### 验收标准
- [ ] `theme = "dracula"` 字符串可用
- [ ] `[theme]` 部分表可用（只写 1-2 个颜色不报错）
- [ ] 坏 config：stderr 有警告 + doctor 显示 `[!!]`
- [ ] overrides 生效，且 mod 级覆盖 config 级
- [ ] `theme import` 后 config.toml 含 `[theme]` 段
- [ ] 单元测试覆盖 untagged 两种形态

---

## 任务 ⑥：Mod 系统是"主题快捷切换器"，不是文档宣传的"完整 UI 配置包"

### 问题现象
- `mod save` 生成固定模板（不是当前配置快照）；`mod use` 任何名字都成功；`use -` / `@scene` / `pick` 全部占位；Mod 的 layout/widgets/overrides 不驱动渲染——6 个出厂 Mod 的真实差异只有主题。

### 具体原因（证据）
- `main.rs:390-414` `config_to_mod`：写死 `compact="activity"`、`dashboard="grid-2x2"`、`theme preset="nord"`、widgets 空表——DEPLOY.md 宣称"保存当前配置为新 Mod"，实现是"生成固定模板文件"。
- `main.rs:297-304` `mod use`：任何名字都写入 `active_mod` → 查不到时 load_theme 静默回退默认主题（无校验、无报错）。
- `main.rs:292-295`：`use -` 打印 "not yet persisted"。
- `mod use @daily`：`@daily` 直接被当作 mod 名写入 → 查不到 → 静默回退（**场景别名是坏的**）。
- `main.rs:380-385`：`pick` 打印提示 + 调用 list。
- layout 不驱动渲染：`compact.rs` 只读 `config.compact_layout`；`dashboard.rs` 只读 `config.dashboard.default_layout`；`mod.layout.compact/dashboard/compact_lines`、`mod.animation.effects`、`mod.widgets.*` 全为元数据。

### 修复方案（拍板：路线 A 骨架先行，6 项）

| 项 | 修复 |
|----|------|
| `mod use` 校验 | 内置 + 用户目录都加载不到 → 报错退出（非零码），不写 config |
| `mod save` 真实快照 | 从**当前生效配置**生成：当前 theme（含 overrides 合并结果）、当前 `compact_layout`、`runtime_overrides.compact_lines`、`[widgets.*]` 段整体写入 `mod.widgets`、`[mod.animation]` 写 `enabled=true` + 空 effects（拍板：保留段结构，不渲染） |
| `mod use -` | state.json 存 `previous_mod`，`-` 切换回去 |
| `mod use @scene` | 场景别名校验：匹配预设 scene 才生效，否则报错 |
| layout 灌入渲染 | `compact.rs`：`mod.layout.compact_lines` 优先于 `theme.compact_lines`；`mod.layout.compact` 为 `minimal`/`activity` 时映射到对应 widget 数组，其余布局 ID 明确报错"布局未实现" |
| `mod pick` | 最简序号选择器（约 30 行）：列序号+名称，输入序号切换（拍板：拒绝占位） |

**第二步（后续迭代，不在本次）**：`mod.widgets.*` 灌入 widget 配置查询（复用任务 ⑤ 的 overrides 合并层）。
**文档修正**：DEPLOY.md 的 `mod save` 描述、`mod pick` 状态同步更新。

### 验收标准
- [ ] `mod use nonexistent` 报错退出（exit 非 0），config 未被污染
- [ ] `mod save my-custom` 后 `mod export my-custom` 内容包含当前 widgets 配置
- [ ] `use A` → `use B` → `use -` 回到 A
- [ ] `use @daily` 生效；`use @unknown` 报错
- [ ] `mod use obsidian-command`（agent-centric，compact_lines=3）实际渲染 3 行
- [ ] `mod pick` 可通过序号切换

---

## 任务 ⑦：三个 Widget 的 ANSI 颜色系统失效（空字符串上色）

### 问题现象
- rate_limits 的"超 90% 变红"、session_stats 的三色区分、token_attribution 的前缀/百分比着色——**全部从未在终端呈现**，输出基本全默认色。

### 具体原因（证据）
`ansi_fg(text, hex)` 实现为 `\x1b[38;2;r;g;bm{text}\x1b[0m`（`ansi.rs:7-13`）。四处调用把**空字符串**包进颜色、把要着色的数字放在颜色代码外面：
- `rate_limits.rs:27-34`：`ansi::ansi_fg("", fc)` + `fh` 在外 → 阈值变色从未发生
- `session_stats.rs:53-64`：同模式，`⏱`/`·`/数字全在空 wrap 外
- `token_attribution.rs:34-40`：`"top:"` 空 wrap + `pct` 在外
- `cost_display.rs:25-28`：半错——`¥` 符号上色，数字 `1.42` 在色外

黑盒测试只断言"输出非空/含子串"，不断言 ANSI 结构 → 测试永远抓不住。

### 修复方案（拍板：4 处修复 + ANSI 结构断言）

1. 统一模式：**所有要着色的文本整体包进 `ansi_fg`，含数字**：
   ```rust
   // rate_limits 修复后
   format!("5h:{} 7d:{}",
       ansi::ansi_fg(&format!("{:.0}%", fh), fc),
       ansi::ansi_fg(&format!("{:.0}%", sd), sc))
   ```
   session_stats / token_attribution / cost_display 同法。
2. **黑盒夹具加 ANSI 结构断言**：匹配 `\x1b\[38;2;\d+;\d+;\d+m[^\x1b]+` 且**色内文本非空**；现有"含子串"断言升级为"含 ANSI 色且文本在色内"。

### 验收标准
- [ ] rate_limits 超阈值时数字为红（输出含 `\x1b[38;2;` 且数字在色内）
- [ ] session_stats / token_attribution / cost_display 三色生效
- [ ] 新黑盒断言通过；全量套件通过

---

## 任务 ⑨：历史库是只写不读的数据孤岛

### 问题现象
- SQLite 建了、表建了、三个查询写了，但没有任何消费代码——用户从第一天到最后一天都看不到"本周总费用"等设计承诺的统计。

### 具体原因（证据）
- `dashboard.rs:91-93`：唯一写入路径——仅仪表盘 `q`/`Esc` 退出时 `record_session`。
- `render`（主要使用形态）没有任何 `HistoryStore` 调用 → 紧凑模式用户的历史库永远接近空。
- 读取路径为零：`dashboard.rs` 无历史面板；`serve.rs` 的 `/api/data` 无历史字段；CLI 无 `history` 子命令。
- `dashboard.rs:55`：`HistoryStore::open().ok()` 静默吞失败。
- DESIGN.md:1011-1022 承诺的"每日费用趋势 / Token 用量折线 / 代理平均耗时 / 高峰分析"在界面不存在。

### 修复方案（拍板：会话切换结账 + history 子命令 + serve 带摘要；dashboard 面板推迟）

1. **render 会话切换结账**（复用任务 ① 的 state.json）：
   ```rust
   // render 每次启动：
   // 1. 读 state.json → last_path + last_data（上一会话最后快照）
   // 2. 若 last_path != 当前 transcript_path → 用 last_data 给上一会话结账
   //    history.record_session(&last_data, agent_count, active_mod)
   // 3. 更新 state.json：last_path / last_data
   ```
   边界：Claude Code 退出时最后一条会话可能不结账（render 不再被调用）——第一版接受，文档写明。
2. **`claude-hud history` 子命令**（约 60 行）：周统计（会话数/总费用/总 token/平均时长/平均代理数）+ 最近 10 条 + 近 7 天每日费用表。
3. **`serve` 的 `/api/data` 追加 `weekly` 摘要**：前端加卡片（本周费用/会话数）。
4. **dashboard 历史面板**：推迟（拍板，⬜ 明确标注，不假装存在）。
5. `HistoryStore::open()` 失败时 eprintln 警告。

### 验收标准
- [ ] 连续两次 render（不同 transcript_path）后 history 表有记录
- [ ] `claude-hud history` 输出正确
- [ ] `/api/data` 含 `weekly` 字段，前端卡片渲染
- [ ] 会话切换不产生重复记录（同 path 不重复结账）

---

## 任务 ⑩：Windows 是一等公民，但 Shell Widget 在 Windows 永远失败

### 问题现象
- Windows 用户配置 `type = "shell_output"` Widget → 每次渲染输出 `shell: <错误>`，功能 100% 不可用，文档无提示。

### 具体原因（证据）
- `scripting.rs:77-91`：
  ```rust
  let output = Command::new("sh").arg("-c").arg(command).output()...
  ```
  `sh` 在原生 Windows 上不存在（安装脚本只放二进制，不保证 Git Bash/WSL 在 PATH）。
- 发布承诺三平台：`plugin.json` `platforms: ["macos","linux","windows"]`、release.yml windows-x64、install.ps1。
- `DEPLOY.md:235` 已承认同类问题（"Windows 需要 Git Bash 或 WSL 提供 git 命令"）——Git 探测做了依赖声明，Shell Widget 没有。
- 连带：`probe/system.rs:35-42` `time_now()` Windows 分支返回固定 `--:--:--`（未接线死代码，同模式）。

### 修复方案（拍板：cmd /C 分支 + 文档说明 + 删死代码）

1. **`run_shell_command` 平台分支**：
   ```rust
   pub fn run_shell_command(command: &str) -> Result<String, String> {
       #[cfg(windows)]
       let output = { let mut c = Command::new("cmd"); c.arg("/C").arg(command).output() };
       #[cfg(not(windows))]
       let output = { let mut c = Command::new("sh"); c.arg("-c").arg(command).output() };
       // 统一错误处理（保持现有结构）
   }
   ```
2. **DEPLOY.md Shell Widget 章节**加平台说明：Unix `sh -c` / Windows `cmd /C`，复杂命令建议写成 `.bat`/`.sh` 脚本再调用。
3. **删除 `time_now()` 与 `memory_mb()`**（拍板：未接线的平台残缺死代码，YAGNI；`memory_mb` 仅 Linux 实现，同处理）。

### 验收标准
- [ ] Windows 上 shell widget 通过 `cmd /C` 执行（CI windows 构建 + 冒烟或用文档标注的手工验证）
- [ ] `time_now` / `memory_mb` 及其唯一引用已删除，`cargo build` 通过
- [ ] DEPLOY.md 平台说明已更新

---

## 任务 ⑪：占位功能与死代码收尾

### 问题现象
- `completion` 命令自我指涉（照提示执行会无限循环）；dashboard `1-9` 空分支；`last_agent_count` 变量写好了从没用过；两个通知函数未接线。

### 具体原因（证据）
- `main.rs:458-468`：打印 `source <(claude-hud completion bash)`——用户执行后又调用自身打印同样文本，**永不产生补全**。
- `dashboard.rs:96-98`：`KeyCode::Char('1'..='9') => { // future }` 空分支。
- `dashboard.rs:52` 声明 `last_agent_count`、`:78` 传入 `check_alerts`、`:92` 传入 `record_session`——但 `check_alerts` 函数体（182-193 行）**从未使用该参数**；而 `notify.rs:26-31` 的 `agents_complete` 函数已写好等这个边沿检测。
- `notify.rs:50-55` `agent_stalled` 已写好未接线（依赖任务 ④ 修复后 stalled_agents 才有数据）。

### 修复方案（拍板：四项处置，通知接线与任务 ④ 一起交付）

| 项 | 处置 | 成本 |
|----|------|------|
| `completion` | **真实现**：加 `clap_complete` 依赖（clap 官方配套），`clap_complete::generate(shell, &mut Cli::command(), "claude-hud", &mut stdout)` | 1 依赖 + 15 行 |
| dashboard `1-9` 空分支 | **删除**（真实现标签切换时再加） | 3 行 |
| `agent_stalled` 接线 | dashboard 循环中：`summary.stalled_agents()` 非空时发通知；**去重**（记录已通知代理名，同一代理只通知一次，防 5s 轰炸） | ~15 行 |
| `agents_complete` 接线 | 边沿检测：上一 tick 有活跃代理、当前为 0 → `notify::agents_complete(count)`，更新 `last_agent_count`（变量终于被用上） | ~10 行 |

### 验收标准
- [ ] `claude-hud completion bash/zsh/fish` 输出真实补全脚本
- [ ] 卡顿代理出现时收到**一次**通知（不重复轰炸）
- [ ] 代理全部结束时收到一次通知
- [ ] `dashboard` 按 `1`-`9` 无副作用（删除空分支后）
- [ ] 全项目无"写好了但没用"的变量/函数（元数据字段除外）

---

# 第二轮拷打（功能与用户使用角度）— 新增任务 ⑫-⑱

> 2026-07-31 第二轮 `/grill-me`：9 问（Q1-Q9）从功能完整性与用户实际使用角度拷打，全部拍板。
> Q2（安装占位符）/Q4（备份还原）/Q8（全局配置）合并为任务 ⑰；其余一问一任务。

## 任务 ⑫：通知防轰炸——任务①修好后必然引爆的事故（Q1）

### 问题现象
check_alerts 在 TUI 主循环每帧调用（默认 tick 500ms），阈值硬编码、无去抖、无开关；任务①修好数据通路后立即激活，条件满足时每秒 2 条系统通知直到条件消失。

### 具体原因（证据）
- `src/dashboard.rs:78` — `check_alerts` 每帧调用（`refresh_interval_ms` 默认 500ms，config.rs:53）
- `src/dashboard.rs:182-193` — 阈值硬编码：context ≥95% / cost ≥$10 / rate limit ≥90%，无配置路径
- `src/notify.rs:4` — `send()` 无幂等/去抖，每次直接发 toast
- 当前不炸只是因为 `read_current_data` 读不到数据（任务①的坑），修复后代码立即激活

### 修复方案（拍板：冷却期 + [alerts] 配置段 + 默认开）
1. 每类通知**独立冷却 10 分钟**：dashboard 进程内内存 `HashMap<AlertKind, Instant>`（进程内足够，check_alerts 仅在 dashboard 跑）
2. `[alerts]` 配置段（全字段 `#[serde(default)]`，存量配置无感）：
   ```toml
   [alerts]
   context_critical_pct = 95.0    # 0 = 关闭该类
   cost_threshold_usd = 10.0      # 0 = 关闭该类
   rate_limit_pct = 90.0          # 0 = 关闭该类
   cooldown_minutes = 10
   ```
3. 默认开 + 冷却；`0` 即关闭该类，不需要额外 enabled 字段
4. 通知消息货币符号随任务⑭统一（notify.rs 的 `¥` 硬编码）

### 验收标准
- 条件持续满足时，10 分钟内同类通知 ≤1 条
- `[alerts]` 改阈值即时生效；设 `0` 关闭该类且不发通知
- 冷却状态仅内存；dashboard 重启后重置（可接受）
- `cargo test` 通过；黑盒套件无回归

## 任务 ⑬：状态栏静默失效——用户永远不知道 HUD 坏了（Q3）

### 问题现象
render 出错时错误只写 stderr（Claude Code statusLine 只消费 stdout），用户看到空白/残影状态栏；transcript 缺失时连 stderr 都没有。唯一诊断入口 doctor 无触发理由。

### 具体原因（证据）
- `src/main.rs:119-121` — render 错误只 `eprintln` + `exit(1)`，stdout 为空
- `src/compact.rs:86-88` — transcript 不存在时静默 `return`
- process-per-refresh 架构下错误证据随 5 秒进程消亡，无法追溯

### 修复方案（拍板：故障可见 / 正常瞬态安静）
1. render 失败 → **stdout 输出截断错误标记**（≤80 字符）：`[hud err] <摘要> — run 'claude-hud doctor'`；保留 stderr 完整错误 + 非零退出码（手动跑 render 的用户仍见完整错误）
2. **last_error 落盘**：失败时写 state 目录 `last_error`（ISO8601 时间戳 + 摘要，单文件覆盖写，与任务① state.json 同目录顺路实现）；doctor 检查项报告 `last render failure: <时间> <摘要>`，无则 `[ok] no recent failures`
3. transcript 缺失**保持静默**（新会话前几秒是正常瞬态，P2 组件按 `—` 诚实降级）；分界线：文件不存在=正常，存在但解析失败=故障

### 验收标准
- 管道喂坏 JSON → stdout 出现 `[hud err]` 且退出码非零；手动终端跑可见 stderr 完整错误
- 失败后 doctor 显示 last render failure（含时间）；无失败时 `[ok]`
- transcript 缺失：无输出、退出码 0，正常渲染输出与现状一致
- 错误标记不吞正常渲染路径

## 任务 ⑭：成本正确性——同一份 USD 数据三处三种货币符号 + 三方模型价格失真（Q6）

### 问题现象
数据源 `total_cost_usd`（美元），但 compact 默认 `¥`、dashboard `$`、notify 硬编码 `¥`、alerts 硬编码 `¥`；`¥0.50` 被误读为 0.5 元人民币（实际 0.5 美元，偏差近 7 倍）。三方模型/网关场景下 Claude Code 按官方价估算，成本失真或恒为 0。

### 具体原因（证据）
- `src/widgets/cost_display.rs:17-18` — compact 默认符号 `¥`（可配置），数据为 USD
- `src/widgets/cost_display.rs:31` — dashboard 用 `$`
- `src/notify.rs:37` — `¥{:.2}` 硬编码，无配置路径
- `src/widgets/alerts.rs:50` — `¥{:.2}` 硬编码
- 价格 100% 透传 Claude Code statusLine JSON（session.rs），HUD 零计算能力

### 修复方案（拍板：符号统一 + [pricing] 精确匹配重算 + token 展示）
1. 四处统一 `currency_symbol` 配置，**默认 `$`**（与数据单位 USD 一致）；notify/alerts 读同一配置
2. `[pricing]` 配置段（全字段 `#[serde(default)]`，单位 USD/1M tokens），**模型名精确匹配**（用户拍板，不做前缀匹配）：
   ```toml
   [pricing]
   "claude-opus-4-7" = { input = 15.0, output = 75.0, cache_read = 1.5, cache_creation = 18.75 }
   "my-gateway-model" = { input = 0.2, output = 0.6 }
   ```
   - 命中 → 用 transcript 精确 token（`src/core/transcript.rs:271-276` 已有 usage 统计）重算成本，显示 **`≈` 标记**（`≈$0.42`，延续"≈n 注明估"诚实原则）
   - 未命中 → 透传 Claude Code 的 cost，零破坏
3. **token 用量统计展示**（用户拍板引入）：compact 状态栏增加 tokens in/out 展示（context_bar 已有总量，补 in/out 或独立 widget，实施时选成本最低者）
4. doctor 校验 `[pricing]` 单价格式（非负数值、结构合法）
5. DEPLOY.md 注明"货币符号仅作展示，数据单位始终为 USD"

### 验收标准
- 默认显示 `$`；改 `currency_symbol` 后 compact/dashboard/notify/alerts 四处生效
- `[pricing]` 命中模型：按 token 重算且带 `≈`；未命中：透传不变
- 坏单价（负数/非数值）→ doctor 报错并给出定位
- 存量 config.toml 无 `[pricing]` 时行为与现状完全一致
- token 用量展示在状态栏可见

## 任务 ⑮：compact 零宽度感知——窄终端状态栏必然溢出（Q5）

### 问题现象
compact 输出定宽渲染、无任何截断；窄终端（tmux 分屏、并排窗口、80 列）下溢出终端宽度，状态区被撑开/换行，布局崩坏。用户不会归因于终端窄，只会认为 HUD 是坏的。

### 具体原因（证据）
- `src/compact.rs:37-74` — 全程无宽度探测、无截断；元素定宽（bar_width 16-20），模型名/分支名/agent 名全量输出
- Claude Code 官方文档硬约束：statusLine 命令捕获 stdout，`tput cols` 与语言级宽度检测**全部失效**，只能读 `COLUMNS`/`LINES` 环境变量（v2.1.153+）
- 默认 2 行输出（`compact_lines=2`，obsidian-command 3 行），典型行宽 100-130 字符

### 修复方案（拍板：COLUMNS + 组级截断 + 字段截断，两者并施）
1. 读 `COLUMNS` 环境变量（parse 失败/缺失 → 兜底 80；下限 clamp 40），不引入 terminal_size（在 statusLine 环境必然失效）
2. **组级截断**：从行尾整组丢弃直至放得下（保留 separator 边界，宁可少显示一组不显示半个组）；测量必须先剥 ANSI 转义再数可见字符，引入 `unicode-width`（CJK 名称宽度正确；编译期依赖，不影响"运行时零依赖"宣称）
3. **字段级截断**：模型名/分支名/agent 名无界字段 24 字符 + `…`（共享 `truncate(s, n)` 辅助函数）——防单个超长字段吃掉整行

### 验收标准
- `COLUMNS=80` 下超宽输出：行尾组被丢弃、剩余组完整、无半个组
- 无 `COLUMNS` 时兜底 80；垃圾值（`COLUMNS=abc`）同样兜底
- ANSI 转义码不计入宽度（剥码后测量）；CJK agent 名宽度正确
- 正常宽终端（≥120 列）行为与现状一致

## 任务 ⑯：dashboard 交互只有两把钥匙——q 和 Esc（Q7）

### 问题现象
README 宣称"full-screen TUI dashboard for deep diagnostics"、CHANGELOG 宣称 3 种布局模式，但运行时只能退出；切换布局的唯一途径是改 config.toml 再重启。三种布局实际上只有被配置选中的那一种存在。

### 具体原因（证据）
- `src/dashboard.rs:88-98` — 按键只有 `q`/`Esc`；`'1'..='9'` 空分支（注释 "future"，上轮⑪已拍板删除）
- `src/dashboard.rs:137-170` — `build_grid_2x2`/`build_sidebar`/`build_single_panel` 三布局代码均存在
- `src/dashboard.rs:116-135` — 布局启动时读配置定死，无运行时切换路径；无帮助视图、无按键提示

### 修复方案（拍板：l 循环 + ? 帮助 + 布局选择持久化）
1. **`l` 键循环切换布局**（grid-2x2 → sidebar → focus），切换时底部提示当前布局名
2. **布局选择持久化**（用户拍板：仅内存不符合直觉）：切换写回 config.toml 的 `dashboard.default_layout`，下次启动沿用；复用 `write_atomic`（main.rs:177-184）+ TOML 往返——**注意取舍：往返会丢失 config.toml 中的注释**，任务内注明并在 doctor/文档中提示
3. **`?` 帮助视图**：显示全部按键 + 当前 mod 名
4. 删除 `'1'..='9'` 空分支（落实上轮⑪）

### 验收标准
- `l` 循环切换三布局且底部有提示；`?` 显示帮助
- 切换后 config.toml 的 `default_layout` 已更新；重启 dashboard 沿用最后选择
- 删除空分支后无死代码；`q`/`Esc` 退出不受影响

## 任务 ⑰：安装/卸载/全局配置 UX——占位符仓库、配置双向丢失、全局无提示（Q2+Q4+Q8）

### 问题现象
① README 宣称 one-line install，但占位符仓库 `user/claude-hud` 使按文档操作的用户 100% 失败且报网络错；② setup 静默覆盖用户已有 statusLine，卸载后原配置永久丢失（.bak 每次被覆盖、不提示不还原）；③ 配置全局生效但用户无任何提示，多窗口互相影响时无从归因。

### 具体原因（证据）
- `scripts/install.sh:5` / `scripts/install.ps1:5` — `REPO` 默认占位符 `user/claude-hud`（注释"发布前替换"）
- `README.md:19-22` — 直接给出安装命令，未标注未发布状态
- `src/core/cc_config.rs:13-17` — `root["statusLine"] = ...` 无条件覆盖；`:69-74` 测试断言替换语义
- `src/main.rs:218-223` — 备份写固定 `settings.json.bak`，每次 setup 覆盖旧备份；`:241-265` uninstall 删配置目录、不还原 .bak、不提示
- `src/core/config.rs` — 配置全局单份，无 per-project 概念

### 修复方案（拍板：占位符检测 + 时间戳备份 + 全局声明；per-project 推迟 v0.2）
1. **安装脚本占位符检测**：`$REPO` 命中 `user/claude-hud` 或 API 返回 404 → 明确报 "Claude HUD 尚未发布，请使用源码构建（cargo build --release）"，不报网络错误
2. **README 安装段加 not-yet-released 标注**（真实仓库创建后移除——用户后续自行创建仓库）
3. `cc_config` 新增 `has_status_line(&str) -> bool`；`setup_cc_settings` 仅当**已有 statusLine** 时创建**时间戳备份**（`settings.json.hud.bak-<epoch>`，SystemTime epoch 秒）并打印 `replacing existing statusLine (backup at ...)`；无已有 statusLine 时不产生备份文件
4. **uninstall 结尾提示**备份位置与还原方法（`your original settings backup (if any) is at ~/.claude/settings.json.hud.bak-* — copy it back over ~/.claude/settings.json to restore`）；**不删 .bak**；**不做交互式还原**（管道安装场景 stdin 非 TTY；.bak 是安装时快照，自动还原会覆盖用户安装期间的后续修改 = 另一种数据丢失）
5. **全局生效提示**：`mod use` / `theme import` / 布局切换等写配置命令输出追加 `(applies to all windows)`
6. **DEPLOY.md 配置章节开头声明**："配置全局生效于所有会话窗口；数据层面（session/git）各窗口独立"

### 验收标准
- 占位符仓库下安装脚本报"尚未发布"而非网络 404；真实仓库名注入（HUD_REPO）后走正常路径
- 已有 statusLine 时 setup 打印替换提示 + 时间戳备份；无 statusLine 时无备份文件
- 连续两次 setup（已有 statusLine）产生两个不同时间戳备份
- uninstall 输出备份位置与还原方法；.bak 不被删除
- 写配置命令输出含全局声明；DEPLOY.md 含全局性声明

## 任务 ⑱：升级通路不存在——用户永远停留在 v0.1.0（Q9）

### 问题现象
无 upgrade 能力入口；README 无升级说明；安装脚本"already installed - nothing to do"无法区分"已最新"和"没检测"；用户不知道重跑安装命令即升级、也不知道升级不丢配置。

### 具体原因（证据）
- `scripts/install.sh:31-35` / `scripts/install.ps1:29-31` — 幂等升级逻辑（version.txt 对比）存在但用户不可见
- README 无 Upgrade 节；18 个子命令无 update/upgrade

### 修复方案（拍板：分层 4 件 + 2 明确不做；版本口径 = git tag `v` + Cargo version）
1. **`claude-hud update check` 子命令**（v0.1 做）：
   - 查 `releases/latest` 的 tag，剥 `v` 前缀与 `CARGO_PKG_VERSION` 比较；仓库名与安装脚本同源
   - 三态输出：`✓ up to date (vX.Y.Z)` / `↗ update available: vX.Y.Z — re-run the install script to upgrade` / 未发布（占位符仓库）→ `not published yet` / 网络失败 → `update check unavailable`
   - 网络失败静默降级，**不缓存假结果**；中国用户 GitHub 不可达场景不报错
2. **doctor 集成**：增加 update 检查项，有新版提示 `update available: vX.Y.Z`；网络失败/未发布算 `[..]` 不算 failure
3. **dashboard footer 提示**（最低优先级）：启动时后台线程查一次，有新版显示 `↗ vX.Y.Z available`；不弹窗、不自动下载
4. **安装脚本输出诚实化**：version 一致 → `claude-hud vX.Y.Z is up to date`；不一致 → `upgrading vX.Y.Z → vX.Y.Z`
5. **README Upgrade 一节**："重新运行安装命令即升级（自动检测新版本），config.toml 与数据保留在 ~/.claude/plugins/claude-hud/"
6. **DEPLOY.md 发布章节写明版本约定**：发布流程 = bump Cargo.toml → tag `vX.Y.Z` → CI 出 artifacts；安装脚本、update check、CI 三方同一口径
7. **明确不做（v0.2 候选）**：`upgrade` 自替换（Windows exe 文件占用，需 .cmd 中转/延迟替换）、自动后台下载、render 进程内自动检查（5 秒热路径严禁网络）

### 验收标准
- `update check` 三态输出正确；占位符仓库阶段输出 not published yet 而非网络错
- doctor 显示 update 检查项且网络失败不算 failure
- dashboard footer 无新版本时零打扰
- 安装脚本两态输出（up to date / upgrading）
- README/DEPLOY 更新到位；版本口径三方一致

# 第三轮拷打（未来迭代方向）— v0.2 方向决策 ⑲-㉑ + 延期队列

> 2026-07-31 第三轮 `/grill-me`：6 问（Q1-Q6）拷打未来迭代方向，全部拍板。
> v0.2 主线 = **成本哨兵**（用户拍板：每天第一眼看的是实时成本）；诊断深度（Phase 2 剩余蓝图）改"验证后放行"。

## 任务 ⑲：实时成本状态栏（v0.2 第一优先）

### 方向背景（拍板：Q1 定位 / Q2 形态 / Q3 三方约束）
用户明确：状态栏实时展示**当前会话已用 token + 成本**。数据零新管线——Claude Code 每 5 秒 statusLine JSON 自带 session 累计值（`total_input_tokens` / `total_output_tokens` / `total_cost_usd` / `context_window_size`），纯展示层。

### 修复方案（拍板）
1. **合并单组形态**：`≈$0.42 · 12.3k/45.6k tok`（成本 + in/out tokens 一组；`k` 缩写省宽度，配合⑮宽度感知）
2. **双轨计算**：
   - `[pricing]` 命中 → in/out 重算 + `≈`（实时路径无 cache 数据，**必然低估**——cache_read 是成本大头；`≈` 即诚实标注，不做"含不含 cache"细分标记）
   - 未命中 → 透传 Claude Code `total_cost_usd`（官方模型准、含 cache）
   - 精确重算（含 cache）留给 dashboard/复盘场景（transcript 有完整 usage）
3. **三方模型三约束**（用户拍板吸收进⑭/⑲规格）：
   - **未命中可见**：带 `≈` = "非官方计费，估算值"（命中重算与未命中透传统一语义）；serve/dashboard 完整数据视图标注"当前模型未配置单价（model.id: xxx）"；DEPLOY.md 写明模型 ID 以 stdin 的 `model.id` 为准 + 如何查看
   - **混合模型会话**：stdin 累计 token × 当前模型单价 = 脏值；`≈` 标注 + DEPLOY.md 注明"混合模型会话重算不准确，建议固定模型或依赖透传"；**不做模型级归因管线**（v0.2 不背）
   - **网关无 usage/cost** → `—` 诚实降级（不显示 `$0.00` 假精确）；doctor 信息项：`[pricing] 已配置 N 个模型单价`（不能假装校验模型存在性）
4. token 用量展示（用户第二轮拍板"引入 token 统计"）随本任务落地

### 验收标准
- 状态栏显示 `≈$X.XX · Xk/Xk tok`；未配置 [pricing] 时行为 = 现状透传
- [pricing] 命中/未命中/未配置三态输出正确；网关缺 usage → `—`
- DEPLOY.md 含 model.id 指引 + 混合模型警告
- 宽度超限时组级截断（⑮）正常生效

## 任务 ⑳：预算告警 [budget]（v0.2 第二优先）

### 方向背景（拍板：Q4）
用户拍板："成本哨兵"必须在**状态栏路径**也响——原 `check_alerts` 只在 dashboard 进程跑（dashboard.rs:78），用户不开 dashboard 就收不到任何预警（context 95% / cost 阈值 / rate limit）。预警下沉 render 进程。

### 修复方案（拍板）
1. **预警检查下沉 render 进程**：每 5 秒的 render 进程内做阈值比较（纯内存，零成本），触发时发通知；**跨进程去重靠 state.json**（任务①设施：last-fired 时间戳 + 已触发最高档位，读-比-写三行代码，无需常驻进程）
2. **任务⑫规格升级**：冷却从"dashboard 进程内 HashMap"升级为"state.json 跨进程去重"（dashboard `check_alerts` 保留，同一套阈值/冷却逻辑共享函数；serve 未来接入）
3. **`[budget]` 配置段**（与 `[alerts].cost_threshold_usd` **并存**——前者"超 X 提醒我"单档开关 v0.1 已拍板，后者"预算是 X 接近时提醒"渐进版；两者都触发时先到者生效，不做特殊合并）：
   ```toml
   [budget]
   cap_usd = 5.0               # 会话成本上限（0 = 关闭预算）
   warn_pcts = [50, 80, 100]   # 达到这些百分比时通知，每档一次
   ```
4. **档位去重**：只记录已触发的最高档位（单调递进），比每档独立冷却简单
5. **诚实性**：预算基于 `≈` 估算值触发（三方模型精度同样打折），文档注明"预算基于估算值"

### 验收标准
- 不开 dashboard，状态栏进程在 cost 跨档时发通知；10 分钟冷却跨进程生效（两个 render 进程不会重复发）
- `[budget]` 三档渐进触发，每档一次；`cap_usd = 0` 关闭
- `[alerts].cost_threshold_usd` 与 `[budget]` 并存互不干扰
- state.json 冷却记录可被 doctor 读取检查

## 任务 ㉑：成本周报（v0.2 第三优先）

### 方向背景（拍板：Q5）
防"第二份没人看的数据"（任务⑨前车之鉴）——**不做新架构、不做推送**，增强两个已有出口。

### 修复方案（拍板）
1. `claude-hud history --weekly`：周聚合输出——**固定五指标**：本周成本（≈）、会话数、token 总量、最长会话时长、最高成本单会话（全部来自现有 history 表字段，零新数据采集）
2. serve 页面周趋势曲线（history.rs:162 日成本聚合已有）
3. **不做的**：agent 维度周报（transcript 解析成本高、无需求证据）、趋势预测（伪精确）、任何推送通道
4. 验收只做功能性（命令输出正确、曲线渲染）；使用价值标注"待用户反馈验证"（COMPLETE.md 路线图同步标注）

### 验收标准
- `history --weekly` 五指标输出正确（空库显示 `—` 而非 0）
- serve 周曲线渲染正常；无数据时段不渲染空曲线
- 无新数据采集、无推送

## 延期队列（决策表，非任务）

| 项 | 状态 | 放行条件（全部满足才启动） |
|----|------|--------------------------|
| 多会话聚合监控 | 延期 | Q8 的 per-project 决策反转 + 用户主动提出需求；**per-project 配置没做之前绝不做** |
| 插件市场（install user/repo） | 延期 | 真实仓库 + 首个 release + 有人真正分享 mod（社区证据）+ 安装脚本占位符已替换；**v1.0 的梦** |
| Homebrew tap / 其他分发 | 延期 | release 稳定 2 个版本 + 用户请求 |
| 性能优化 | ✅ 已完成（v0.3 批次，2026-08-04） | token_timeline 上限 + 结账去重表 + serve 历史缓存 + 预算占比 + warning 清零；黑盒 141 例 |
| 动画接入（v0.4 候选） | ✅ 已完成（v0.4 批次，2026-08-04） | 时间相位纯函数重建 animation.rs（now_phase/breathe/gradient/ease_out/scanline_offset，CLAUDE_HUD_PHASE env 黑盒确定性）+ 6 效果接线（渐变进度条/呼吸/缓动计数器/CRT 扫描线/伪 3D 面板/盲文频谱）+ tabbed 布局补全（四态循环 + ←/→ 切换）；黑盒 147 例 + 单元测试 136 个 |
| 国际化 | **低优先级计划内**（用户拍板） | 同上；currency_symbol 配置（⑭）已就绪为基础 |
| 竞品功能吸收 | 观察不吸收 | 用户反馈缺什么再抄；⑮组级截断思路已同 Barista |

**路线图改写原则**（拍板）：COMPLETE.md 第 21 章每个 ⬜ 项附放行条件；无条件的项删除；路线图 = 决策表而非愿望列表。

# 实施顺序与验证流程

```
批次 A（① ②⑧ ⑫ ⑬）──┬── 批次 B（③ ④ ⑭）───┬── 批次 C（⑤⑥⑦⑨⑩⑪⑮⑯⑰⑱）
   cargo test          │     cargo test       │     cargo test
   黑盒套件             │     黑盒套件         │     黑盒套件
   更新 COMPLETE.md     │     更新 COMPLETE.md │     更新 COMPLETE.md
```

- 每批独立可交付、可回滚（commit 粒度：每任务一个 commit，按 `fix: ...` / `feat: ...` 前缀）
- 黑盒套件：`python scripts/test_hud.py`（`--case` 可单跑）
- 每批完成后同步更新 `COMPLETE.md` 第 20 章的实现状态表（✅/🟡/⬜）
- 全部完成后跑一次全量验证：`cargo test` + 全量黑盒 + `claude-hud doctor` + 三个平台构建（CI）

---

*任务文档生成于 2026-07-31，源自三轮 grill-me 拷打会话：第一轮 11 项设计缺陷（任务①-⑪）+ 第二轮 9 项功能/使用角度问题（任务⑫-⑱，Q2/Q4/Q8 合并为⑰）+ 第三轮 6 项未来方向决策（v0.2 任务⑲⑳㉑ + 延期队列），共 24 项决策全部经用户拍板。*
