# Claude HUD — 完整设计与功能文档

> 本文档整合项目的设计蓝图与实际实现，覆盖：架构、数据来源、Widget 系统、主题/动画/Mod 系统、仪表盘、Web 面板、脚本扩展、CLI、配置、安装部署、测试与发布。
> 每个功能均标注实现状态：✅ 已实现 / 🟡 部分实现（含占位或退化）/ ⬜ 设计蓝图（未实现）。

---

## 1. 项目概述

**Claude HUD** 是 Claude Code 的双模终端可视化插件，用 Rust 编写，提供：

- **紧凑状态栏**（日常使用）：通过 Claude Code 状态行机制，将模型、上下文、代理、Skills/MCP、费用、Git 等信息渲染为 1-3 行 ANSI 文本。
- **全屏 TUI 仪表盘**（深度诊断）：ratatui 驱动的全屏面板，展示代理可观测性、Token 归因、时间线、会话统计。
- **Web 仪表盘**（第二屏）：`claude-hud serve` 在 localhost:9527 提供实时网页监控。

**核心竞争力**（相对 claude-hud / soffit 等竞品）：全屏 TUI 仪表盘 + 代理可观测性 + Skills/MCP 追踪 + 跨会话历史 + Rhai 脚本扩展。

**工程特点**：单一二进制、零运行时依赖（无 Node/Python）、跨平台（Windows/macOS/Linux）、无 Nerd Font 时自动降级图标。

---

## 2. 技术栈

| 层级 | 选型 | 用途 |
|------|------|------|
| CLI 框架 | `clap` 4 (derive) | 25 个子命令 |
| JSON 解析 | `serde` + `serde_json` | stdin SessionData + Transcript JSONL |
| TUI 引擎 | `ratatui` 0.29 + `crossterm` 0.28 | 全屏仪表盘 |
| 紧凑渲染 | `crossterm` ANSI 序列（手写，不用 ratatui） | 状态栏输出 |
| 脚本扩展 | `rhai` 1 | 用户自定义 Widget 沙箱 |
| 持久化 | `rusqlite` 0.32 (bundled) | 跨会话历史 |
| 通知 | `notify-rust` 4 | OS 原生通知 |
| HTTP | `ureq` 2 + `tiny_http` 0.12 | HTTP 轮询 Widget + Web 仪表盘 |
| 配置 | `toml` 0.8 | 用户配置解析 |
| 路径 | `dirs` 5 | 跨平台家目录定位 |

发布配置：`opt-level=3`、`lto=true`、`codegen-units=1`、`strip=true`。

---

## 3. 项目结构

```
claude-hud/
├── Cargo.toml                 # 依赖与发布配置
├── plugin.json                # Claude Code 插件清单
├── README.md / DEPLOY.md      # 使用与部署文档
├── DESIGN.md                  # 原始设计蓝图（部分超前于实现）
├── PLUGIN.md                  # 打包与市场发布文档
├── CHANGELOG.md
├── COMPLETE.md                # 本文档
├── src/
│   ├── main.rs                # CLI 入口：25 个子命令分发
│   ├── compact.rs             # 紧凑模式：stdin → widgets → ANSI 多行输出
│   ├── dashboard.rs           # 仪表盘：ratatui 事件循环 + 布局 + 通知触发
│   ├── serve.rs               # Web 仪表盘 HTTP 服务器
│   ├── doctor.rs              # 自检命令（健康报告）
│   ├── notify.rs              # OS 原生通知封装
│   ├── core/
│   │   ├── session.rs         # SessionData + stdin JSON 反序列化
│   │   ├── transcript.rs      # Transcript JSONL 增量解析 + 统计/归因/预测
│   │   ├── widget.rs          # Widget trait + WidgetRegistry + WidgetConfig
│   │   ├── theme.rs           # 主题引擎：20 token、6 预设、图标集决议、字体探测
│   │   ├── config.rs          # AppConfig / ModPackage 加载（内置预设编译进二进制）
│   │   ├── cc_config.rs       # settings.json 合并/移除 statusLine（原子写 + 备份）
│   │   ├── history.rs         # SQLite 跨会话历史
│   │   ├── scripting.rs       # Rhai 引擎 + Shell 命令 + HTTP 轮询
│   │   ├── animation.rs       # 时间相位纯函数（now_phase/breathe/gradient/ease_out/scanline_offset）
│   │   ├── i18n.rs            # 轻量 i18n：Language/回退链/tr/tr_dyn（include_str! 内嵌字符串表）
│   │   └── ansi.rs            # ANSI True Color / 截断等工具
│   ├── widgets/               # 14 个内置 Widget + 脚本 Widget
│   └── probe/
│       ├── git.rs             # Git 状态探测（分支/脏/领先落后）
│       └── filesystem.rs      # Skills/MCP 数量扫描
├── src/presets/*.toml         # 6 个出厂预设 Mod（include_str! 编译进二进制）
├── locales/en.toml            # 英文基准字符串表（全量）
├── locales/zh.toml            # 中文表（en 子集，缺失回退 en）
├── scripts/
│   ├── install.sh / install.ps1 / uninstall.sh / uninstall.ps1
│   ├── example.rhai           # Rhai 自定义 Widget 示例
│   └── test_hud.py + hudlib/  # 黑盒测试套件
├── fixtures/                  # 测试夹具（config/json/mods/transcript）
├── reports/                   # 测试报告输出
└── .github/workflows/release.yml  # 4 平台发布矩阵
```

---

## 4. 数据来源（四类）

### 4.1 stdin JSON（Claude Code 状态行直接提供）

`SessionData` 反序列化自状态行管道的 JSON（`src/core/session.rs`）：

