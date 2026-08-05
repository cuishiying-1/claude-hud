# Claude HUD — 部署与使用文档

## 项目概述

Claude HUD 是 Claude Code 的双模终端可视化插件：紧凑状态栏（日常使用）+ 全屏仪表盘（深度诊断）。

**技术栈**：Rust 2021 · ratatui · crossterm · serde · clap · rusqlite · rhai · notify-rust

## 快速开始

### 一键安装（推荐，无需 Rust）

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/cuishiying-1/claude-hud/master/scripts/install.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/cuishiying-1/claude-hud/master/scripts/install.ps1 | iex
```

安装器自动完成：下载预编译二进制 → 加入 PATH → 运行 `claude-hud setup`（合并 statusLine 到 `~/.claude/settings.json`）。输出三态：`installing` / `up to date`（幂等跳过）/ `upgrading`。

> **尚无 release**：仓库已建立但未发布首个 release 时，安装脚本报 `cannot resolve latest release`（无网络时同理）。首个 release 创建前请使用 `cargo build --release` 本地构建。

重启 Claude Code 或执行 `/reload-plugins`，状态栏底部应出现 HUD 显示。

> `setup` 在已存在 statusLine 或 settings.json 损坏时，每次运行都会生成新的时间戳备份 `settings.json.hud.bak-<epoch>`（永不自动删除，可用于回溯）。

### 一键卸载

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/cuishiying-1/claude-hud/master/scripts/uninstall.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/cuishiying-1/claude-hud/master/scripts/uninstall.ps1 | iex
```

### 从源码构建（开发者）

```bash
cd claude-hud
cargo build --release
# 二进制位置：target/release/claude-hud
cargo install --path .   # 可选：安装到 PATH
```

### 自检

```bash
claude-hud doctor
```

检查 PATH、config.toml、statusLine 配置、图标集决议、git 可用性与样例渲染，输出 `[ok]`/`[!!]` 健康报告。

## CLI 命令参考

### 基础命令

| 命令 | 说明 |
|------|------|
| `claude-hud render` | 紧凑模式：stdin → ANSI 状态栏（Claude Code 自动调用） |
| `claude-hud dashboard` | 全屏 TUI 仪表盘（`q`/`Esc` 退出） |
| `claude-hud serve` | Web 仪表盘（`http://localhost:9527`） |
| `claude-hud setup` | 一键配置 Claude Code |
| `claude-hud doctor` | 自检：配置/状态行/图标/git/渲染健康报告（含 `update:` 信息项） |
| `claude-hud uninstall` | 移除 statusLine 与配置目录（卸载脚本内部调用） |
| `claude-hud history` | 跨会话历史：Weekly stats / Recent sessions / Daily cost（空库显示 `—`） |
| `claude-hud history --weekly` | 周报五指标：会话数/成本合计/token 总量/最长时长/最高单会话（空库 `—`；成本带 `≈`） |
| `claude-hud update check` | 检查新版本（仓库无 release 或离线时输出 `not published yet` / `update check unavailable`） |

### Mod 管理

| 命令 | 说明 |
|------|------|
| `claude-hud mod list` | 列出所有已安装 Mod |
| `claude-hud mod use <name>` | 切换 Mod（先校验存在性；`-` 返回上一 Mod；`@scene` 按场景别名解析） |
| `claude-hud mod preview <name>` | 预览 Mod 效果 |
| `claude-hud mod current` | 显示当前激活的 Mod |
| `claude-hud mod save <name>` | 保存当前配置真实快照（合并后的主题 + compact_widgets + widgets 段） |
| `claude-hud mod pick` | 序号选择器：列出内置 + 用户 Mod，输入序号切换 |
| `claude-hud mod export <name>` | 导出 Mod 为 .toml |
| `claude-hud mod import <file>` | 导入 .toml 到本地库 |
| `claude-hud mod install <user/repo>` | 从 GitHub 仓库 mods/ 目录批量安装（⑰ v0.6） |
| `claude-hud mod delete <name>` | 删除用户 Mod |
| `claude-hud mod reset` | 恢复出厂默认（Glacier Workstation） |

