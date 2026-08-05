# Claude HUD — 设计文档

## 概述

Claude Code 的可配置终端可视化插件。双模架构：紧凑状态栏（日常使用）+ 全屏仪表盘（深度诊断）。

### 竞争定位

| | claude-hud (jarrodwatts) | soffit (noxcraftdev) | Claude HUD (我们) |
|---|---|---|---|
| **语言** | Node.js/Bun | Rust + ratatui | Rust + ratatui |
| **形态** | 紧凑状态栏 | 紧凑状态栏 + GUI 编辑器 | 紧凑 + **全屏仪表盘** |
| **扩展** | 配置开关 | Shell 脚本 | **Rhai 脚本** + HTTP 轮询 |
| **Stars** | 14.5k | 较小 | — |
| **分发** | npm/plugin market | cargo/brew/curl | cargo/plugin market |

**核心差异化**：全屏 TUI 仪表盘（竞品空白）+ Rhai 脚本引擎 + 代理可观测性 + Skills/MCP 追踪 + 跨会话历史分析。

## 技术栈

| 层级 | 选型 | 用途 |
|------|------|------|
| CLI 框架 | `clap` | 子命令：render / dashboard / serve / setup |
| JSON 解析 | `serde` + `serde_json` | stdin SessionData + Transcript JSONL |
| TUI 引擎 | `ratatui` + `crossterm` | 全屏仪表盘 |
| 紧凑渲染 | `crossterm` ANSI + 手写 | 状态栏输出（不用 ratatui） |
| 脚本扩展 | `rhai` | 用户自定义 Widget 沙箱 |
| 持久化 | `rusqlite` | 跨会话历史（Phase 2） |
| 通知 | `notify-rust` | OS 原生通知（Phase 2） |
| 配置 | `toml` | 用户配置文件解析 |

## 项目结构

```
claude-hud/
├── Cargo.toml
├── config.toml                    # 默认配置
├── DESIGN.md
├── src/
│   ├── main.rs                    # CLI 入口：render / dashboard / serve / setup
│   ├── compact.rs                 # 紧凑模式：stdin → widgets → ANSI
│   ├── dashboard.rs               # 仪表盘：ratatui 事件循环 + 网格布局
│   ├── serve.rs                   # Web 仪表盘（Phase 3）
│   ├── core/
│   │   ├── mod.rs
│   │   ├── session.rs             # SessionData + stdin JSON 反序列化
│   │   ├── transcript.rs          # Transcript JSONL 增量解析（Phase 2）
│   │   ├── widget.rs              # Widget trait + WidgetRegistry
│   │   ├── theme.rs               # 主题引擎 + 预制主题 + Token 管理
│   │   └── config.rs              # 配置加载（TOML）
│   ├── widgets/
│   │   ├── mod.rs                 # Widget 注册清单
│   │   ├── context_bar.rs         # 上下文进度条（True Color 渐变）
│   │   ├── model_display.rs       # 当前模型 + 扩展思考状态
│   │   ├── cost_display.rs        # 会话费用
│   │   ├── agent_overview.rs      # 代理总览（总数 + 进度条）
│   │   ├── agent_detail.rs        # 代理详情（名称/任务/耗时/卡顿标记）
│   │   ├── token_attribution.rs   # Token 归因排行（Phase 2）
│   │   ├── agent_timeline.rs      # 代理时间线（Phase 2）
│   │   ├── skills_mcp.rs          # Skills & MCP 状态
│   │   ├── skills_mcp_dynamic.rs  # Skills & MCP 动态追踪（Phase 2）
│   │   ├── git_status.rs          # Git 分支 + 变更
│   │   ├── rate_limits.rs         # 速率限制
│   │   ├── session_stats.rs       # 会话统计
│   │   ├── alerts.rs              # 智能预警汇总
│   │   └── script_widget.rs       # Rhai / Shell / HTTP 轮询（Phase 3）
│   └── probe/
│       ├── mod.rs
│       ├── git.rs                 # Git 状态探测
│       ├── system.rs              # 系统资源（CPU/内存）
│       └── filesystem.rs          # Skill/MCP 配置扫描
└── scripts/
    └── example.rhai               # 自定义 Widget 示例
```

## 数据来源

### 一、stdin JSON（Claude Code 状态行直接提供）

| 字段 | 描述 |
|------|------|
| `model.id` | 模型 ID，如 `claude-opus-4-7` |
| `model.display_name` | 显示名称，如 `Opus` |
| `context_window.used_percentage` | 上下文使用百分比 (0-100) |
| `context_window.total_input_tokens` | 输入 token 总数 |
| `context_window.total_output_tokens` | 输出 token 总数 |
| `context_window.context_window_size` | 上下文窗口总大小 |
| `context_window.current_usage` | 分项明细：input / output / cache_create / cache_read |
| `cost.total_cost_usd` | 本次会话费用（USD） |
| `cost.total_duration_ms` | 会话总耗时（毫秒） |
| `cost.total_lines_added` | 新增行数 |
| `cost.total_lines_removed` | 删除行数 |
| `rate_limits.five_hour.used_percentage` | 5 小时速率限制百分比 |
| `rate_limits.seven_day.used_percentage` | 7 天速率限制百分比 |
| `transcript_path` | 当前会话转录文件路径（.jsonl） |
| `subagentStatusLine` | 子代理独立状态行数据 |

### 二、Git（执行 git 命令获取）

| 字段 | 描述 |
|------|------|
| 当前分支名 | `git branch --show-current` |
| 变更文件数 | `git status --porcelain \| wc -l` |
| 领先/落后 | `git rev-list --count` |
| 最近 commit 摘要 | `git log -1 --oneline` |

### 三、静态文件扫描

| 字段 | 数据来源 |
|------|----------|
| 已配置 MCP 服务器 | 解析 `settings.json` / `.mcp.json` |
| 已加载 Skills | 扫描 `.claude/skills/` |
| 已启用 Hooks | 解析 `settings.json` |

### 四、Transcript JSONL（Phase 2）