| 字段 | 类型 | 说明 |
|------|------|------|
| `model.id` / `display_name` | string | 模型 ID 与显示名 |
| `context_window.used_percentage` | f64 | 上下文使用百分比 |
| `context_window.total_input_tokens` / `total_output_tokens` | u64 | 输入/输出 token |
| `context_window.context_window_size` | u64 | 窗口总大小 |
| `context_window.current_usage` | 结构 | input/output/cache_creation/cache_read 分项 |
| `cost.total_cost_usd` | f64 | 会话费用 USD |
| `cost.total_duration_ms` | u64 | 会话耗时 |
| `cost.total_lines_added` / `total_lines_removed` | u64 | 代码增删行 |
| `rate_limits.five_hour.used_percentage` / `seven_day.used_percentage` | f64 | 速率限制 |
| `transcript_path` | Option\<string> | 会话 Transcript 文件路径 |
| `subagent_status_line.agents[]` | 数组 | 子代理：name/model/task/elapsed_secs/is_active |

**健壮性**：`deserialize_null_as_default` 把显式 `null` 值当作默认值处理（Claude Code 会话开始时常发送 `null`）。

> ⚠️ **注意**：Rust 字段为 snake_case（`subagent_status_line`），未配置 `#[serde(rename)]`。若 Claude Code 实际下发 camelCase（`subagentStatusLine`），该字段将始终为 `None`，代理总览 Widget 退化为隐藏。当前仅依赖字段结构设计的匹配。

### 4.2 Git 命令探测（`src/probe/git.rs`）

| 数据 | 命令 |
|------|------|
| 当前分支 | `git branch --show-current` |
| 脏状态 | `git status --porcelain` |
| 领先/落后 | `git rev-list --count @{u}..HEAD` / `HEAD..@{u}` |

非 Git 仓库或无 git 命令时返回 `None`，Widget 静默渲染占位符 `—`，不报错。

### 4.3 静态文件扫描（`src/probe/filesystem.rs`）

| 数据 | 来源 |
|------|------|
| Skills 数量 | 扫描 `~/.claude/skills/` + 当前目录 `.claude/skills/`（目录数） |
| MCP 服务器数量 | 解析 `~/.claude/settings.json` 中 `mcpServers` 出现次数（启发式） |

扫描结果注入环境变量 `CLAUDE_HUD_SKILL_COUNT` / `CLAUDE_HUD_MCP_COUNT`，供 `skills_mcp` Widget 读取。

### 4.4 Transcript JSONL（`src/core/transcript.rs`）

增量解析器 `TranscriptReader`：按文件偏移（`last_pos`）只读取新增行，8 种事件类型：

| 事件 | 处理 |
|------|------|
| `tool_use` | 工具调用计数；`mcp__server__tool` 格式识别为 MCP 调用；`Skill` 工具识别 skill 名称 |
| `tool_result` | 解析（当前仅存名字段） |
| `user` / `assistant` | assistant 消息提取 `usage`：input/output/cache_creation/cache_read 累计 |
| `compact_boundary` | 事件占位 |
| `subagent_start` | 登记代理记录（name/model/task/开始时间） |
| `subagent_stop` | 代理标记完成（记录结束时间） |
| 其他 | 忽略 |

**产出 `TranscriptSummary`**：

| 统计 | 说明 |
|------|------|
| `agents` | 代理列表（含 token、工具调用数） |
| `tool_counts` | 工具调用次数表 |
| `skill_calls` | 本会话 Skill 调用（名/次数/最后时间/活跃） |
| `mcp_calls` | MCP 服务器×工具调用统计 |
| `token_timeline` | 每 60s 分桶的 token 消耗快照 |
| `total_tokens` | input/output/cache 总量 |

**派生分析**：
- `token_attribution()` — 按代理工具调用数占比估算 token 归因（启发式，非真实 token 归属）
- `stalled_agents(threshold)` — 卡顿检测（活跃且超过阈值无工具调用）
- `compaction_prediction()` — 由 token 消耗速率线性外推压缩时间（分钟）

> ⚠️ **已知限制**：时间轴可靠性依赖会话首条事件携带有效 ISO8601 时间戳——可靠会话使用真实墙钟时间（单调不回退，缺失 ts 的行沿用最新已知 ts），不可靠会话退化为"每行 +1 秒"模拟（`current_secs`）；`timestamps_reliable` 标志跨进程持久化。`AgentRecord.last_tool_call_secs` 与 `last_tool_name` 在 `tool_use` 分支已更新（归属最近激活代理），`stalled_agents()` 仅在可靠时间轴下有判定意义；agent_detail 卡顿标红与卡顿通知（§11.2）共用该判定。

---

## 5. Widget 系统

### 5.1 核心抽象（`src/core/widget.rs`）

```rust
pub trait Widget {
    fn id(&self) -> &str;                       // 唯一标识
    fn display_name(&self) -> &str;             // 展示名
    fn render_compact(&self, data, theme, config) -> String;  // ANSI 单行
    fn render_dashboard(&self, data, area, frame, theme, config); // ratatui 面板
    fn update_transcript(&self, summary);       // 接收 Transcript 摘要
}
```

`WidgetConfig` 是 `HashMap<String,String>` 的薄封装，提供 `get_str/get_bool/get_f64/get_u64`，从 `config.toml` 的 `[widgets.<id>]` 表反序列化而来（值统一转字符串）。

`WidgetRegistry` 持有全部 Widget 实例；主流程（main.rs）依次注册 14 个内置 Widget + 按配置实例化脚本 Widget。

### 5.2 内置 Widget 清单

**Phase 1（7 个，依赖 stdin + 静态扫描）**

| ID | 紧凑输出示例 | 仪表盘 | 数据源 | 状态 |
|----|-------------|--------|--------|------|
| `model_display` | `▸ Opus 4.7` | `Model: Opus (claude-opus-4-7)` | stdin `model.*` | ✅ |
| `context_bar` | `ctx ████░░ 52%` | ratatui Gauge + token 计数 | stdin `context_window.*` | ✅ |
| `cost_display` | `¥1.42`（超阈值变黄） | `Cost: $x | m s | +n/-n lines` | stdin `cost.*` | ✅ |
| `agent_overview` | `⚡ 2/3 agents`（卡顿时 ⬤ 红） | 总数/活跃/完成/百分比 | stdin `subagent_status_line` | ✅ |
| `skills_mcp` | `◇ 2 ◆ 4` | 静态占位文本 | 环境变量扫描 | ✅ |
| `rate_limits` | `5h:34% 7d:12%`（超 90% 变红） | 5h Gauge + 7d 文本 | stdin `rate_limits.*` | ✅ |
| `git_status` | `main* ↑3`（无仓库渲染 `—`） | 分支/脏/领先/落后 | git 命令 | ✅ |