#### `mod install`（⑰ v0.6）

```bash
claude-hud mod install user/repo
```

- 列出 GitHub 仓库 `mods/` 目录（contents API）并拉取全部 `.toml`，**两阶段批处理**：先全部拉取校验，再统一落盘报告。
- `mod_info.name` 落盘前安全校验：非空、≤64 字符、仅 `[A-Za-z0-9._-]`、不与内置 Mod 重名。
- 含 rhai/shell/http 脚本组件的 Mod 会先打印**供应链警告**，确认来源仓库可信后再使用。
- 安装成功后自动激活字典序最大的 Mod；重复安装同名 = 更新；单条失败跳过继续，全部失败 exit 1。
- 示例仓库结构：`mods/foo.toml`、`mods/bar.toml`（每个文件是完整 Mod 配置，含 `[mod_info]` 表）。

### 主题和 Widget

| 命令 | 说明 |
|------|------|
| `claude-hud theme export` | 导出当前主题 |
| `claude-hud theme import <file>` | 导入主题到 config.toml `[theme]` 段（保留其他段） |
| `claude-hud widget list` | 列出可用 Widget |
| `claude-hud widget test <name>` | 测试单个 Widget |

### 历史查询

`claude-hud history` 读取 SQLite 历史库（`~/.claude/plugins/claude-hud/history.db`），输出三块统计：

```text
Weekly stats:
  Cost: $12.34 | Sessions: 18 | Tokens: 450k | Avg duration: 6.2m | Avg agents: 1.4
Recent sessions:
  #42  2026-08-03T09:12:00  $0.42  6m  2 agents  12k tok
  #41  2026-08-03T08:40:11  $1.05  12m  3 agents  45k tok
Daily cost (last 7 days):
  2026-08-03  $0.42
  2026-08-02  $1.05
```

空库（尚无任何记录）时各块显示 `—`，不显示 0：

```text
Weekly stats:
  Cost: — | Sessions: — | Tokens: — | Avg duration: — | Avg agents: —
Recent sessions:
  —
Daily cost (last 7 days):
  —
```

记录时机：仪表盘 `q`/`Esc` 退出，以及紧凑模式 render 检测到 `transcript_path` 切换（上一会话结束）时自动写入。

### 会话浏览（⑤⑥⑦，v0.6）

```bash
claude-hud sessions                 # 分页列表（默认 10 条，id 降序）
claude-hud sessions --limit 20 --offset 10   # 分页
claude-hud sessions --date 2026-08-01        # 仅此日期之后开始的会话
claude-hud session 42                        # 单会话详情
```

- `session <id>` 详情：模型 / 成本 / 时长 / 代理数 / token 总量；`transcript_path` 存在时尾读 transcript 补充输入输出 token 分解与代理明细；未找到 id → stderr 报错 + exit 1。
- **工具成本排行（估算口径）**：无逐工具 token 数据 → per_call 均摊估算（总成本 ÷ 总调用数 × 各工具调用数），行首带 `≈` 标注；模型未命中内置/用户 `[pricing]` 或零调用/零 token 时显示 `—`（诚实降级）。
- 历史库自 0.6 起新增 `model` / `transcript_path` 两列（旧库首次打开自动 ALTER TABLE 补齐，无需迁移操作）。

### 历史趋势与 Web 升级（⑪⑫⑬⑭，v0.6）

