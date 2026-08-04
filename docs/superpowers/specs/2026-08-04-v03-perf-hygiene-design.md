# v0.3 性能与卫生批次设计（W1-W5）

> 放行依据：TASKS.md 延期队列"性能优化（低优先级计划内）"的放行条件 = v0.1（18 项）+ v0.2（⑲⑳㉑）全部完成，**2026-08-04 已满足**。
> 本批次 5 项：警告清零（W1）、serve 缓存（W2）、timeline 上限（W3）、结账去重（W4）、预算进度显示（W5）。

---

## W1: 17 个构建 warning 清零

### 现状（证据）

`cargo check` 产出 17 条 warning，全部为死代码/未用项（无新告警源，均为历史遗留）：

| 类别 | 数量 | 位置（计划时以单次完整输出定位） |
|------|------|------|
| unused import | 4 | context_bar.rs:3（TokenTotal）、state.rs:5（PathBuf）、pricing.rs:7（Color）、animation.rs:7（ansi） |
| unused variable `theme` | 2 | script_widget.rs:10、model_display.rs:30 |
| never-read 字段/函数/结构体 | 11 | animation.rs（Spark、hsl_to_rgb、9 个未接线原语 + `enabled` 字段）、theme.rs（interpolate_hex）、widget.rs（trait 默认方法 dashboard_size/needs_tick）、transcript.rs（parse-only 结构体未读字段）、session.rs（tuple 字段 0）、history.rs（SessionRecord 未读字段） |

关键事实：

- **动画仅 1 处接线**：`agent_detail.rs:113` 用 `neon_breathing`；其余 9 个原语（spectrum_cycle / eased_value / barber_offset / spark_frame / glitch_offset / marquee_offset / wave_offset / liquid_height / scanline_alpha）零调用。
- 动画原语是 **frame 制**（`frame % N`），与 5s 进程重生架构不兼容——任务②⑧拍板"动画改时间相位驱动"，现有原语属于**过时架构的死代码**，未来动画批次（🟡 蓝图项）需按时间相位重写。
- `AnimationState.enabled` 从未被读（agent_detail 构造时传 true，无人查询）。
- transcript.rs / session.rs 未读字段均为 parse-only（非 serde 序列化，删除不影响 state.json 契约）。
- history.rs SessionRecord 的 `lines_added/lines_removed/mod_used`：INSERT 写入 SQLite 但**任何查询都不读回**（`history` 三块输出无这些字段）——删除 Rust 字段 + INSERT 列值即可，SQLite 列保留（0 默认值），**无需迁移**。

### 方案

1. **4 处 unused import**：删除。
2. **2 处 unused variable `theme`**：改 `_theme`。
3. **animation.rs 收缩**：
   - 保留：`AnimationState { frame }` + `new()` + `tick()` + `neon_breathing`（唯一接线方）。
   - 删除：`enabled` 字段（`new(true)` → `new()`）、spectrum_cycle、eased_value、barber_offset、spark_frame、glitch_offset、marquee_offset、wave_offset、liquid_height、scanline_alpha、`Spark` 结构体、`hsl_to_rgb`。
4. **theme.rs `interpolate_hex`**：无调用者，删除。
5. **widget.rs trait 默认方法 `dashboard_size`/`needs_tick`**：无任何 impl 覆写/调用，删除（YAGNI；未来布局需要时从 git 历史恢复）。
6. **transcript.rs / session.rs 未读字段**：按单次完整 warning 输出逐一定位，确认 parse-only 后删除。
7. **history.rs SessionRecord**：删除未读字段，`record_session` 的 INSERT 同步调整（列保留，值落 0）。
8. **⚠️ 删除前检查 `cfg(test)` 引用**：`cargo check` 不含测试 cfg，被测试引用的项（如测试构造 Spark）需一并处理（删测试或保留项）。

### 验收

- [ ] `cargo check` 0 warnings
- [ ] `cargo test` 全绿（删除波及测试时同步处置）
- [ ] 黑盒套件全绿（确认无行为回归）
- [ ] 黑盒渲染用例输出与改动前一致（git diff 之外的行为对比）

### 拍板（2026-08-04 用户确认）

**选 A 删除**：9 个未接线动画原语（含 `Spark`/`hsl_to_rgb`/`interpolate_hex`）本批次删除。动画蓝图**不废弃**——未来批次（v0.4 候选）按时间相位纯函数重建，见 §未来规划。

---

## W2: serve 每请求重开 SQLite（30s TTL 缓存）

### 现状（证据）

- `serve.rs:94-107` `weekly_json()` 与 `:110+` `trend_json()` **每个 HTTP 请求**都 `HistoryStore::open()` + 全量聚合查询。
- 前端 2s 轮询 `/api/data` → 每 2s 各一次 SQLite open + 查询，长跑 serve 持续空转。
- 数据本身 2s 级变化，但 weekly/trend 是分钟级统计——缓存不损失新鲜度。