| 字段 | 说明 |
|------|------|
| 工具调用（tool_use/tool_result） | 调用次数、耗时、类型分布 |
| 子代理（Task/Agent 事件） | 启停时间戳、模型、任务描述 |
| Todo 进度（TodoWrite 事件） | 完成/总计、当前任务标题 |
| Token 消耗（per-message） | token 归因到代理/工具 |

## Widget 系统

### 核心 trait

```rust
pub trait Widget {
    /// 唯一标识符，如 "context_bar"
    fn id(&self) -> &str;
    /// 用于配置文件中显示的名称
    fn display_name(&self) -> &str;
    /// 紧凑模式下渲染为单行字符串（含 ANSI）
    fn render_compact(
        &self,
        data: &SessionData,
        theme: &Theme,
        config: &WidgetConfig,
    ) -> String;
    /// 仪表盘模式下渲染为一个 ratatui 控件区域
    fn render_dashboard(
        &self,
        data: &SessionData,
        area: ratatui::layout::Rect,
        frame: &mut ratatui::Frame,
        theme: &Theme,
    );
    /// 仪表盘中占用的最小格子大小
    fn dashboard_size(&self) -> (u16, u16);
    /// 是否需要全屏刷新（动画类 widget 返回 true）
    fn needs_tick(&self) -> bool;
}
```

### Phase 1 Widgets（7 个，依赖 stdin JSON + 静态扫描）

| # | Widget | 紧凑模式 | 仪表盘 | 数据来源 |
|---|--------|----------|--------|----------|
| 1 | **model_display** | `[Opus 4.7]` | 模型名 + 提供商 + 扩展思考状态 | stdin `model.*` |
| 2 | **context_bar** | `ctx ████░░ 52%` | True Color 渐变进度条 + token 分项 + 消耗速率 | stdin `context_window.*` |
| 3 | **cost_display** | `¥1.42` | 费用 + 代码增删行数 | stdin `cost.*` |
| 4 | **agent_overview** | `⚡ 2/3 agents` | 总数 + 运行中/已完成 + 卡顿数 + 进度条 | stdin `subagentStatusLine` |
| 5 | **skills_mcp** | `🧩 2 skills 🔌 4 MCPs` | 已加载列表 + 静态配置统计 | 文件扫描 |
| 6 | **rate_limits** | `5h:34% 7d:12%` | 双进度条 + 重置倒计时 | stdin `rate_limits.*` |
| 7 | **git_status** | `main* ↑3` | 分支 + 脏状态 + 领先/落后 + 变更文件数 | `git` 命令 |

### Phase 2 Widgets（5 个，需要 Transcript JSONL 解析）

| # | Widget | 紧凑模式 | 仪表盘 | 解决痛点 |
|---|--------|----------|--------|----------|
| 8 | **agent_detail** | 每代理一行：名称/任务/耗时 | 完整列表 + 卡顿检测（>30s 无工具标红） | "卡在哪里" |
| 9 | **token_attribution** | Top-1 消耗代理名 | 按代理/工具的 token 排行 + 渐变条形图 | "谁在吃 token" |
| 10 | **agent_timeline** | — （仪表盘专属） | 启停时间线 + 并行度可视化 | "为什么这么慢" |
| 11 | **session_stats** | `⏱ 12m · 342 tok/s` | 耗时 + token 速率 + 工具调用次数 | 宏观性能 |
| 12 | **skills_mcp_dynamic** | ●/○ 激活标记 | 本会话调用统计 + 分布 | "哪些被用过" |

### Phase 2 附加功能

| # | 功能 | 描述 |
|---|------|------|
| 13 | **alerts** | 智能预警：上下文阈值变色 · 代理卡顿标记 · 费用超限提醒 · 压缩预测 |
| 14 | **history** | 跨会话 SQLite 持久化：每日费用趋势 · Token 用量折线 · 代理平均耗时 |
| 15 | **notifications** | OS 原生通知：上下文 95% · 代理完成 · 费用超 ¥10 · 速率限制 90% |

### Phase 3 Widgets（扩展生态）

| # | 功能 | 描述 |
|---|------|------|
| 16 | **script_widget** | Rhai 脚本 Widget · Shell 命令 Widget · HTTP 轮询 Widget |
| 17 | **web_dashboard** | `claude-hud serve` → http://localhost:9527 · 第二屏 Web 面板 |

### 紧凑模式 3 级预设

| 预设 | 行数 | 内容 |
|------|------|------|
| **Full**（默认） | 3 行 | 模型 + 上下文 + 代理 + Skills/MCP + 费用 + Git + 速率限制 |
| **Essential** | 2 行 | 模型 + 上下文 + 代理摘要 + 费用 |
| **Minimal** | 1 行（< 80 字符） | `[Opus] ctx 52% │ 2 agents │ ¥1.42` |

## 代理可观测性

5 个痛点 → 5 个功能模块：

| 痛点 | Widget | 数据来源 |
|------|--------|----------|
| "后台在干什么" | `agent_overview` + `agent_detail` | `subagentStatusLine` + Transcript |
| "整体进度" | `agent_overview` 进度条 | 启停计数 + TodoWrite 完成率 |
| "为什么这么慢" | `agent_timeline` | 代理启停时间戳 |
| "卡在哪里" | `agent_detail` 卡顿检测（>30s 无工具调用标红） | Transcript 时间戳 |
| "谁在吃 token" | `token_attribution` 排行 | Transcript per-message token |

### 紧凑模式代理摘要

```
[Opus 4.7] main*
ctx ████░░ 52% │ ⚡ 3 agents · 2 running · 1 done │ ¥1.42
◐ explore: Finding auth code (2m 15s) │ ◐ test-gen: Writing tests (30s) │ ✓ build-check (done)
```

### 仪表盘代理诊断面板

```
┌ Agent Overview ──────┬ Token Attribution ────┐
│ Total: 5             │ explore    12,450 42% │
│ Running: ●●● 3       │ test-gen    8,320 28% │
│ Done: ✓✓ 2           │ code-review 5,100 17% │
│ ⬤ Stalled: 1         │ build-check 2,300  8% │
│ Progress: ████ 40%    │ type-check  1,480  5% │
├ Agent Details ───────┴ Timeline ─────────────┤
│ ● test-gen [haiku] 32s                        │
│   ⚠ Stalled 45s no tool call                  │
│ ● explore [opus] 2m 15s                       │
│ ● code-review [haiku] 1m 05s                  │
│ ✓ build-check done · ✓ type-check done        │
└───────────────────────────────────────────────┘
```