- **TUI 历史趋势面板（⑪）**：dashboard 新 widget `tui_trend`（近 7 天成本柱状，`█` 字符 + 日期标签）。四种布局均通过 `compact_layout` 配置容纳（grid-2x2 / sidebar / focus / tabbed），例如 `compact_layout = ["model_display", "context_bar", "tui_trend", ...]`；历史库不可用或空时显示 `—`（诚实降级）。dashboard 在非 TTY（管道重定向）下渲染单帧后直接退出，不进入 raw mode。
- **Web SVG 成本趋势图（⑫）**：`serve` 仪表盘 "Weekly cost trend" 卡片为服务端渲染 SVG（零依赖，不引图表库）；数据点 < 2 时显示占位提示而非空图。
- **Web 会话列表与成本明细（⑬）**：`/api/sessions?limit=&offset=` 返回分页会话列表（复用 `sessions` 的查询口径）；`/api/sessions/{id}` 返回单会话明细（模型/成本/时长/代理/token 分解 + transcript 尾读工具明细）。前端 "Sessions" 表格行点击展开详情，加载更多分页。
- **周环比（⑭）**：This Week 卡片显示本周 vs 上周的成本 / 会话数 / token 变化百分比（`+12%` / `−8%` / `—`）。周键口径 `%Y-%W`（ISO 周），上周 = `now - 7 days` 所在周（跨年安全）；无上周数据时对比行显示 `—`。
- 黑盒计数：191 例。

### 状态栏成本双轨（⑲）

状态栏成本有两条计算路径：

- **命中 `[pricing]`（内置表或用户表）**：按 stdin 会话累计 token（input/output/cache_read/cache_creation）× 单价重算，并带 `≈` 前缀。内置价格表覆盖 9 个主流模型（2026-07 官方价目，cache_read = 0.1×input、cache_creation = 1.25×input），用户 `[pricing]` 可覆盖任意模型——cache 权重为估算（缓存字段缺失时为 0 按旧公式计）；混合模型会话重算不准确，建议固定模型或依赖透传。模型 ID 以 stdin 的 `model.id` 为准（`claude-hud render --dump` 可查）。
- **未命中（内置表与用户表均无）**：透传 Claude Code 官方 `total_cost_usd`（含 cache，准确，无 `≈`）。Web 仪表盘与 dashboard 完整数据视图会显示"当前模型未配置单价"提示。

零数据降级：网关无 usage/cost 时成本组显示 `—`（不显示 `$0.00` 假精确）。

### 预算告警（⑳）

- 预算基于 `≈` 估算值触发（与状态栏同一实时路径）；档位单调递进（每档一次）+ 10 分钟冷却（复用 `[alerts].cooldown_minutes`）跨进程去重。
- 判定发生在 render 进程（每 5s 管线），**不开 dashboard 也能收到预警**。
- dashboard 不接预算：其 transcript 精确成本与预算的 `≈` 实时语义冲突。
- 与 `[alerts].cost_threshold_usd` 并存互不干扰，先到者先发。
- **状态栏占比（v0.3）**：配置了 `cap_usd` 且成本 > 0 时，cost_display 组尾追加 ` · NN%`（实时成本 ÷ cap）；`cap_usd = 0`（默认关闭）时占比隐藏。
- **成本速率（v0.6）**：有成本且活跃时长 > 0 时，cost_display 组尾追加 ` · ≈$X.X/h`（成本 ÷ 小时化时长）；零时长/零成本不显示（诚实降级）。

### 压缩预测（④）

- **状态栏标注**：context_bar 组尾 `compact ≈Nm`（transcript 首尾桶 token 增量 ÷ 时间 → 速率，线性外推剩余窗口分钟数）；时间轴不可靠 / 桶 < 2 / 速率为 0 时不显示（诚实降级）。
- **临近通知**：`[alerts].compaction_eta_minutes`（默认 15，0 = 关闭）——预测剩余 ≤ 阈值时发桌面通知；复用 `[alerts].cooldown_minutes` 跨进程去重（判定在 render 进程，不开 dashboard 也能收到）。

### 国际化（v0.5）