**Phase 2（6 个，依赖 Transcript 解析）**

| ID | 紧凑输出 | 仪表盘 | 状态 |
|----|---------|--------|------|
| `agent_detail` | 每活跃代理一行：◐ 名 任务 耗时 | 列表 + 卡顿呼吸红色标记 | ✅ |
| `token_attribution` | `top:<代理> n%` | Top-8 渐变条形图 | ✅ |
| `agent_timeline` | （不渲染，空串） | Token sparkline + 代理列表 | ✅ |
| `session_stats` | `⏱ 12m · 342 tok/s · 5 calls` | 耗时/行数/工具 + Top-5 工具 | ✅ |
| `skills_mcp_dynamic` | 活跃 Skill/MCP 名 | 调用次数统计 | ✅ |
| `alerts` | 阈值告警链（呼吸闪烁） | 告警面板 | ✅ |
| `token_rate` | `tok 3.1k/min` 速率文本（空数据 `—`） | 最近 24 桶盲文频谱竖条 | ✅ |

**Phase 3（脚本 Widget，见 §13）**：`script_rhai` / `script_shell` / `script_http`。

> 注意：所有 Widget 的 `id()` 与注册名一致；脚本 Widget 的 id 为固定类型名（`script_rhai` 等），多个同类型脚本只能有一个被渲染。

---

## 6. 紧凑状态栏（`src/compact.rs`）

### 6.1 渲染管线

```
stdin JSON → SessionData → (Transcript 增量解析 → 广播 update_transcript)
→ 按 compact_layout 顺序渲染每个 Widget → 每行用 separator 连接 → 输出多行 ANSI
```

- 行数：`runtime_overrides.compact_lines` 优先，否则 `theme.compact_lines`
- 单行 widget 数 = ceil(布局总数 / 行数)，顺序切分
- 空输出 Widget 自动跳过；空行跳过
- 无错误时输出 `trim_end`（不输出尾随换行，避免状态行变形）

### 6.2 零宽度感知（⑮）

- 宽度源：`COLUMNS` 环境变量（statusLine 场景唯一可靠来源）；非法值回退 80，最小 40
- `fit_line`：从行尾**整组**丢弃直至可见宽度 ≤ 上限（剥 ANSI 后按 Unicode 宽度测量，CJK 计 2）
- 字段级截断：model 名 / git 分支 / 代理名统一 24 字符截断（`ansi::truncate`）；`agent_detail` 的任务描述另有 40 字符截断

### 6.3 布局控制

- 紧凑布局**实际由** `compact_layout` 数组（Widget 顺序）+ 行数控制
- **Mod 布局注入**（`resolve_compact_layout`）：激活 Mod 的 `compact_widgets` 快照优先（mod save 时捕获）；否则按 `layout.compact` 映射（minimal/activity 已实现；agent-centric/kpi/contextual/full 报 "not implemented"）
- 行数三层优先：`runtime_overrides.compact_lines` → Mod `compact_lines` → `theme.compact_lines`
- 分隔符：`config.separator`（默认 ` │ `）

---

## 7. 全屏仪表盘（`src/dashboard.rs`）

- 进入 alternate screen + raw mode，500ms tick 事件循环
- 每 tick：读 stdin 快照 → Transcript 增量解析广播 → 检查通知条件 → 重绘
- 布局模式（由 `dashboard.default_layout` 选择）：

| 模式 | 实现 | 面板数 |
|------|------|--------|
| `grid-2x2` | ✅ 2×2 网格 | 4 |
| `sidebar` | ✅ 左 1/3 + 右 2/3 上下 | 3 |
| `tabbed` / `focus` | 🟡 退化为单全宽面板 | 1 |
| `hex-2x3` / `freeform` | ⬜ 未实现（回退 grid-2x2） | — |

- 面板分配：按 `compact_layout` 顺序映射到面板区域，超出面板数取 `context_bar` 兜底
- 快捷键：`q` / `Esc` 退出（退出时记录会话到历史库，携带真实代理数）；`l` 循环布局 grid-2x2 → sidebar → focus（best-effort 持久化到 config.toml `dashboard.default_layout`）；`?` 开合帮助面板；底部 1 行 footer 常驻显示 Layout / Mod / 键位提示
- 通知触发（§11）在仪表盘模式下工作；紧凑模式不触发

---

## 8. 主题系统（`src/core/theme.rs`）

### 8.1 20 个 Token

**颜色（11）**：`bg` `fg` `accent` `success` `warning` `danger` `muted` `border` `skill_color` `mcp_color` `model_color`

**样式（9）**：`bar_filled`(char) `bar_empty`(char) `separator`(string) `border_style`(enum: single/double/rounded/thick/hidden) `icon_set`(enum) `bar_width` `padding` `compact_lines` `dashboard_grid`

### 8.2 6 套内置主题

| 主题 | 背景 | 主色 | 气质 |
|------|------|------|------|
| dracula | #282a36 | 紫 #bd93f9 / 绿 #50fa7b | 赛博朋克 |
| **nord（默认）** | #2e3440 | 青 #88c0d0 / 绿 #a3be8c | 北欧冰川 |
| tokyo-night | #1a1b26 | 蓝 #7aa2f7 | 都市夜 |
| catppuccin | #1e1e2e | 紫 #cba6f7 | 柔和护眼 |
| monochrome | #1a1a1a | 纯灰度 | 极简 |
| solarized-dark | #002b36 | 青 #2aa198 | 经典 |

### 8.3 图标集与自动决议（`IconSet::Auto`）

