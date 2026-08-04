# v0.4 视觉批次设计（动画时间相位重建 + tabbed 布局）

> 放行依据：TASKS.md 延期队列"动画接入（v0.4 候选）"放行条件 = v0.3 完成（2026-08-04 已满足）。
> 范围拍板（2026-08-04 用户确认）：**动画 6 效果 + tabbed 布局补全合并为本批次**；缓动计数器**仅仪表盘**（紧凑 5s 进程重生每进程一帧，无法滚动）。

---

## V1: animation.rs 时间相位纯函数重建

### 现状（证据）

- v0.3 W1 已删 9 个 frame 制原语，仅留 `AnimationState { frame }` + `new()` + `tick()` + `neon_breathing`（`agent_detail.rs:113` 唯一接线方）。
- **frame 制与 5s 进程重生不兼容**（每进程 frame 从 0 起跳，动画冻结）；v0.3 拍板"动画改时间相位驱动"。
- **先例**：`alerts.rs:110` 的 `time_phase(period)` 已是墙钟相位实现（8s 周期 danger/warning 硬切换）。

### 方案

```rust
/// 墙钟相位 [0,1)：进程内墙钟毫秒 % (period*1000)。CLAUDE_HUD_PHASE 环境变量
/// 覆盖（黑盒确定性，COLUMNS 先例）——合法 f64 ∈ [0,1) 直接返回，否则回退墙钟。
pub fn now_phase(period_secs: f64) -> f64

/// 亮度呼吸：hex 与 hex*0.45 之间正弦脉动（k = 0.5+0.5·sin(2π·phase)）。
pub fn breathe(hex: &str, phase: f64) -> (u8, u8, u8)

/// 线性 RGB 插值，t 钳制 [0,1]。
pub fn gradient(hex_a: &str, hex_b: &str, t: f64) -> (u8, u8, u8)

/// ease-out：1 - (1-t)²。
pub fn ease_out(t: f64) -> f64
```

- **删除**：`AnimationState` 结构体、`tick()`、`neon_breathing`（v0.3 保留物，本批全部退役）。
- 接线替换：
  - `alerts.rs:39`：`time_phase(8) < 4` 硬切换 → `breathe(&theme.danger, now_phase(4.0))` 明暗呼吸（更平滑）；删除 `time_phase` 函数与其测试。
  - `agent_detail.rs`：dashboard `neon_breathing` → `breathe`；compact 卡顿指示 `◐` 用 `breathe(&theme.danger, now_phase(4.0))` 呼吸色（活跃时保持 success 静态，避免全组闪烁）。

### 验收

- [x] 单测：`now_phase` env 覆盖（0.0 / 0.5 精确返回；非法值回退且结果 ∈ [0,1)）；`breathe` 相位 0/0.25/0.75 边界（亮度 0.725/1.0/0.45；相位 0 与 0.5 同为 0.725——正弦对称，勿断言其不同）；`gradient` 端点精确（t=0 → a 色，t=1 → b 色）；`ease_out` 端点与单调（6 个单测通过）
- [x] 黑盒：`CLAUDE_HUD_PHASE=0` 与 `=0.25` 两次 render 输出 truecolor 色码不同（呼吸可断言）（P5-13a/P5-13b 通过）
- [x] `cargo test` 全绿；`cargo check` 0 warnings

---

## V2: context_bar 渐变进度条

### 现状（证据）

- `context_bar.rs:25`：3 档变色（danger/warning/success）——`warn_threshold`/`critical_threshold` 阈值切换。
- **`gradient = "true"` 配置键在 DEPLOY.md 已声明但从未接线**（get_bool("gradient") 无任何调用）——v0.4 补上。

### 方案

1. `render_compact`：`gradient = get_bool("gradient", true)`（拍板"真渐变替 3 档变色" → 默认开）；`true` 时 filled 段**逐 cell 渐变**：`t = i / (bar_width-1)`，`gradient(success, danger, t)`；`false` 保留 3 档原逻辑。
2. empty 段保持 `theme.border` 不变；filled=0 时无渐变（空条）。
3. ANSI 输出：每 cell 独立 `ansi::ansi_fg`（`38;2;r;g;b` truecolor）。

### 验收