## Skills & MCP 追踪

### 数据来源分层

| 层级 | 信息 | 数据来源 | Phase |
|------|------|----------|-------|
| 静态 | 已配置的 Skill/MCP 数量 + 名称 | 文件扫描 | P1 |
| 动态 | 本会话激活/调用的 Skill/MCP | Transcript 中 `Skill` 和 `mcp__*` 工具调用 | P2 |
| 实时 | 当前正在执行的 Skill/MCP | stdin JSON 工具活动数据 | P1 |

### 紧凑模式

```
🧩 brainstorming grill-me │ 🔌 codegraph spectra │ 5 skills · 4 MCPs
  ● active     ● active      ○ idle (atlassian)
```

### 仪表盘面板

```
┌ 🧩 Skills (5 loaded) ──┬ 🔌 MCP Servers (4) ────┐
│ ● brainstorming (3m)    │ ● codegraph (3 tools)   │
│ ● grill-me (12m)        │ ● spectra (2 tools)     │
│ ○ domain-modeling       │ ○ atlassian (5 tools)   │
│ ○ full-cycle-dev        │ ○ postgres (8 tools)    │
│ ○ grill-with-docs       │                         │
│ Active: 2 · Calls: 5    │ Active: 2 · Calls: 20   │
├ MCP Call Distribution ───────────────────────────┤
│ codegraph_explore ━━━━━━━━━━ 12 (60%)            │
│ spectra__impact   ━━━━ 5 (25%)                   │
│ spectra__context  ━ 3 (15%)                      │
└──────────────────────────────────────────────────┘
```

## 主题系统

### 可自定义 Token（20 个）

#### 颜色 Token（11 个）

| Token | 用途 |
|-------|------|
| `bg` | 背景色 |
| `fg` | 前景色 / 主文字 |
| `accent` | 强调色（模型名、选中态） |
| `success` | 成功 / 完成状态 |
| `warning` | 警告色（上下文 > 80%） |
| `danger` | 危险色（上下文 > 95%、卡顿） |
| `muted` | 次要文字 / 非活跃元素 |
| `border` | 边框 / 分隔线 |
| `skill_color` | Skills 图标色 |
| `mcp_color` | MCP 图标色 |
| `model_color` | 模型名称色 |

#### 样式 Token（9 个）

| Token | 类型 | 说明 |
|-------|------|------|
| `bar_filled` | char | 进度条填充字符（默认 `█`） |
| `bar_empty` | char | 进度条空白字符（默认 `░`） |
| `separator` | string | Widget 分隔符（默认 ` │ `） |
| `border_style` | enum | 边框样式：single / double / rounded / thick |
| `icon_set` | enum | 图标集：nerd / ascii / minimal |
| `bar_width` | number | 进度条宽度（字符数） |
| `padding` | number | 紧凑模式内边距 |
| `compact_lines` | number | 紧凑模式行数：1 / 2 / 3 |
| `dashboard_grid` | number | 仪表盘默认网格列数 |

### 图标集

| 图标集 | 风格 | 依赖 |
|--------|------|------|
| **nerd**（默认） | Nerd Fonts 图标 | 需安装 Nerd Font |
| **ascii** | `[*] [!] [?] [i] [~] [x] [v] [+]` 纯 ASCII | 零依赖 |
| **minimal** | `▸ ▹ ● ○ ◇ ◆ ■ □ ▰ ▱ ▲ ▽ ⬢ ⬡` Unicode 几何 | 零依赖 |

### 10 套预制主题

| 主题 | 底色 | 主色调 | 气质 |
|------|------|--------|------|
| **Dracula** | #282a36 | 紫 #bd93f9 / 绿 #50fa7b / 粉 #ff79c6 | 赛博朋克，鲜明大胆 |
| **Nord** | #2e3440 | 青 #88c0d0 / 绿 #a3be8c / 黄 #ebcb8b | 北欧冰川，冷静优雅 |
| **Tokyo Night** | #1a1b26 | 蓝 #7aa2f7 / 绿 #9ece6a / 金 #e0af68 | 现代都市夜，干净精致 |
| **Catppuccin** | #1e1e2e | 紫 #cba6f7 / 绿 #a6e3a1 / 黄 #f9e2af | 柔和暖色，护眼舒适 |
| **Monochrome** | #1a1a1a | 纯灰度 | 极简主义，零干扰 |
| **Solarized Dark** | #002b36 | 青 #2aa198 / 绿 #859900 / 黄 #b58900 | 经典终端，怀旧舒适 |
| **Gruvbox Dark** | #282828 | 黄 #fabd2f / 绿 #b8bb26 / 红 #fb4934 | 复古暖调，木工坊气质 |
| **One Dark** | #282c34 | 蓝 #61afef / 绿 #98c379 / 黄 #e5c07b | Atom 经典，高辨识 |
| **GitHub Dark** | #0d1117 | 蓝 #58a6ff / 绿 #3fb950 / 红 #f85149 | 代码平台默认，稳重可信 |
| **Palenight** | #292d3e | 蓝紫 #82aaff / 绿 #c3e88d / 黄 #ffcb6b | Material 深紫，柔和夜用 |

### 主题引用（三级配置深度）

```toml
# 级别 1：一行切换（字符串预设名，替换基底）
theme = "dracula"

# 级别 2：预设引用 + 微调
[theme]
preset = "dracula"

[theme.overrides]
accent = "#ff79c6"
bar_filled = "▓"

# 级别 3：部分/完整表（显式键逐键覆盖基底，未写出的键用基底值）
[theme]
accent = "#ff6b6b"
bar_width = 20
```