| 图标集 | 示例 | 依赖 |
|--------|------|------|
| `nerd` | 🧩 🔌 | 需安装 Nerd Font |
| `minimal` | ◇ ◆ ▸ | 零依赖 |
| `ascii` | [SK] [MC] | 零依赖，纯 ASCII |
| `auto`（默认） | 动态决议 | 探测字体后映射到 nerd 或 minimal |

**字体探测**（`detect_nerd_font`，失败永不 panic）：
- Windows：`reg query HKLM\...\Fonts` 输出含 "nerd"
- Linux：`fc-list` 输出含 "nerd"
- macOS：扫描 `/System/Library/Fonts`、`/Library/Fonts`、`~/Library/Fonts` 文件名

决议规则（`resolve_icon_set_with`）：Auto → 有 Nerd Font 则 Nerd，否则 Minimal；**显式选择永不被降级**（如显式 `nerd` 无字体时仍用 nerd）。

### 8.4 主题引用形态（ThemeRef 双形态 + 四层叠加）

1. `theme = "dracula"` — 字符串预设引用（✅ ThemeRef 双形态之一，直接生效）
2. `[theme]` 表 — `preset` + 显式键 + `overrides`（✅ 完整支持，键可用 `..Default` 语义省略）
3. Mod 引用 — 激活 Mod 的 `[theme].preset` / `[theme].overrides` 参与叠加（✅）

**叠加链（低 → 高）**：基底（Mod preset → config 字符串预设 → 默认 nord）→ config `[theme]` 显式键 → config `[theme].overrides` → Mod `[theme].overrides`（最高优先级）

> 解析失败不再静默：stderr 输出 `[claude-hud] warning:` 警告并回退默认配置；`doctor` 对坏配置输出 `[!!]` 并返回非零退出码。

---

## 9. 动画系统（`src/core/animation.rs`）

### 9.1 现状（v0.4 时间相位重建）

动画系统重建为**墙钟时间相位纯函数**（2026-08-04）：`now_phase(period)` 返回周期内位置 [0,1)，`CLAUDE_HUD_PHASE` env（合法 f64 ∈ [0,1)）覆盖以获得黑盒确定性（COLUMNS 先例）；`breathe(hex, phase)` 正弦亮度脉动（phase 0.25 全亮 / 0.75 最暗 0.45×）；`gradient(hex_a, hex_b, t)` 线性 RGB 插值；`ease_out(t)`；`scanline_offset(phase, height)`。frame 制 `AnimationState` 已删除（与 5s 进程重生架构不兼容：紧凑进程每进程仅 1 帧）。

### 9.2 实际接入情况（6 效果接线）

- ✅ 渐变进度条：context_bar 紧凑进度条逐 cell truecolor 渐变（success→danger，接线既有 `gradient` 配置键，默认开；关 → 3 档变色）
- ✅ 呼吸：alerts critical 分支与 agent_detail 卡顿标记使用 `breathe(danger, now_phase(4.0))`（4s 周期正弦脉动）
- ✅ 缓动计数器：仪表盘 cost_display 0.8s ease-out 滚动（唯一进程内动画状态，常驻进程适用；紧凑进程不适用拍板确认）
- ✅ CRT 扫描线：dashboard 背景每 4 行 border 色 dim 行 + 相位行进 accent 扫描带（`[dashboard] scanlines`，默认开）
- ✅ 伪 3D 面板：focus/tabbed 布局 accent 边框（光源）+ 右下偏移 1 格 border 色阴影块（ratatui 0.29 无按侧边框样式，用偏移阴影实现 bevel）
- ✅ 盲文频谱：新 widget `token_rate` 仪表盘最近 24 桶竖条（8 级块字符）+ 紧凑 `tok 3.1k/min` 速率文本（尾桶增量口径；空数据 `—`）

---

## 10. Mod 系统（`src/core/config.rs` + `main.rs`）

### 10.1 Mod 文件格式

```toml
[mod_info]
name = "Glacier Workstation"
version = "1.0.0"
description = "..."
scene = "daily-dev"          # @daily 场景别名

[layout]                     # compact/dashboard/compact_lines（compact 驱动渲染注入）
[theme]                      # preset（+ overrides 参与四层叠加）
[animation]                  # enabled + effects（元数据）
[widgets.<id>]               # 每 widget 配置
```

### 10.2 6 个出厂预设（编译进二进制）

| Mod | scene | layout.compact | theme | 适合 |
|-----|-------|----------------|-------|------|
| **glacier-workstation（默认）** | daily-dev | activity | nord | 日常开发 |
| obsidian-command | heavy-agent | agent-centric | dracula | 重度代理 |
| ember-night | night-coding | kpi | solarized-dark | 深夜 |
| matrix-surveillance | ssh-remote | activity | monochrome | SSH/远程 |
| noir-precision | daily-dev | minimal | monochrome | 极简 |
| noir-tabbed | small-screen | contextual | monochrome | 小屏 |

用户 Mod 存 `~/.claude/plugins/claude-hud/mods/*.toml`；加载顺序：内置 → 用户目录。

### 10.3 生效链路

```
config.toml: active_mod = "xxx"
→ 主题：Mod 的 theme.preset / overrides 参与四层叠加（§8.4）
→ 渲染：Mod 的 compact_widgets 快照 / layout.compact 注入紧凑布局（§6.3）
```

### 10.4 Mod 管理命令

✅ 全部 11 项实现：`list` / `use`（校验存在性，`-` 经 previous_mod 往返，`@scene` 场景别名）/ `preview` / `current` / `save`（当前配置真实快照：合并主题 + compact_widgets + widgets 段）/ `export` / `import` / `delete` / `reset` / `pick`（序号选择器）

---

## 11. 智能预警与通知（`src/widgets/alerts.rs` + `src/notify.rs`）

### 11.1 紧凑模式内联告警（alerts Widget）