- [x] 单测：gradient=true 时输出含 ≥2 个不同 truecolor fg 色码且填满 cell；gradient=false 时整段单色（3 个单测通过；gradient-off 用例用 pct 97 → danger，plan 原 90 落在 warn 区间为缺陷，已修正）
- [x] 黑盒：`[widgets.context_bar] gradient="true"` → 输出含 ≥2 色码（P5-12a）；`gradient="false"` → 单色（P5-12b）
- [x] 既有宽度断言（D2-05/D2-07）回归通过（色码数量变化不影响组串整体宽度断言口径——按实际输出核对）

---

## V3: 缓动计数器（仪表盘 cost_display）

### 现状（证据）

- `cost_display.rs:45` `render_dashboard`：Text 直显 `total_cost_usd` 4 位小数，无动画。
- **紧凑不可行**（拍板确认）：5s 进程重生每进程渲染一帧，从 0 滚到真实值 = 每 5s 闪烁假象。

### 方案（仪表盘进程内状态——时间相位架构的唯一进程内例外，不进黑盒）

1. `CostDisplay` unit struct → struct + `Mutex<EasedValue>`：
   ```rust
   struct EasedValue { target: f64, start: f64, start_at: std::time::Instant }
   ```
   帧间 `target` 变化 → 重置锚点（start=当前显示值, target=新值）；未变化 → 继续缓动。
2. 显示值 `display = start + (target - start) * ease_out(elapsed / 0.8s)`（0.8s 缓动时长，clamp [0,1]）。
3. `render_dashboard` 数字用 display 值；duration/行数等静态字段不受影响。
4. compact 路径不动。

### 验收

- [x] 单测：`EasedValue` 推进——target 不变时 0.4s 后显示值在中点附近（t=0.5 → ease 0.75）；target 变化重置锚点（3 个单测通过；100→50 中点 = 62.5 而非 plan 的 75——ease_out(0.5)=0.75 乘 50，plan 缺陷已修正）
- [x] `cargo test` 全绿；黑盒无新增（dashboard TUI 不进黑盒），既有用例回归

---

## V4: CRT 扫描线（dashboard 背景）

### 方案

1. `draw_dashboard` 在 widget 渲染**之前**对 `main_area` 渲染背景层（widget 覆盖其上）：
   - 每 4 行一行全宽空格 `theme.border` fg（dim 效果）；
   - 扫描带 1 行（位置 = `scanline_offset(now_phase(8.0), height)`）`theme.accent` fg。
2. 单 `Text` widget 一次渲染（每行 = 空格串），不逐行 render widget。
3. 配置门：`[dashboard] scanlines`（默认 true）；false 时不渲染背景层。
4. 三个布局（grid/sidebar/focus/tabbed）统一生效。

### 验收

- [x] 单测：`scanline_offset` 边界（phase 0 → 0；phase → 1 前 → height-1；height=0 → 0）（并入 animation.rs 6 个单测）
- [x] 手动：dashboard 三个布局可见扫描线且不遮挡文字（widget 在上层）
- [x] 黑盒无新增（TUI）；既有用例回归

---

## V5: 伪 3D 面板（focus / tabbed 内容面板）

### 方案

1. focus 与 tabbed 布局的内容面板渲染顺序改为：先画 `Block::bordered()` 3D 边框，再 `area.inner(1,1)` 内渲染 widget。
2. 3D bevel：top/left 边框 `theme.accent`（光源），bottom/right `theme.border`（阴影）——ratatui 0.29 `border_style(Style, Borders)` 按侧支持（实现时验证 API；若不可用降级为单色 border + accent 标题，验收降级）。
3. grid/sidebar 布局不动（现状）。

### 验收

- [x] 手动：focus 布局边框呈 bevel 立体感；widget 内容完整显示在 inner 区
- [x] 黑盒无新增；既有用例回归
  - 实现注记：ratatui 0.29 `Block::border_style` 仅单侧样式（无 per-side border）→ 方案 2 降级路径生效：accent 全边框 + 右下偏移 1 格 border 色 shadow block（伪 3D 达成）

---

## V6: 盲文频谱 token_rate widget

### 现状（证据）