叠加顺序（自低到高）：基底（active_mod 的 mod preset，否则 config 的 preset，否则默认 nord）
→ config `[theme]` 显式键 → config `[theme.overrides]` → mod `[mod.theme.overrides]`（最高）。
config 的 `theme = "..."` 字符串在 active_mod 存在时不参与叠加（基底已由 mod 决定）。
坏 config 不再静默：stderr 打印 `[claude-hud] warning` 并回退默认，doctor `[!!]` 可查。

### 主题扩展能力

- **导出/导入**：`claude-hud theme export > my.toml` / `claude-hud theme import my.toml`
- **布局自由排列**：`compact_layout` 数组控制 Widget 顺序 + 每行内容
- **社区市场**：`claude-hud theme install user/repo`（类比 Oh My Zsh 主题生态）

## UI 预设

5 套完整的终端 UI 预设，每套包含：紧凑状态栏 + 仪表盘面板 + 配色 + 图标 + 动画风格。

### 预设总览

| # | 预设 | 底色 | 主色 | 气质 | 行数 | 图标 | 动画 |
|---|------|------|------|------|------|------|------|
| 1 | **Noir Minimalist** | #0d0d0d | 纯白/灰 | Dieter Rams 工业极简 | 1-2 | ASCII | 关闭 |
| 2 | **Obsidian Neon** | #0b0015 | 霓虹紫 | Dracula 进化版 | 2 | Nerd | 呼吸脉冲 |
| 3 | **Ember Warmth** | #0f0906 | 琥珀金 | 壁炉余烬，深夜护眼 | 2 | Unicode | 缓动 |
| 4 | **Glacier Steel** | #0c1116 | 冰蓝钢 | Nord 精密仪器 | 2 | Unicode | Braille 图 |
| 5 | **Matrix Terminal** | #000000 | 荧光绿 | 黑客终端，纯 ASCII | 1-2 | ASCII | 扫描线 |

### 1. Noir Minimalist

```toml
[theme]
bg = "#0d0d0d"
fg = "#f0f0f0"
accent = "#f0f0f0"
success = "#cccccc"
warning = "#999999"
danger = "#ffffff"
muted = "#555555"
border = "#1a1a1a"
icon_set = "ascii"
bar_filled = "█"
bar_empty = "░"
separator = " · "
compact_lines = 1
```

零装饰，纯信息。Dieter Rams 设计原则：每一个像素都要有存在的理由。大胆的留白是核心设计语言。

```
Opus 4.7 — ctx ████░░░░░░░░░░░░░░ 52% · 2 agents · ¥1.42 · main*
skills brainstorming grill-me — mcp codegraph spectra atlassian
```

### 2. Obsidian Neon

```toml
[theme]
bg = "#0b0015"
fg = "#e0d0f0"
accent = "#c084fc"
success = "#50fa7b"
warning = "#f1fa8c"
danger = "#ff79c6"
muted = "#6272a4"
border = "#1a1030"
skill_color = "#ff79c6"
mcp_color = "#f1fa8c"
model_color = "#bd93f9"
icon_set = "nerd"
bar_filled = "▓"
bar_empty = "░"
separator = " ┊ "
border_style = "rounded"
compact_lines = 2
```

Dracula 进化版：更深邃的暗色 + 更锐利的霓虹。Nerd Fonts 图标 + 告警呼吸脉冲动画（2s 周期）。

```
╭─ Opus 4.7 ─╮ ┊ ctx ▓▓▓▓▓▓▓░░░░░░░░░░░░░ 52%
⬢ 2/3 agents ┊ ¥1.42 ┊ 🧩 grill-me brainstorming ┊ ◆ codegraph spectra
```

### 3. Ember Warmth

```toml
[theme]
bg = "#0f0906"
fg = "#c0a890"
accent = "#e6a050"
success = "#f0c070"
warning = "#d08040"
danger = "#ff6030"
muted = "#705030"
border = "#1a0f08"
skill_color = "#d08040"
mcp_color = "#c07030"
model_color = "#e6a050"
icon_set = "minimal"
bar_filled = "▬"
bar_empty = "▬"
separator = " │ "
compact_lines = 2
```

壁炉余烬配色。暖琥珀 + 深棕底，长时间夜间编码不刺眼。像在黑暗的房间里看着炭火燃烧。

```
▎Opus 4.7 │ ctx ▬▬▬▬▬▬▬░░░░░░░░░░░ 52% │ ⚡ 2 agents │ ¥1.42
🧩 brainstorming grill-me │ ◇ codegraph spectra │ main* ↑3
```

### 4. Glacier Steel

```toml
[theme]
bg = "#0c1116"
fg = "#c0c8d0"
accent = "#88c0d0"
success = "#a3be8c"
warning = "#ebcb8b"
danger = "#d08770"
muted = "#5e7388"
border = "#15202b"
skill_color = "#7a93a8"
mcp_color = "#7a93a8"
model_color = "#88c0d0"
icon_set = "minimal"
bar_filled = "▰"
bar_empty = "▰"
separator = " ▪ "
compact_lines = 2
```

Nord 冰蓝钢：精密仪器的质感。Braille 趋势图 + 压缩预测，工具理性至上。

```
▸ Opus 4.7 ▪ context ▰▰▰▰▰▰▰░░░░░░░░░░░ 52% ▪ 2 agents ▪ ¥1.42
skills brainstorming grill-me ▪ mcp codegraph spectra ▪ ⏱ 12m
```

### 5. Matrix Terminal

```toml
[theme]
bg = "#000000"
fg = "#00aa00"
accent = "#00cc00"
success = "#00ff00"
warning = "#cccc00"
danger = "#ff0000"
muted = "#006600"
border = "#002200"
skill_color = "#008800"
mcp_color = "#008800"
model_color = "#00cc00"
icon_set = "ascii"
bar_filled = "█"
bar_empty = "░"
separator = "|"
border_style = "single"
compact_lines = 2
```

纯荧光绿 + 纯黑。CRT 扫描线纹理 + 荧光粉余辉效果。零 Unicode 依赖，任何终端都能完美呈现。

```
[OPUS_4.7]|CTX:████████░░░░░░░░░░░░52%|AGT:2/3|$1.42|main*
SKL:brainstorming,grill-me|MCP:codegraph,spectra|T:12m
```