| 触发条件（默认阈值） | 表现 |
|---------------------|------|
| 上下文 ≥ 95% | `⚠ ctx n%` 红/黄呼吸闪烁 |
| 上下文 ≥ 80% | `ctx n%` 黄色 |
| 费用 ≥ ¥10 | `¥x.xx` 黄色 |
| 5h 速率限制 ≥ 90% | `5h limit!` 红色 |
| 卡顿代理 | `⚠ n stalled` |
| 压缩预测 < 10 分钟 | `compact ~nm` |

### 11.2 OS 通知（notify-rust，5 秒超时）

`notify.rs` 提供 5 个便捷函数，全部接线（alert.rs `send_notifications` + dashboard.rs 通知块）：

| 函数 | 触发条件 | 接线 |
|------|---------|------|
| `context_critical` | 上下文 ≥ 95% | ✅ |
| `cost_threshold` | 费用 ≥ $10 | ✅ |
| `rate_limit_warning` | 5h 限制 ≥ 90% | ✅ |
| `agents_complete` | 所有代理完成（活跃数 >0 → 0 边沿，进程内去重） | ✅ |
| `agent_stalled` | 代理卡顿（超阈值无工具调用） | ✅ |

阈值当前为代码常量（95/10/90），未读配置；代理类通知为进程内去重（同条件不重复弹）。

---

## 12. 跨会话历史（`src/core/history.rs`）

SQLite：`~/.claude/plugins/claude-hud/history.db`，表 `sessions`：

| 列 | 说明 |
|----|------|
| id / started_at | 自增 ID + 时间戳 |
| duration_secs / total_cost_usd / total_tokens | 时长/费用/token |
| agent_count / lines_added / lines_removed | 代理数/增删行 |
| mod_used | 当时激活的 Mod |
| model / transcript_path | 会话模型 / transcript 路径（v0.6 新增列；旧库首次打开 ALTER TABLE 自动补齐） |

查询能力：`weekly_stats()`（近 7 天汇总）、`recent_sessions(n)`、`daily_cost_trend()`（近 7 天每日费用）、`sessions_page(limit, offset, date)`（⑤ 分页列表）、`session_by_id(id)`（⑥ 单会话详情）。

记录时机：**仪表盘退出时**（q/Esc）；**紧凑模式会话切换自动结账**——render 检测到 `transcript_path` 变化即把上一会话写入历史库（失败仅 stderr 警告，不中断渲染）。`claude-hud history` 子命令输出三块统计（§15）。📊 TUI 仪表盘暂未展示历史趋势（Web 面板已有 This Week 卡片，见 §14）。

---

## 13. 脚本扩展（`src/core/scripting.rs` + `src/widgets/script_widget.rs`）

### 13.1 Rhai 脚本 Widget

- 注入 `data` 对象（11 个字段：model_id/model_name/context_pct/input_tokens/output_tokens/cost_usd/duration_ms/lines_added/lines_removed/rate_5h_pct/rate_7d_pct）与 `theme` 对象（7 个颜色 hex）
- 脚本返回 String 即 Widget 输出；5s 刷新缓存
- 示例：`scripts/example.rhai`（含 ANSI 上色辅助函数）

### 13.2 Shell 命令 Widget

```toml
[widgets.ci_status]
type = "shell_output"
command = "curl -s ... | jq -r '.status'"
refresh_seconds = 30
```

### 13.3 HTTP 轮询 Widget

```toml
[widgets.weather]
type = "http_poll"
url = "https://api..."
refresh_seconds = 300
```

- Shell/HTTP 均有刷新节流（`last_refresh` + 时间判断）与缓存
- 失败输出 `rhai: / shell: / http: <err>` 前缀，不崩溃
- 注册：main.rs 扫描 `config.widgets` 表中 `type` 字段自动实例化

---

## 14. Web 仪表盘（`src/serve.rs`）

- `claude-hud serve` → `http://localhost:9527`（绑定 127.0.0.1）
- 路由：

| 路由 | 说明 |
|------|------|
| `/` / `/index.html` | 内置单页 HTML（深色卡片式，JetBrains Mono） |
| `/api/data` | JSON：model/context_pct/cost_usd/duration_ms/weekly + 全部 Widget 紧凑输出 |
| `/api/health` | `OK` |

- `weekly` 字段：本周聚合（total_cost/total_sessions/total_tokens/avg_duration_min/avg_agents_per_session）；历史库 open/query 失败时返回 `available:false` 全 0（前端显示 `—`）
- 前端每 2s 轮询 `/api/data`，卡片含模型、上下文进度条、费用、时长、**This Week 历史卡片** + 各 Widget 输出区
- 实现使用 `Box::leak` 持有 registry/config/theme（静态生命周期）

---

## 15. CLI 完整命令参考（25 个子命令）

### 基础（6）

| 命令 | 说明 | 状态 |
|------|------|------|
| `render` | 紧凑模式：stdin → ANSI 状态栏（Claude Code 自动调用） | ✅ |
| `dashboard` | 全屏 TUI 仪表盘 | ✅ |
| `serve` | Web 仪表盘 :9527 | ✅ |
| `setup` | 创建 config.toml + 合并 statusLine 到 settings.json | ✅ |
| `uninstall` | 移除 statusLine + 删除插件配置目录 | ✅ |
| `doctor` | 自检：binary/config/statusLine/图标/git/样例渲染/update | ✅ |

### 历史与升级（4）

| 命令 | 说明 | 状态 |
|------|------|------|
| `history` | 输出 Weekly stats / Recent sessions / Daily cost 三块；空库各块显示 `—`（不显示 0） | ✅ |
| `sessions` | 分页会话列表（`--limit` / `--offset` / `--date`；空库 `—`） | ✅ |
| `session <id>` | 单会话详情（模型/成本/时长/代理/token；transcript 尾读补明细；未找到 exit 1） | ✅ |
| `update check` | 查询 GitHub latest release 与本地版本比较；仓库无 release（404）或离线时输出 `not published yet` / `update check unavailable` | ✅ |

### Mod（10）