### 方案

1. serve.rs 增加模块级缓存：
   ```rust
   static HISTORY_CACHE: Mutex<Option<(Instant, String, String)>> = Mutex::new(None);
   // (fetched_at, weekly_json, trend_json)
   ```
2. 抽纯函数 `fn ttl_fresh(fetched_at: Instant, now: Instant, ttl: Duration) -> bool`（单测边界）。
3. `weekly_json()` / `trend_json()` 改为走缓存：未过期直接返回；过期/未填充 → 加锁重算两份并更新。
4. TTL = 30s；`Mutex` 短临界区（仅命中判定 + 回填），tiny_http 单线程循环下无竞态压力。

### 验收

- [ ] 单测：ttl_fresh 30s 边界（29s fresh / 30s expired）
- [ ] 黑盒 D6-02（serve 字段存在性）回归通过——**输出不变，纯内部优化**
- [ ] `cargo test` 全绿

---

## W3: token_timeline 无界增长（上限 360 桶 = 6h）

### 现状（证据）

- `transcript.rs:448-451`：60s epoch 桶只 push 不裁剪——超长会话 Vec 持续累积，state.json 体积同步膨胀。
- `compaction_prediction`（:513-527）只读 `[0]` 与 `[len-1]`——6h 窗口内的速率估算足够。

### 方案

1. 常量 `MAX_TIMELINE_BUCKETS: usize = 360`。
2. `fn cap_timeline(&mut self)`：`if len > MAX { drain(0..len - MAX) }`；在 **push 后**（:451 处）与 **`to_state()` 序列化前**各调用一次（恢复旧状态文件时立即封顶）。
3. 封顶后 `[0]` 即 6h 前桶，`compaction_prediction` 语义不变（窗口 6h）。

### 验收

- [ ] 单测：push 400 桶后 `len == 360` 且首桶为第 40 个桶的 ts；to_state 序列化前同样封顶
- [ ] 单测：封顶后 compaction_prediction 仍正常（窗口语义不破坏）
- [ ] `cargo test` 全绿；黑盒全绿

---

## W4: 会话切换结账去重（transcript path 抖动 double-billing）

### 现状（证据）

- `compact.rs:143-159` 结账条件 `should_checkout`（:213-217）= `prev_ts != 0 && prev 非空 && prev != cur`。
- **风险**：transcript_path 抖动（A→B→A→B…）时，A 在首次切换被结账，抖动回来再切走又结账一次 → 同一会话在 history 中重复记账，周报成本虚高。

### 方案（实施修正 2026-08-04：单槽 → path→ts 表）

> **拍板修正**：原方案的 flat 单槽（`last_checkout_path`/`last_checkout_ts`）在实现后推演发现**相位错位**——判定发生在"前次快照 path"（prev）上，而单槽只记最后一次结账，严格交替振荡下 prev 与 last 永远不同 path，去重永不触发（A→B→A→B 仍结账 3 次）。改为 **path→ts 映射表**，去重判定直接在"prev path 是否在冷却期内已结账"上，与振荡相位无关：

1. `StateFile` 增表字段（serde(default)，旧 state.json 无损兼容）：
   ```rust
   /// path → 最近结账时刻；同 path 冷却期内最多结账一次
   #[serde(default)] pub checkout_billed: HashMap<String, u64>,
   ```
2. `should_checkout` 签名（纯函数，可单测）：
   ```rust
   pub fn should_checkout(
       prev_ts: u64, prev_path: Option<&str>, cur_path: Option<&str>,
       billed: &HashMap<String, u64>, now: u64, cooldown_secs: u64,
   ) -> bool
   ```
   规则：原四态不变，**追加**——`!(billed.get(prev_path) 存在 && now - ts < cooldown_secs)`。
   语义：**同一 path 在冷却期内最多结账一次**。
3. 结账执行后 `state.checkout_billed.insert(prev_path, now)`；随后 `retain(now - ts < cooldown_secs)` 清理过期记录（表有界）。
4. cooldown 复用 `config.alerts.cooldown_minutes`（默认 10 分钟，与预算告警同一旋钮）。

行为推演（A→B→A→B，冷却 10 分钟）：

| 切换 | prev→cur | 判定 | 记录 |
|------|----------|------|------|
| 1 | A→B | A 未结账过 → 结账 A | billed={A:t1} |
| 2 | B→A | B 未结账过 → 结账 B | billed={A:t1, B:t2} |
| 3 | A→B | A 已在 t1 结账，冷却内 → **跳过** | billed 不变 |
| 4 | B→A | B 已在 t2 结账，冷却内 → **跳过** | billed 不变 |

结果：A、B 各记账一次，振荡不再 double-billing。10 分钟后同 path 再次出现 = 新会话，正常结账。

### 验收