`language = "en" | "zh"` 键切换界面语言（未知值回退 en 并在 stderr 警告）。覆盖三面：**运行时输出**（render/doctor/history/mod/update 等）、**clap 帮助**（`--help` 后处理注入）、**Web 仪表盘**（HTML 标记替换 + JS 翻译表）。字符串表内嵌于 `locales/en.toml`（全量基准）与 `locales/zh.toml`（en 子集），回退链：当前语言 → en → key 本身。不翻译项：单位（`%`/`$`/`m`/`s`）、图标、健康检查协议体（`OK`）、console 内部日志。widget 显示名走 `widget.<id>` key，缺省回退英文稳定 id。

### 状态栏宽度

紧凑模式输出做**宽度感知**：以 `COLUMNS` 环境变量为宽度源（statusLine 场景下终端不会真正 resize，环境变量是唯一可靠信号）。超出可用宽度时从行尾整组丢弃直至适配；单字段超过 24 字符（model 名 / git 分支 / 代理名）截断并加 `…`。`COLUMNS` 缺失或非法时默认 80 列，最小钳制 40 列。

> **v0.4 视觉增强**：context_bar 进度条默认逐 cell truecolor 渐变（success→danger；`gradient = "false"` 回退 3 档变色）；token_rate 显示 `tok 3.1k/min` 速率文本（transcript 尾桶增量口径，无数据时 `—`）；alerts 临界与 agent_detail 卡顿标记使用 4s 呼吸动画（`CLAUDE_HUD_PHASE` env 可固定相位用于测试）。逐 cell ANSI 色码不影响宽度口径（fit_line 先剥 ANSI 再测宽）。
>
> **v0.6 卡顿归因（⑮）**：agent_detail 卡顿标记扩展为 `stalled 3m15s · bash`（en）/ `卡顿 3m15s · bash`（zh）——时长（闲置秒数）+ 最后工具名归因，danger 色；alerts 卡顿行同步升级。无工具记录（旧会话/无 ToolUse）维持原 elapsed 显示。数据来自 transcript 工具归属（ToolUse 落点同步记 `last_tool_name`）；时间轴不可靠不显示（不假告警）。

```bash
COLUMNS=100 claude-hud render < data.json   # 指定宽度
```

Web 仪表盘与 dashboard TUI 不受影响（各自独立布局）。

`claude-hud completion <shell>` 基于 clap_complete 生成真实补全脚本，支持 `bash` / `zsh` / `fish` / `powershell`（不支持的 shell 名直接报错，exit 1）：

```bash
claude-hud completion bash        # 追加到 ~/.bashrc
claude-hud completion zsh         # 追加到 ~/.zshrc（或 compinit 目录）
claude-hud completion fish        # 写入 ~/.config/fish/completions/claude-hud.fish
claude-hud completion powershell  # 追加到 $PROFILE
```

## 配置文件

> **配置全局生效于所有会话窗口**（settings.json 中的 statusLine 同为全局配置）；**数据层面**（session/git 探测、状态文件、历史记录）各窗口独立。
> 写配置的命令（`mod use` / `mod reset` / `mod save` / `mod delete` / `mod import` / `theme import`）会输出 `(applies to all windows)` 提示。
> `icon_set` 默认 `auto`，无 Nerd Font 时自动降级为 minimal 图标，无需手动配置。

位置：`~/.claude/plugins/claude-hud/config.toml`