| 命令 | 说明 | 状态 |
|------|------|------|
| `mod list` | 内置 + 用户 Mod 及激活标记 | ✅ |
| `mod use <name>` | 写 active_mod 到 config.toml | ✅ |
| `mod use -` | 经 state.json previous_mod 往返切换上一 Mod（无记录时报错） | ✅ |
| `mod preview <name>` | 打印 Mod 元信息 | ✅ |
| `mod current` | 当前激活 Mod 详情 | ✅ |
| `mod save <name>` | 用模板（activity/grid-2x2/nord）生成新 Mod 文件 | ✅（内容固定模板，非当前配置快照） |
| `mod export <name>` | 导出为 .toml 到 stdout | ✅ |
| `mod import <file>` | 校验并写入 mods/ 目录 | ✅ |
| `mod delete <name>` | 删除用户 Mod（内置不可删） | ✅ |
| `mod reset` | 恢复默认 config（Glacier Workstation） | ✅ |
| `mod pick` | 序号选择器：内置 + 用户 Mod 列表，输入序号切换 | ✅ |

### 主题 / Widget / 补全（5）

| 命令 | 说明 | 状态 |
|------|------|------|
| `theme export` | 导出当前主题 TOML | ✅ |
| `theme import <file>` | 校验并写入 config.toml `[theme]` 段（保留其他段） | ✅ |
| `widget list` | 列出全部已注册 Widget | ✅ |
| `widget test <name>` | 用空 SessionData 渲染测试 | ✅（数据为空，验证不 panic） |
| `completion <shell>` | clap_complete 生成真实补全脚本（bash/zsh/fish/powershell；不支持 shell 报错 exit 1） | ✅ |

**setup 细节**（`cc_config.rs`）：备份原 settings.json 为**时间戳文件** `settings.json.hud.bak-<epoch>`（已存在 statusLine 或 JSON 损坏时每次 setup 都写新备份；`.hud.bak-*` 永不被 setup/uninstall 删除；无 `.json.bak` 迁移逻辑）→ 合并/替换 `statusLine`（type=command, command=`claude-hud render`, refreshInterval=5）→ **原子写**（临时文件 + rename，防中途崩溃截断）。损坏 JSON 自动备份并重建。

**doctor 检查项**：binary 存在与版本、config.toml 存在且可解析、settings.json statusLine 指向 `claude-hud render`、图标集决议结果、git 可用性（可选提示）、样例 JSON 渲染不 panic、`update:` 信息项（复用 `update check` 状态，恒 exit 0）。任一失败返回非零退出码。

---

## 16. 配置参考（config.toml）

位置：`~/.claude/plugins/claude-hud/config.toml`

```toml
active_mod = "glacier-workstation"   # 激活 Mod
preset = "full"                      # 蓝图遗留，当前不驱动渲染
separator = " │ "                    # 紧凑模式分隔符
compact_layout = [                   # Widget 顺序（仪表盘面板也按此顺序映射）
    "model_display", "context_bar", "agent_overview",
    "cost_display", "skills_mcp", "alerts",
]

[dashboard]
refresh_interval_ms = 500            # 仪表盘 tick
default_layout = "grid-2x2"          # grid-2x2 | sidebar | tabbed | focus

[theme]                              # 可选：内联完整主题表（见 §8.4）

[widgets.context_bar]                # Widget 级配置（值均为字符串）
bar_width = "18"
warn_threshold = "80"
critical_threshold = "95"

[widgets.cost_display]
currency_symbol = "¥"
warn_threshold_usd = "10.0"

[widgets.agent_overview]
stall_threshold_sec = "30"

[widgets.alerts]
context_warn = "80"
context_critical = "95"
cost_warn_usd = "10.0"

[widgets.ci_status]                  # Phase 3 脚本 Widget（§13）
type = "shell_output"
command = "..."
refresh_seconds = "30"

[runtime_overrides]                  # 临时覆盖（不修改 Mod）
compact_lines = 1
```

Claude Code 集成（`setup` 自动写入 settings.json）：

```json
{
  "statusLine": {
    "type": "command",
    "command": "claude-hud render",
    "refreshInterval": 5
  }
}
```

---

## 17. 安装 / 卸载 / 自检

### 一键安装（无需 Rust）

```bash
# macOS/Linux
curl -fsSL https://raw.githubusercontent.com/cuishiying-1/claude-hud/master/scripts/install.sh | bash
# Windows (PowerShell)
irm https://raw.githubusercontent.com/cuishiying-1/claude-hud/master/scripts/install.ps1 | iex
```

安装器流程（install.sh）：
1. 平台/架构检测（linux-x64 / macos-x64 / macos-arm64；不支持即报错退出）
2. 安装目录 `~/.local/bin`（Windows：`%LOCALAPPDATA%\claude-hud\bin`），PATH 校验与注入（bash/zsh rc 文件或用户级 PATH 注册表）
3. **Release 解析**：从 GitHub Releases 解析 latest tag（仓库无 release 时 404 → 明确报错退出）；**三态输出**——`installing` / `up to date`（`version.txt` 与 latest 相同，幂等跳过）/ `upgrading`；`version.txt` 存 tag 原始值（含 `v`），展示时剥离 `v` 前缀
4. 本地开发模式：`HUD_LOCAL_BIN` / `HUD_LOCAL_STUB` 环境变量跳过网络
5. 自动执行 `claude-hud setup`

### 卸载（uninstall.sh / uninstall.ps1）

调用 `claude-hud uninstall`：移除 settings.json 中的 statusLine（保留其余配置）→ 删除插件配置目录 → 提示可安全删除二进制。

### 自检

`claude-hud doctor`（见 §15）。故障排查：`echo '{...}' | claude-hud render` 可独立验证渲染。

---

## 18. CI/CD 与发布（`.github/workflows/release.yml`）