## 布局系统

### 紧凑状态栏 — 6 种布局

| # | 布局 ID | 名称 | 行数 | 适合 |
|---|---------|------|------|------|
| A | `minimal` | Minimal 单行极简 | 1 (<80chars) | 窄终端、SSH |
| B | `activity` | Activity 双行聚焦 | 2 | **日常开发默认** |
| C | `agent-centric` | Agent-Centric 代理优先 | 3 | 重度使用子代理 |
| D | `full` | Full Expansion 三行全开 | 3 | 大显示器、信息至上 |
| E | `contextual` | Contextual 按需显隐 | 1-2 动态 | 安静时最小、活跃时扩展 |
| F | `kpi` | KPI Dashboard 指标仪表 | 2 | 数据驱动、性能调优 |

#### B. Activity 布局（推荐默认）

```
▸ Opus 4.7 ▪ ctx ▰▰▰▰▰▰░░░░░░░░ 52% ▪ 2 agents ▪ main* ↑3
skills brainstorming grill-me ▪ mcp codegraph spectra ▪ ¥1.42 ▪ 5h:34%
```

**第 1 行**：模型 + 上下文 + 代理摘要 + Git 分支
**第 2 行**：Skills/MCP 活动 + 费用 + 速率限制

#### E. Contextual 布局（动态行数）

```
-- 安静状态（无代理、无告警） --
[Opus] · ctx ▬▬▬▬░░░░░░ 52% · ¥1.42 · main*

-- 代理出现，自动展开第 2 行 --
[Opus] · ctx ▬▬▬▬░░░░░░ 52% · ⚡ 3 agents · ¥1.42
 ◐ explore... · ◐ test-gen... · ✓ build-check

-- 告警触发，自动变色 --
[Opus] · ctx ▬▬▬▬▬▬▬▬░░ 89% · ⚡ 3 agents · ¥1.42
 ⚠ context critical — compaction in ~5 min
```

### 全屏仪表盘 — 6 种布局

| # | 布局 ID | 名称 | 面板数 | 适合 |
|---|---------|------|--------|------|
| A | `grid-2x2` | 2×2 均衡网格 | 4 | **仪表盘默认** |
| B | `hex-2x3` | 2×3 六宫格 | 6 | 多指标同屏 |
| C | `sidebar` | 侧栏布局 (1:2) | 3 | 代理列表常驻 |
| D | `tabbed` | 标签页切换 | 1/页 | 小屏幕、单面板专注 |
| E | `focus` | 专注模式 | 1 全宽 | 深度排查单个问题 |
| F | `freeform` | 自由网格 (12列) | 自定义 | 高级用户自定布局 |

#### A. 2×2 Grid（推荐默认）

```
┌ Agent Overview ──────┬ Token Attribution ────┐
│ Total: 5  Running: 3 │ explore  ████████ 42%  │
│ Done: 2   Stalled: 1 │ test-gen ██████ 28%   │
│ Progress: ████ 40%    │ code-review ███ 17%   │
├ Skills & MCP ────────┴ Context + Alerts ──────┤
│ 🧩 2 active ○ 3 idle │ ctx 52%  ¥1.42  ⏱ 12m │
│ 🔌 2 active ○ 2 idle │ ⚠ 1 stalled · ~8m     │
└───────────────────────────────────────────────┘
```

#### C. Sidebar（代理优先）

```
┌ Agent List ──┬ Token Attribution ─────────────┐
│ ● explore    │ explore ████████ 12,450 (42%)  │
│ ● test-gen   │ test-gen ██████  8,320 (28%)  │
│   ⚠ stalled  │ code-review ████ 5,100 (17%)  │
│ ● code-review│                                │
│ ✓ build-check├ Skills & MCP + Context ────────┤
│ ✓ type-check │ 🧩 2 active 🔌 2 active        │
│              │ ctx 52% · ¥1.42 · ⏱ 12m        │
│ Progress 40% └────────────────────────────────┘
└──────────────┘
```

### Layout × Theme 搭配矩阵

| 场景 | Compact 布局 | Dashboard 布局 | Theme | 动画 |
|------|-------------|---------------|-------|------|
| 日常开发 | B. Activity (2行) | A. 2×2 Grid | Glacier Steel | Braille 图 |
| 重度代理 | C. Agent-Centric (3行) | C. Sidebar | Obsidian Neon | 呼吸+Glitch |
| 深夜编码 | F. KPI (2行指标) | A. 2×2 Grid | Ember Warmth | 数字缓动 |
| SSH / 远程 | B. Activity | E. Focus Mode | Matrix Terminal | 扫描线 |
| 炫技 / 截图 | C. Agent-Centric | A. 2×2 Grid | Obsidian Neon | 全开 |
| 笔记本小屏 | A. Minimal (1行) | D. Tabbed | Noir Minimalist | 关闭 |
| 性能诊断 | F. KPI (指标密集) | B. 2×3 Hex | Glacier Steel | Braille+频谱 |
| 自定义一切 | E. Contextual (动态) | F. Freeform (自由) | 任意 | 任意 |

### 6 种推荐搭配（出厂预设 Mod）

| # | Mod 名称 | 场景 | Compact | Dashboard | Theme |
|---|----------|------|---------|-----------|-------|
| 1 | Noir Precision | daily-dev | A. Minimal | A. 2×2 | Noir Minimalist |
| 2 | **Glacier Workstation** | daily-dev | B. Activity | A. 2×2 | Glacier Steel |
| 3 | Obsidian Command Center | heavy-agent | C. Agent-Centric | C. Sidebar | Obsidian Neon |
| 4 | Ember Night Shift | night-coding | F. KPI | A. 2×2 | Ember Warmth |
| 5 | Matrix Surveillance | ssh-remote | B. Activity | E. Focus | Matrix Terminal |
| 6 | Noir Tabbed | small-screen | E. Contextual | D. Tabbed | Noir Minimalist |

## Mod 管理与切换

### 磁盘存储