```toml
# 界面语言（en | zh；未知值回退 en）
language = "en"

# 激活的 Mod
active_mod = "glacier-workstation"

# 紧凑模式 Widget 顺序
compact_layout = [
    "model_display",
    "context_bar",
    "agent_overview",
    "cost_display",
    "skills_mcp",
    "token_rate",
    "alerts",
]

separator = " │ "

[dashboard]
refresh_interval_ms = 500
default_layout = "grid-2x2"    # grid-2x2 | sidebar | focus | tabbed（仪表盘内按 `l` 循环切换并持久化到此键）
scanlines = true               # CRT 扫描线背景（每 4 行一条 dim 行 + 行进扫描带）

# Widget 级配置
[widgets.context_bar]
bar_width = "18"
gradient = "true"   # 进度条逐 cell truecolor 渐变（success→danger，默认开；false 回退 3 档变色）
warn_threshold = "80"
critical_threshold = "95"

[widgets.cost_display]
currency_symbol = "¥"
warn_threshold_usd = "10.0"
show_tokens = "false"        # token 数（Xk in / Xk out tok）展示开关：显式设置时优先；
                             # 未设置时布局含 context_bar 自动隐藏（去重），
                             # 无 context_bar 的极简布局默认显示

[widgets.agent_overview]
stall_threshold_sec = "30"

# 预算告警（render 进程判定，不开 dashboard 也能收到）
[budget]
cap_usd = 5.0              # 会话成本上限（0 = 关闭预算，默认关闭）
warn_pcts = [50, 80, 100]  # 达到这些百分比时通知，每档一次（单调递进）
                           # cap > 0 时状态栏 cost_display 组尾显示占比（如 · 62%）

# 告警阈值与冷却（默认值；0 = 关闭对应项）
[alerts]
context_critical_pct = 95.0
cost_threshold_usd = 10.0
rate_limit_pct = 90.0
cooldown_minutes = 10             # 冷却窗口：同一种告警在此时间内最多发一次
compaction_eta_minutes = 15       # 压缩临近通知：预测剩余 ≤ 此分钟数时发（0 = 关闭）

# 临时覆盖（不修改 Mod 本身）
[runtime_overrides]
compact_lines = 1
```

## 6 个出厂预设 Mod

| Mod | 场景 | 紧凑 | 仪表盘 | 主题 | 适合 |
|-----|------|------|--------|------|------|
| **glacier-workstation** | daily-dev | Activity 双行 | 2×2 Grid | Nord | 日常开发（默认） |
| obsidian-command | heavy-agent | Agent-Centric 三行 | Sidebar | Dracula | 重度代理 |
| ember-night | night-coding | KPI 双行 | 2×2 Grid | Solarized | 深夜编码 |
| matrix-surveillance | ssh-remote | Activity 双行 | Focus | Monochrome | SSH/远程 |
| noir-precision | daily-dev | Minimal 单行 | 2×2 Grid | Monochrome | 极简 |
| noir-tabbed | small-screen | Contextual 动态 | Tabbed | Monochrome | 笔记本小屏 |

### 日常使用流程

```bash
# 早上 — 日常开发
claude-hud mod use glacier-workstation

# 晚上 — 切换暖色
claude-hud mod use ember-night

# 重度代理 — 侧栏监控
claude-hud mod use obsidian-command

# SSH — 纯 ASCII
claude-hud mod use matrix-surveillance

# 按场景别名切换（daily/night/agent/ssh → 出厂 mod）
claude-hud mod use @night

# 回到上一个 mod（往返 toggle）
claude-hud mod use -

# 序号选择器
claude-hud mod pick

# 恢复出厂默认
claude-hud mod reset
```

## 仪表盘快捷键

| 键 | 功能 |
|----|------|
| `q` / `Esc` | 退出仪表盘（会话写入历史库） |
| `l` | 循环布局 grid-2x2 → sidebar → focus → tabbed（best-effort 持久化到 config.toml `dashboard.default_layout`） |
| `←` / `→` | tabbed 布局下切换 tab（wrap 环绕） |
| `?` | 开合帮助面板 |

底部 1 行常驻提示条：`Layout: <当前布局> · Mod: <当前 Mod> · l=cycle ?=help q=quit`。

> `l` 持久化布局为 best-effort：toML 读写往返会丢失 `config.toml` 中的注释（拍板取舍），其余配置段不受影响。

## Web 仪表盘

```bash
claude-hud serve
# 浏览器打开 http://localhost:9527
```

实时刷新（2s 间隔），显示模型、上下文、费用、时长、**This Week 历史卡片**（本周费用/会话数/token/平均时长/平均代理数；历史库不可用时显示 `—`）+ 全部 Widget 输出。适合放在第二块屏幕。