- `TranscriptSummary.token_timeline: Vec<TokenSnapshot>`（60s 桶，W3 已封顶 360 桶）——现仅 compaction_prediction 使用。
- dashboard 面板分配 = `config.compact_layout` 顺序（`dashboard.rs:244-249`）——新 widget 需进 compact_layout 才有面板位。

### 方案

1. 新文件 `src/widgets/token_rate.rs`（agent_detail 的 Mutex 缓存模式）：
   - `render_compact`：速率 = 尾桶 `total_tokens / 60s` → `tok 3.2k/min`（k 缩写复用 cost_display::format_tokens 口径或本地实现）；timeline 空 → `—`。
   - `render_dashboard`：最近 24 桶 → 8 级块条 `▁▂▃▄▅▆▇█`（0 级用空格），max 桶归一化；标题 "Token Rate"。
2. 注册：`main.rs` registry + 默认 `compact_layout` 列表尾部加 `token_rate`。
3. compact 宽度：组串短，`fit_line` 组级截断兜底。

### 验收

- [x] 单测：速率计算（桶 3100 tok → 3.1k/min；空 timeline → None）；8 级归一化边界（0 → 0 级；max → 8 级）（4 个单测通过）
- [x] 黑盒：带 transcript fixture 的用例输出含 `tok` 段（P5-14，fixture 增量语义 delta 3100 → "3.1k/min"）；无数据用例 → `—`（P5-15）
- [x] 既有 compact 宽度用例（COLUMNS 小宽度丢弃新组）回归通过
  - 口径注记：速率 = 尾桶 total_tokens − 前桶（增量语义）——plan 原 "尾桶/60" 为累计值误解，已修正（token_timeline total_tokens 为 epoch-aligned 累计桶）

---

## V7: tabbed 布局补全

### 现状（证据）

- `dashboard.rs:239`：`"tabbed"` 是 focus 别名（`build_single_panel`）；`next_layout` 循环不含 tabbed（`:382-390` 测试锁定三态）。
- noir-tabbed mod 声明 "Tabbed" dashboard 布局——当前无实际 tab 交互。

### 方案

1. `next_layout` 四态循环：grid-2x2 → sidebar → focus → **tabbed** → grid-2x2（测试同步改）。
2. tabbed 渲染：顶部 tab 条 1 行（`compact_layout` 各 widget 的 `display_name()`，激活项 accent fg）+ 下方内容面板（3D 边框，见 V5）。
3. 键：`←`/`→` 切换 tab（循环）；`l` 仍为布局循环（离开 tabbed 正常）；tab 状态进程内（不持久化，与 show_help 同级）。
4. 帮助面板第 4 行更新：`←/→  switch tab (tabbed)`。
5. `persist_layout` 不改（default_layout 可存 tabbed）。

### 验收

- [x] 单测：`next_layout` 四态循环 + 未知值回退；tab 切换纯函数（wrap 边界）（2 个单测通过）
- [x] 手动：tabbed 布局 tab 条渲染、←/→ 循环切换、内容面板切换正确
- [x] 黑盒无新增（TUI）；既有用例回归

---

## 批次总验收

- [x] `cargo check` **0 warnings**；`cargo test` 全绿（120 → **136**：+6 animation −1 alerts time_phase +3 context_bar +3 cost_display +4 token_rate +2 dashboard tab）
- [x] 黑盒套件全绿（141 → **147**：P5-12a/12b 渐变、P5-13a/13b 呼吸 env、P5-14/15 token_rate）
- [x] `claude-hud doctor` 正常（T11 复核）
- [x] 工作区无新增死代码；文档同步（CHANGELOG 0.6.0 段、COMPLETE.md §9/§20/§21、DEPLOY.md 新增配置键说明、TASKS.md 动画行勾选）
- [x] 手动 dashboard 冒烟（三个布局 + tabbed + 扫描线 + 3D 边框）

## 明确不做

- 动画效果砍除清单维持 v0.3 拍板：火花拖尾/波浪/液体/故障/理发店/跑马灯/RGB 色环/电影揭示/热力图（9 种）不重建
- 国际化、主题市场、多会话监控、Homebrew tap（延期队列）
- 模型级 token 归因、趋势预测/推送（⑲⑳㉑ 已拍板排除）
- compact 模式缓动计数（拍板确认：进程重生单帧无法滚动）