```
~/.claude/plugins/claude-hud/
├── mods/                         # 用户已安装的 Mod 包
│   ├── glacier-workstation.toml
│   ├── obsidian-command.toml
│   └── my-custom-mod.toml        # 用户自建
├── config.toml                   # 主配置（active_mod = "glacier-workstation"）
└── plugin.json
```

出厂预设编译在二进制 `src/presets/` 中，用户 Mod 存 `mods/*.toml`。

### Mod 文件格式

```toml
# glacier-workstation.toml

[mod]
name = "Glacier Workstation"
version = "1.0.0"
description = "日常开发默认搭配：冰蓝钢 + Activity双行 + 2×2仪表盘"
scene = "daily-dev"              # @daily 场景别名

[mod.layout]
compact = "activity"             # 引用内置布局 ID
dashboard = "grid-2x2"
compact_lines = 2

[mod.theme]
preset = "glacier-steel"         # 引用内置主题 ID

[mod.animation]
enabled = true
effects = ["gradient-bar", "panel-reveal", "braille-charts"]

[mod.widgets.context_bar]
bar_width = 18
gradient = true
```

### 配置引用方式

```toml
# 方式 1：引用 Mod（推荐）
active_mod = "glacier-workstation"

# 方式 2：引用 + 临时覆盖
active_mod = "glacier-workstation"

[runtime_overrides]
compact_lines = 1                # 今天想用单行

# 方式 3：不用 Mod，全部内联（老鸟模式）
# active_mod 留空，直接写 theme + layout + widgets
```

### CLI 命令

| 命令 | 说明 |
|------|------|
| `claude-hud render` | 紧凑模式：stdin → ANSI |
| `claude-hud dashboard` | 全屏 TUI 仪表盘 |
| `claude-hud serve` | Web 仪表盘（Phase 3） |
| `claude-hud setup` | 自动配置 Claude Code settings.json |
| `claude-hud mod list` | 列出所有已安装 Mod + 激活状态 |
| `claude-hud mod use <name>` | 切换 Mod（即时生效） |
| `claude-hud mod use -` | 快速回切到上一个 Mod |
| `claude-hud mod use @scene` | 按场景别名切换（@daily/@night/@agent/@ssh） |
| `claude-hud mod preview <name>` | 预览 Mod（不实际切换） |
| `claude-hud mod current` | 显示当前激活的 Mod 详情 |
| `claude-hud mod save <name>` | 将当前配置保存为新 Mod |
| `claude-hud mod pick` | 交互选择器：方向键选 + 右侧实时预览 |
| `claude-hud mod export <name>` | 导出 Mod 为 .toml（分享用） |
| `claude-hud mod import <file>` | 导入 .toml 到本地 Mod 库 |
| `claude-hud mod delete <name>` | 删除用户 Mod（出厂预设不可删） |
| `claude-hud mod reset` | 恢复出厂默认 Mod |
| `claude-hud theme export` | 导出主题 |
| `claude-hud theme import <file>` | 导入主题 |
| `claude-hud completion <shell>` | 生成 Shell 补全脚本 |
| `claude-hud widget list` | 列出可用 Widget |
| `claude-hud widget test <name>` | 测试单个 Widget |

### 5 层快捷切换

| 方式 | 命令 | 击键 | 适用场景 |
|------|------|------|----------|
| 模糊匹配 | `claude-hud mod use gl` | 2-3 键 | 记得名字前几个字母 |
| Tab 补全 | `claude-hud mod use g<TAB>` | 1 键 + TAB | 不确定完整名称 |
| 快速回切 | `claude-hud mod use -` | 1 键 | 试效果来回对比 |
| 场景别名 | `claude-hud mod use @night` | 6 键 | 按用途语义切换 |
| 交互选择器 | `claude-hud mod pick` | 方向键选 | 不记得名字、看预览再定 |

### 用户自定义 Mod 生命周期

```
1. 选择基础 Mod  →  claude-hud mod use glacier-workstation
2. 微调配置      →  编辑 config.toml [runtime_overrides]
3. 预览效果      →  claude-hud render --sample
4. 保存为新 Mod  →  claude-hud mod save my-custom
5. 导出分享      →  claude-hud mod export my-custom > ~/my-mod.toml
```

## 高级渲染与动画

ratatui 帧循环（500ms tick）+ True Color + Unicode 可实现 15 种终端效果。

### 效果总览

| # | 效果 | 技术 | 难度 | 视觉 | Phase |
|---|------|------|------|------|-------|
| 1 | True Color 渐变进度条 | 每字符独立 RGB 线性插值 | ★☆☆ | ★★★★ | P1 |
| 2 | 霓虹发光 (呼吸脉冲) | RGB 亮度 sin 波调制，2s 周期 | ★★☆ | ★★★★★ | P1 |
| 3 | 伪 3D 面板 (半块阴影) | ░ + 3 层颜色偏移 → 面板浮起感 | ★☆☆ | ★★★★ | P1 |
| 4 | 电影式面板揭示 | 面板交错淡入，每面板 +150ms 延迟 | ★★☆ | ★★★★ | P1 |
| 5 | CRT 扫描线 + 余辉 | 交替行色差 + 移动 4px 横条 + 色衰 | ★☆☆ | ★★★ | P1 |
| 6 | 数字缓动 | 计数器从旧值平滑滚动到新值 | ★★☆ | ★★★ | P2 |
| 7 | 火花拖尾 | 值变化时发射衰减粒子 (·•◦°) | ★★☆ | ★★★ | P2 |
| 8 | Braille 频谱图 | ⣾⣷⣯ 点阵 = 8 级高度微型柱状图 | ★★☆ | ★★★★★ | P2 |
| 9 | Braille 热力图 | ⠀⣀⣤⣶⣿ 5 级密度 + True Color 热力色标 | ★★☆ | ★★★★★ | P2 |
| 10 | 波浪变形 | sin 函数驱动 ▁▂▃ 8 级字符 Y 偏移 | ★★★ | ★★★★ | P2 |
| 11 | 液态填充 | 进度条顶部波形 ▄▀█ + sin 偏移 | ★★★ | ★★★★★ | P2 |
| 12 | Glitch 故障效果 | 随机替换字符为 ▯▮▰▪ + 偏移，600ms 恢复 | ★★☆ | ★★★ | P2 |
| 13 | 理发店旋转条纹 | ░▒▓█ 交替 + 每帧左移一位 | ★★☆ | ★★★ | P2 |
| 14 | 跑马灯滚动 | 超长文本每 200ms 左移一字，循环滚动 | ★★☆ | ★★★ | P2 |
| 15 | RGB 光谱循环 | HSL 色相每帧递增 1° → 全色相旋转 | ★★☆ | ★★★★ | P2 |