- [x] 单测：原四态 + 振荡跳过（A 冷却内二结账被挡）+ 冷却过期放行 + 空表不挡 + 振荡两 path 各记一次
- [x] 黑盒 P5-09：4 次 render A→B→A→B → `history` 恰好 2 条（Sessions: 2，无 #3）
- [x] 现有黑盒 P4-01 / P5-05（双 render 结账）回归通过（用例补 `remove_state=True`：checkout_billed 跨进程持久，残留会挡结账）
- [x] `cargo test` 全绿

---

## W5: 状态栏预算进度显示（[budget] 命中时）

### 现状（证据）

- ⑳ 已完成：`check_budget` 档位单调 + 跨进程冷却 + 通知 + doctor 读取 `budget_tier`。
- **缺口**：状态栏本身不显示预算使用情况——用户配置 `[budget]` 后只能等通知，无法日常感知成本/预算水位。
- 注入点：`compact.rs:270` `inject_cost_realtime(data, config, &mut widget_config)` 是现成的单一注入位。

### 方案

1. `pricing.rs` `inject_cost_realtime` 末尾追加注入（复用 config，签名不变）：
   ```rust
   widget_config.set_f64("budget_cap_usd", config.budget.cap_usd);
   ```
   （cap_usd = 0 表示关闭，由 widget 侧判断。）
2. `cost_display.rs` `render_compact`：`budget_cap_usd > 0 && cost > 0` 时组尾追加 ` · {pct:.0}%`（`pct = cost / cap * 100`，不钳制，超 100 如实显示）；零数据 `—` 降级分支在追加之前 return，不受影响。
3. 样例：`[pricing]` 命中 + `cap_usd = 5.0` + 实时成本 $3.1 → `≈$3.10 · 12.3k/45.6k tok · 62%`。

### 验收

- [ ] 单测：cap>0 && cost>0 → 含 `· 62%`；cap=0 → 无 `%` 后缀；零数据 → `—` 不变
- [ ] 黑盒：config 带 `[budget]` + pricing 命中 → stdout 含 `· 62%`（精确组串，避开 context_bar 的 `50%` 干扰）；默认 config（cap=0）→ 不含 `· N%`
- [ ] D2-05/D2-07（COLUMNS=200 宽度回归）通过——组变长不影响已有宽度断言
- [ ] `cargo test` 全绿；黑盒全绿

---

## 批次总验收

- [ ] `cargo check` **0 warnings**
- [ ] `cargo test` 全绿（112 → 约 123）
- [ ] 黑盒套件全绿（138 → 140）
- [ ] `claude-hud doctor` 输出正常（无回归）
- [ ] 工作区无新增死代码；文档同步（CHANGELOG [Unreleased]、COMPLETE.md §20/§21、DEPLOY.md 预算显示样例）

## 未来规划：动画接入批次（v0.4 候选，W1 删除后的承接）

拍板（2026-08-04）：动画功能未来必须实现；W1 删除仅移除**过时 frame 制脚手架**，蓝图保留。

**架构**：`core/animation.rs` 重建为**时间相位纯函数**——无进程状态，同一时刻任何进程渲染结果一致（紧凑 5s 进程呼吸可见、黑盒可按给定时间戳断言）：

```rust
pub fn breathe(hex: &str, phase: f64) -> (u8, u8, u8)   // phase = 墙钟秒内相位
pub fn gradient(hex_a: &str, hex_b: &str, t: f64) -> (u8, u8, u8)
```

**效果分档**（DESIGN.md 15 种蓝图 → 6 做 9 砍）：

| 档 | 效果 | 场景 |
|----|------|------|
| 做 | 渐变进度条（真渐变替 3 档变色） | 紧凑 context_bar |
| 做 | 呼吸指示（alerts/代理状态色） | 紧凑 + 仪表盘 |
| 做 | 缓动计数器（成本数字滚动） | 紧凑 cost_display |
| 做 | CRT 扫描线 | 仪表盘背景 |
| 做 | 伪 3D 面板（焦点面板边框） | 仪表盘（随布局补全批次） |
| 做 | 盲文频谱（token 速率） | 仪表盘 |
| 砍 | 火花拖尾/波浪/液体/故障/理发店/跑马灯/RGB 色环/电影揭示/热力图 | 跑马灯被 ⑮ 截断取代；其余纯装饰、宽度预算内无生存空间 |

**放行条件**：v0.3 完成即满足；可与"仪表盘布局补全（tabbed/focus）"合并为 v0.4 视觉批次。
**验收关键**：时间相位 → 黑盒可断言；紧凑模式跨 5s 快照呼吸交替可见。

## 明确不做

- 国际化、紧凑/仪表盘布局补全（各自独立批次；布局补全可与动画批次合并为 v0.4 视觉批次）
- 动画接入 → 非不做：见 §未来规划（v0.4 候选）
- 主题市场、多会话监控、Homebrew tap（延期队列，放行条件未满足）
- 模型级 token 归因、dashboard 接预算、趋势预测/推送（⑲⑳㉑ 已拍板排除）