## 自定义 Widget

### Rhai 脚本 Widget

`config.toml`:
```toml
[widgets.my_custom]
type = "rhai_script"
script_path = "~/.claude/plugins/claude-hud/scripts/my_widget.rhai"
```

`scripts/my_widget.rhai`:
```rhai
let pct = data.context_pct;
let color = if pct > 90.0 { theme.danger }
    else if pct > 70.0 { theme.warning }
    else { theme.success };
`★ ${data.model_name} ${pct.to_int()}% $${data.cost_usd.to_fixed(2, 2)}`
```

### Shell 命令 Widget

```toml
[widgets.ci_status]
type = "shell_output"
command = "curl -s https://ci.example.com/build/123 | jq -r '.status'"
refresh_seconds = "30"
```

平台差异：Unix 用 `sh -c` 执行，Windows 用 `cmd /C` 执行。含引号/管道的复杂命令建议先写成 `.sh` / `.bat` 脚本文件，再让 Widget 调用脚本，避免跨平台转义问题。

### HTTP 轮询 Widget

```toml
[widgets.weather]
type = "http_poll"
url = "https://api.weather.example.com/current"
refresh_seconds = "300"
```

> 布局 ID（`[layout] compact`）全部真实实现：activity / minimal / agent-centric / kpi / contextual；contextual 按 subagent 活跃度动态切换（空闲 → minimal 集，活跃 → activity 集）；未知 ID 报 `layout not implemented` 上屏（doctor 可查）。

## 故障排除

### 状态栏不显示

1. 确认 `claude-hud render` 能正常运行：
   ```bash
   echo '{"model":{"id":"test","display_name":"Test"},"context_window":{"used_percentage":50,"total_input_tokens":1000,"context_window_size":200000},"cost":{"total_cost_usd":0.1,"total_duration_ms":60000}}' | claude-hud render
   ```
2. 检查 `~/.claude/settings.json` 中 statusLine 配置
3. 确认 `claude-hud` 在 PATH 中

### Windows 注意事项

- 需要 Git Bash 或 WSL 提供 `git` 命令
- Nerd Fonts 图标需要安装 Nerd Font 字体，否则使用 `icon_set = "minimal"` 或 `"ascii"`
- 编译时如遇 `rusqlite` 链接错误，确认安装了 `bundled` feature

### macOS 注意事项

- 首次运行时可能被 Gatekeeper 拦截，到"安全性与隐私"中允许

### Linux 注意事项

- 系统通知需要 D-Bus（`libnotify`），多数桌面环境默认已安装
- 无桌面环境时通知功能静默失败

## 项目文件布局

```
~/.claude/plugins/claude-hud/
├── config.toml              # 用户配置
├── mods/                    # 用户安装的 Mod 包
│   └── my-custom.toml
├── scripts/                 # 用户 Rhai 脚本
│   └── my_widget.rhai
└── history.db               # 跨会话历史（`history` 子命令与 Web This Week 卡片数据源）
```

## 版本发布口径

1. **bump 版本**：修改 `Cargo.toml` 的 `version`（如 `0.3.0`），并在 `CHANGELOG.md` 补充对应段
2. **打 tag**：`git tag vX.Y.Z`（带 `v` 前缀，如 `v0.3.0`），push 触发 `.github/workflows/release.yml` 4 平台矩阵构建
3. **tag 原始值（含 `v`）** 写入安装目录的 `version.txt`，下载 URL 使用原始 tag；安装脚本展示版本号时剥离 `v` 前缀（`${LATEST#v}`）
4. **三方同一口径**：
   - `update check` / doctor：从 GitHub latest release 读取 `tag_name`，比较时忽略 `v` 前缀（`cmp_versions` 逐段数字比较）
   - 安装脚本：`version.txt` 与 latest tag 相同 → `up to date`（幂等退出）；不同 → `upgrading`
   - CI：`v*` tag 触发发布，Release body 使用 `CHANGELOG.md`