- 触发：tag `v*`（含 PR 校验）
- 矩阵 4 平台：macos-x64 / macos-arm64 / linux-x64 / windows-x64（msvc）
- 打包：tar.gz（mac/linux）/ zip（windows）→ 上传 artifact → `softprops/action-gh-release` 建 Release（body 用 CHANGELOG.md）
- 版本策略：Major=破坏性变更，Minor=新功能，Patch=修复；首次 v0.1.0
- 市场发布路径：plugin.json（见 PLUGIN.md）→ `/plugin marketplace add` / `/plugin install`

---

## 19. 测试体系

### 单元测试（cargo test）

| 模块 | 覆盖点 |
|------|--------|
| `theme.rs` | Auto 决议（有/无 Nerd Font、显式不降级） |
| `cc_config.rs` | merge/remove statusLine：空输入、保留其他键、替换、非法 JSON、null 根 |
| `git_status.rs` | 无仓库占位、分支渲染、脏/领先/落后标记 |

### 黑盒测试套件（scripts/test_hud.py + hudlib/）

- 针对 release 二进制执行端到端用例（render/serve/setup/mod/doctor）
- `hudlib/`：`cases`（用例定义）、`assertions`（断言）、`runner`（执行器）、`report`（Markdown 报告）、`env`（配置备份/恢复）
- 夹具 `fixtures/`：config 变体（ascii_theme/空布局/全部 13 widget/未知布局/分隔符）、JSON 变体（null 字段/unicode/垃圾输入）、mods（smoke-a/b）、transcript（valid/corrupted/empty/agents）
- 运行：`python scripts/test_hud.py`（支持 `--case` / `--exe` / `--report`），输出到 `reports/`

---

## 20. 实现状态总览（蓝图 vs 现状）

### ✅ 完整实现

CLI 25 子命令 · 14 内置 Widget · 3 脚本 Widget · 主题 20 token + 6 预设 + 字体探测 · 图标 auto 决议 · 6 出厂 Mod · Transcript 增量解析与统计 · 紧凑渲染管线 · 仪表盘 4 布局 · Web 面板 · SQLite 历史数据层 · 5 类 OS 通知 · setup/uninstall/doctor · 一键安装脚本 ×2 平台 · CI 发布矩阵 · 单元测试 + 黑盒测试套件 · 数据通路（state.json 5 段全量原子写 + Transcript 跨进程游标累计 + 告警跨进程冷却 + 越阈告警配置化 + doctor 自检 last_error 上报）· 输入契约（subagentStatusLine/扁平 rate_limits 双形态 + render --dump 键分类 + doctor 契约探针）· 真实时间轴（ISO8601 主时间轴 + timestamps_reliable 降级 + epoch 60s 分桶 + 真实卡顿/压缩预测）· 成本正确性（currency_symbol 全局 + [pricing] 三态重算 + context_bar tokens + doctor 负单价校验）· 配置契约（ThemeRef 双形态 + 四层叠加 + 失败警告 + import 落盘）· Mod 真相（use 校验 + previous_mod + @scene + save 快照 + 渲染灌入 + pick）· ANSI 整段上色（4 widget + 黑盒 ANSI 结构断言）· 历史库消费（history 三块输出 + render 会话切换自动结账 + serve weekly 字段 + Web This Week 卡片 + 黑盒用例 130 例）· Shell Widget 跨平台（Windows cmd /C、Unix sh -c + 死代码清理）· 补全真实现（clap_complete：bash/zsh/fish/powershell）· 零宽度感知（COLUMNS 宽度源 + fit_line 组级截断 + 字段 24 字符截断）· dashboard 交互（l 布局循环 + default_layout 持久化 + ? 帮助面板 + 底部 footer + ←/→ tab 切换）· 通知全接线（5/5 + 进程内去重）· 安装健壮性（无 release 明确报错 + 三态输出 + setup 时间戳备份）· 全局生效提示（写配置命令 8 处 `(applies to all windows)`）· 升级通路（update check 404/离线降级 + doctor update:）· v0.2 成本哨兵（realtime_cost 双轨 + cost_display 合并单组 `≈$X · Xk/Xk tok` + 零数据 `—` 降级 + `[budget]` 档位单调/跨进程冷却 + doctor 档位读取 + `history --weekly` 五指标 + serve 周趋势曲线 + 黑盒用例 138 例）· v0.3 性能与卫生（token_timeline 360 桶上限 + 结账 path→ts 表去重（振荡防 double-billing）+ serve 历史 30s TTL 缓存 + 状态栏预算占比 `· NN%` + 17 个构建 warning 清零（动画原语收缩/死代码清理）+ 黑盒用例 141 例）· v0.4 视觉批次（时间相位动画重建 + 6 效果接线 + tabbed 布局补全 + 黑盒用例 147 例）· v0.5 国际化（language 键 + en/zh 表 + 回退链 + clap 后处理注入 + serve JS T 表 + doctor/main 全量接入 + CLAUDE_HUD_CONFIG env 注入 + 黑盒用例 152 例）· 批次 III 布局补全（agent-centric/kpi/contextual 真实实现 + contextual 动态两态（subagent 判据）+ 未实现布局回归改接 hex-2x3 + 黑盒用例 156 例 + 单元测试 151 个）· 批次 I 成本与预测（① 内置模型价格库（9 模型 2026-07 官方价 + 用户 [pricing] 覆盖合并 + doctor 内置表信息项）+ ② 实时成本 cache 权重修正（cache_read/cache_creation × 单价，缺失回归不变）+ ③ 成本速率段 `· ≈$X.X/h`（零时长/零成本隐藏）+ ④ 压缩预测标注 `compact ≈Nm`（transcript 首尾桶斜率外推）+ `[alerts] compaction_eta_minutes` 临近通知（默认 15，0=关，复用冷却去重）+ 黑盒用例 165 例 + 单元测试 166 个）· 批次 V 卡顿归因（⑮ AgentRecord.last_tool_name 记录（serde default 旧 state 兼容）+ agent_detail 卡顿归因文本 `stalled 3m · bash`（danger 色，无工具记录维持 elapsed）+ alerts 卡顿行归因 + 不可靠时间轴不假告警 + 黑盒用例 168 例 + 单元测试 170 个）· 批次 II 会话复盘与浏览（⑤ sessions 分页列表 --limit/--offset/--date + ⑥ session 详情（model/transcript_path 入库 migration + transcript 尾读补 token 分解/代理明细）+ ⑦ 工具成本归因排行（估算路径 per_call 均摊 ≈ 标注，未命中 `—`）+ 黑盒用例 180 例 + 单元测试 180 个）