### 各效果详细说明

#### 1. True Color 渐变进度条

每个字符独立 RGB 值，从绿色 (0,255,0) 经黄色 (255,255,0) 平滑过渡到红色 (255,0,0)。

```
████████████████████
↑ RGB (0,255,0) → (64,240,0) → (128,220,0) → ... → (255,0,0)
```

用于 context_bar Widget，直观展示上下文压力。

#### 2. 霓虹发光（呼吸脉冲）

卡顿/告警标记的前景色亮度以 sin 波调制：

```
帧 0:  ⚠ STALLED  (RGB 255,120,115 — 满亮度)
帧 5:  ⚠ STALLED  (RGB 180,85,80  — 70% 亮度)
帧 10: ⚠ STALLED  (RGB 120,55,50  — 50% 亮度)
帧 15: ⚠ STALLED  (RGB 180,85,80  — 回升)
帧 20: ⚠ STALLED  (RGB 255,120,115 — 回到满亮度)
```

2 秒完整周期。边框颜色同步跟随呼吸。非告警状态无此效果。

#### 3. 伪 3D 面板

三层半块字符 (░ U+2591) 在面板右下偏移叠加：

```
┌────────────────┐
│ Panel Content  │░░     ← L1: #060608
│                │░░     ← L2: #08080a
└────────────────┘░░     ← L3: #0a0a0c
```

面板像浮在背景上方 6px。仪表盘中选中/高亮的面板阴影更深。

#### 4. 电影式面板揭示

仪表盘启动时，面板依次淡入：

```
t=0ms:    ┌ Agent Overview ────┐  (opacity: 0 → 1, 300ms)
t=150ms:  ├ Token Attribution ─┤  (opacity: 0 → 1, 300ms)
t=300ms:  ├ Skills & MCP ──────┤  (opacity: 0 → 1, 300ms)
t=450ms:  └ Context Trend ────┘  (opacity: 0 → 1, 300ms)
t=600ms:  All panels ready.       (final status line)
```

#### 5. CRT 扫描线 + 荧光粉余辉

- 每隔一行 bg 加深 1 位（#000000 → #000100），肉眼看是细微栅线
- 4px 高半透明横条每 3 秒从顶移到底
- 字符颜色用指数衰减模拟荧光粉余辉（Matrix 主题专属）

#### 6. 数字缓动

计数器值变化时，不是直接跳变，而是每帧逼近：

```
旧值: ¥1.20
帧 1: ¥1.25
帧 2: ¥1.31
帧 3: ¥1.38
帧 4: ¥1.42  ← 到达新值
```

#### 7. 火花拖尾

数值变化时右侧发射 3-5 个粒子，每帧右移 + 衰减：

```
¥1.42 ·•◦°      ← 4 个粒子，渐行渐远
¥1.42  ·•◦°     ← 右移 1 位
¥1.42   ·•◦    ← 衰减 + 右移
¥1.42          ← 500ms 后消散完毕
```

#### 8. Braille 频谱图

Braille 点阵字符上下组合提供 8 级高度，用于微型柱状图：

```
⣿⣷⣯⣟⡿⢿⣻⣽⣾⣷⣯⣟⡿⢿⣻⣽⣾⣷⣯⣟
↑ 每分钟 token 消耗柱状图，小空间内展现数据趋势
```

#### 9. Braille 热力图

用 5 级密度字符 (⠀⣀⣤⣶⣿) + 热力色标（蓝→绿→黄→红）：

```
⠀⣀⣤⣶⣿⣿⣶⣤⣀⠀     ← 冷蓝到热红渐变，展示代理活跃度分区
⣀⣤⣶⣿⣿⣿⣿⣶⣤⣀     每格 = 1 分钟，颜色密度 = 该分钟代理活跃度
⣤⣶⣿⣿⣿⣿⣿⣿⣶⣤
```

#### 10. 波浪变形

Context 突变时触发：12 帧 Unicode block 字符 (▁▂▃▄▅▆▇█) 逐列 sin 偏移：

```
▁▂▃▄▅▆▇█▇▆▅▄▃▂▁▂▃▄▅▆▇█▇▆▅▄▃▂▁
← 数据变化时触发正弦波，2 秒后恢复直线
```

#### 11. 液态填充

进度条顶部一行用 ▄▀█ 字符 + sin 偏移模拟液面晃动：

```
▄▄▀█▄▄▀█▄▄▀█▄▄     ← 液面有微小波浪
████████████████     ← 固体填充
```

仅对"进行中"的进度条启用，静态时无波浪。

#### 12. Glitch 故障效果

卡顿警告触发时，对应文字行短暂"故障"：

```
正常: test-gen agent stalled 45s
帧 1: t▯st-gen agent stalled 45s    ← 随机字符替换
帧 2: t▯st-gen ag▪nt stalled 45s    ← 第二个替换
帧 3: test-gen agent st▰lled 45s    ← 继续偏移
帧 4: test-gen agent stalled 45s    ← 600ms 后恢复
```

#### 13. 理发店旋转条纹

活跃进度条用交替密度字符产生滚动效果：

```
帧 0: ▓█▓█▓█▓█▓
帧 1: █▓█▓█▓█▓█     ← 每帧左移一位
帧 2: ▓█▓█▓█▓█▓
```

仅对"正在进行中"的进度条启用。

#### 14. 跑马灯滚动

超长代理任务描述自动滚动：

```
帧    显示内容
0:    Finding authentication patterns across 14 service files...
1:    inding authentication patterns across 14 service files in...
2:    nding authentication patterns across 14 service files in s...
...
```

每 200ms 左移 1 字符位，到头停顿 1s 再循环。

#### 15. RGB 光谱循环

进度条填充色持续旋转 HSL 色相：

```
帧 0:    ████████  (HSL 0°,  红)
帧 20:   ████████  (HSL 40°, 橙)
帧 40:   ████████  (HSL 80°, 黄)
帧 60:   ████████  (HSL 120°,绿)
帧 80:   ████████  (HSL 160°,青)
...
帧 360:  ████████  (HSL 360°,回到红)
```

每帧递增色相 1°。用于 "RGB Gamer" 主题，纯炫技。

### 动画调度

```
ratatui 事件循环 (500ms tick)
├── needs_tick() == true 的 Widget 每 tick 更新一帧
├── 帧计数器 widget_frame_n 驱动所有动画周期
├── 非动画 Widget 只在数据变化时重绘（脏标记）
└── 全局动画开关：主题中 animate = true/false
```

## 配置系统

### 完整 config.toml

```toml
# === 全局设置 ===
mode = "compact"                    # compact | dashboard
preset = "full"                     # full | essential | minimal
theme = "dracula"                   # dracula | nord | tokyo-night | catppuccin | monochrome | solarized-dark（自定义见「主题引用」三级别）

# === 紧凑模式布局 ===
compact_layout = [
    "model_display",
    "context_bar",
    "agent_overview",
    "cost_display",
    "skills_mcp",
    "alerts",
]

separator = " │ "

# === 仪表盘设置 ===
[dashboard]
refresh_interval_ms = 500
default_layout = "grid"             # grid | columns | focus

# === Widget 级别配置 ===
[widgets.context_bar]
show_percentage = true
show_tokens = true
bar_width = 20
warn_threshold = 80
critical_threshold = 95

[widgets.model_display]
show_extended_thinking = true
show_provider = false

[widgets.agent_overview]
show_stalled = true
stall_threshold_sec = 30
max_visible = 5

[widgets.alerts]
context_warn = 80
context_critical = 95
cost_warn_usd = 10.0
rate_limit_warn = 90
show_compaction_prediction = true

[widgets.cost_display]
currency_symbol = "¥"

[widgets.skills_mcp]
show_idle = true
max_visible_mcps = 4

# === 自定义扩展 Widget（Phase 3） ===
[widgets.ci_status]
type = "shell_output"
command = "curl -s https://ci.example.com/latest | jq '.status'"
refresh_seconds = 30

[widgets.my_custom]
type = "rhai_script"
script_path = "~/.claude/plugins/claude-hud/scripts/my_widget.rhai"
```

### Claude Code 集成

```json
{
  "statusLine": {
    "type": "command",
    "command": "claude-hud render",
    "refreshInterval": 5
  }
}
```

一键配置：`claude-hud setup` 自动写入 `~/.claude/settings.json`。

## 跨会话历史（Phase 2）

SQLite 持久化（`~/.claude/plugins/claude-hud/history.db`），记录每次会话的关键指标。

### 仪表盘历史面板

- 每日费用趋势（sparkline）
- Token 用量折线
- 代理平均耗时变化
- 使用高峰时段分析
- "本周总费用 ¥342，比上周增长 40%"
- "平均每天用 3.2M tokens"

## 智能预警（Phase 2）

| 触发条件 | 行为 |
|----------|------|
| 上下文 > 80% | 进度条变黄 |
| 上下文 > 95% | 进度条变红 + OS 通知 |
| 代理 > 30s 无工具调用 | 红色 ⚠ 卡顿标记 |
| 速率限制 > 90% | 红色警告 |
| 费用超过阈值 | 费用闪烁提醒 |

### 预测性建议

- "按当前消耗速度，预计 8 分钟后压缩"（基于 token 消耗速率线性外推）
- "当前 5 个代理并发，token 消耗是平时的 3 倍"

## 分阶段实施计划

### Phase 1 — 核心骨架（2-3 周）

**目标**：可用的紧凑状态栏

- Cargo.toml + 全部依赖
- SessionData 结构体 + stdin JSON 反序列化
- Widget trait + WidgetRegistry
- 配置系统（TOML 加载）
- 主题引擎（20 个 Token + 6 套预制 + 3 套图标）
- 7 个 P1 Widget 全部实现
- 紧凑模式渲染引擎 + 3 级预设（Full / Essential / Minimal）
- **5 种 P1 动画**：True Color 渐变 + 霓虹呼吸 + 伪 3D 面板 + 电影式揭示 + CRT 扫描线
- `claude-hud setup` 自动配置
- 仪表盘骨架（ratatui 空白面板 + 事件循环 + 动画调度系统）

**产出**：`cargo install` 可用，作为 Claude Code 状态栏运行

### Phase 2 — 深度诊断（2 周）

**目标**：代理可观测性 + 跨会话历史

- Transcript JSONL 增量解析引擎
- 5 个 P2 Widget + alerts + history + notifications
- SQLite 持久化 + 历史趋势
- 仪表盘完整功能（所有面板对位）
- 智能预警系统
- **10 种 P2 动画**：数字缓动 + 火花拖尾 + Braille 频谱 + 热力图 + 波浪变形 + 液态填充 + Glitch 故障 + 理发店条纹 + 跑马灯 + RGB 光谱循环

**产出**：仪表盘可替代紧凑模式做主要交互

### Phase 3 — 扩展生态（2 周）

**目标**：可编程扩展 + 第二屏

- Rhai 脚本引擎 + script_widget
- Shell 命令 Widget + HTTP 轮询 Widget
- Web 仪表盘（HTTP 服务器）
- Widget 市场 + 主题市场机制

**产出**：社区可贡献自定义 Widget 和主题

### Phase 4 — 分发与生态

- GitHub Release CI（Win/Mac/Linux 二进制）
- Homebrew + cargo install
- Claude Code 插件市场发布（plugin.json）
- 多会话监控

### Phase 5 — 持续迭代

- 国际化
- 更多预制主题
- 性能优化（大型 transcript 处理）