### 🟡 部分实现 / 占位

| 项 | 现状 |
|----|------|
| 仪表盘布局 | hex-2x3/freeform 未实现 |
| Mod 布局 ID | full 未实现（报 "not implemented"）；minimal/activity/agent-centric/kpi/contextual 已实现（批次 III 补全后 6 个出厂 Mod 全部真实渲染） |
| 动画 | 6 效果已接线（渐变进度条/呼吸/缓动计数器/CRT 扫描线/伪 3D 面板/盲文频谱）；其余装饰效果按拍板砍除 |
| 历史展示（TUI） | TUI 仪表盘趋势面板未实现（Web 面板已有 This Week 卡片） |

### ⬜ 设计蓝图未实现（见 DESIGN.md）

紧凑布局 full（minimal/activity/agent-centric/contextual/kpi 已实现，批次 III）· 仪表盘 6 布局中的 2 种 · 15 种动画效果中的多数 · 主题市场 install user/repo · 多会话监控 · Homebrew tap

---

## 21. 路线图

| 阶段 | 内容 | 状态 |
|------|------|------|
| Phase 1 核心骨架 | CLI + 7 Widget + 主题 + 紧凑渲染 + setup | ✅ |
| Phase 1.5 数据通路 | 跨进程 state.json 数据层 + Transcript 游标累计 + 告警冷却 + doctor 诊断 + 黑盒用例 96 例 | ✅ |
| Phase 2 契约与真实性 | 双命名契约 + 真实时间戳 + 成本正确性 + 黑盒用例 106 例 | ✅ |
| Phase 2 深度诊断 | Transcript 解析 + 6 P2 Widget + 历史 + 通知 + 仪表盘 | 🟡 主体完成，历史展示待补 |
| Phase 3 扩展生态 | Rhai/Shell/HTTP Widget + Web 仪表盘 | ✅ |
| Phase 3 配置契约 | ThemeRef 双形态 + Mod 系统真相 + ANSI 修复 + 黑盒用例 123 例 | ✅ |
| Phase 4 分发生态 | GitHub Release CI + 插件市场 + 多会话监控 | 🟡 CI 完成，市场/多会话待做 |
| Phase 4 性能与卫生 | token_timeline 上限 + 结账去重 + serve 缓存 + 预算占比 + warning 清零 | ✅ |
| Phase 4 batch C 剩余（⑨⑩⑪⑮⑯⑰⑱，2026-08-04） | 历史库消费 + Shell Widget 跨平台 + 补全真实现 + 零宽度感知 + dashboard 交互 + 通知全接线 + 安装健壮性 + 升级通路 + 黑盒用例 130 例 + 单元测试 99 个 | ✅ |
| v0.2 成本哨兵（⑲⑳㉑，2026-08-04） | 实时成本状态栏（双轨 + 合并单组）+ 预算告警（档位单调/跨进程冷却）+ 成本周报（--weekly + serve 周曲线）+ 黑盒用例 138 例 + 单元测试 112 个 | ✅（㉑ 使用价值待用户反馈验证） |
| v0.4 视觉批次（动画 6 效果 + tabbed，2026-08-04） | 时间相位动画重建（CLAUDE_HUD_PHASE 确定性）+ 渐变进度条/呼吸/缓动计数器/CRT 扫描线/伪 3D 面板/盲文频谱 + tabbed 布局补全 + 黑盒用例 147 例 + 单元测试 136 个 | ✅ |
| v0.5 国际化（2026-08-04） | language 键 + en/zh 字符串表 + 回退链 + 运行时/clap/Web 三面覆盖 + 黑盒用例 152 例 + 单元测试 147 个 | ✅ |
| v0.6 批次 III 布局补全（2026-08-04） | agent-centric/kpi/contextual 真实实现 + contextual 动态两态 + 6 个出厂 Mod 全渲染 + 黑盒用例 156 例 + 单元测试 151 个 | ✅ |
| v0.6 批次 I 成本与预测（①②③④，2026-08-04） | 内置价格库（①）+ 实时 cache 权重（②）+ 成本速率段（③）+ 压缩预测标注与临近通知（④）+ 黑盒用例 165 例 + 单元测试 166 个 | ✅ |
| v0.6 批次 V 卡顿归因（⑮，2026-08-04） | AgentRecord.last_tool_name 记录 + agent_detail 卡顿归因 `stalled 3m · bash`（无工具记录维持 elapsed）+ alerts 卡顿行归因 + 不可靠时间轴不假告警 + 黑盒用例 168 例 + 单元测试 170 个 | ✅ |
| v0.6 批次 II 会话复盘与浏览（⑤⑥⑦，2026-08-05） | sessions 分页列表（⑤）+ session 详情（⑥，migration + transcript 尾读）+ 工具成本归因排行（⑦，估算路径）+ 黑盒用例 180 例 + 单元测试 180 个 | ✅ |
| Phase 5 持续迭代 | 更多主题、性能优化 | ⬜ |

---

*本文档基于源码（src/ 共约 4100 行）与现有文档（DESIGN/PLUGIN/DEPLOY/README/CHANGELOG）整理，生成于 2026-07-31，更新于 2026-08-05（Phase 4 batch C 剩余 ⑨⑩⑪⑮⑯⑰⑱ + v0.2 成本哨兵 + v0.3 性能与卫生 + v0.4 视觉批次 + v0.5 国际化 + v0.6 批次 III 布局补全 + v0.6 批次 I 成本与预测 + v0.6 批次 V 卡顿归因 + v0.6 批次 II 会话复盘与浏览交付回写）。*
