# Claude HUD — 部署与使用文档

## 项目概述

Claude HUD 是 Claude Code 的双模终端可视化插件：紧凑状态栏（日常使用）+ 全屏仪表盘（深度诊断）。

**技术栈**：Rust 2021 · ratatui · crossterm · serde · clap · rusqlite · rhai · notify-rust

## 快速开始

### 一键安装（推荐，无需 Rust）

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/<user>/claude-hud/main/scripts/install.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/<user>/claude-hud/main/scripts/install.ps1 | iex
```

安装器自动完成：下载预编译二进制 → 加入 PATH → 运行 `claude-hud setup`（合并 statusLine 到 `~/.claude/settings.json`）。输出三态：`installing` / `up to date`（幂等跳过）/ `upgrading`。

> **尚未发布**：当前仓库为占位符（`user/claude-hud`），安装脚本检测到后直接报错退出。真实 release 创建前请使用 `cargo build --release` 本地构建。

重启 Claude Code 或执行 `/reload-plugins`，状态栏底部应出现 HUD 显示。

> `setup` 在已存在 statusLine 或 settings.json 损坏时，每次运行都会生成新的时间戳备份 `settings.json.hud.bak-<epoch>`（永不自动删除，可用于回溯）。

### 一键卸载

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/<user>/claude-hud/main/scripts/uninstall.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/<user>/claude-hud/main/scripts/uninstall.ps1 | iex
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
| `claude-hud update check` | 检查新版本（占位符仓库输出 `not published yet`） |

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
| `claude-hud mod delete <name>` | 删除用户 Mod |
| `claude-hud mod reset` | 恢复出厂默认（Glacier Workstation） |

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

### 状态栏宽度

紧凑模式输出做**宽度感知**：以 `COLUMNS` 环境变量为宽度源（statusLine 场景下终端不会真正 resize，环境变量是唯一可靠信号）。超出可用宽度时从行尾整组丢弃直至适配；单字段超过 24 字符（model 名 / git 分支 / 代理名）截断并加 `…`。`COLUMNS` 缺失或非法时默认 80 列，最小钳制 40 列。

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
# 激活的 Mod
active_mod = "glacier-workstation"

# 紧凑模式 Widget 顺序
compact_layout = [
    "model_display",
    "context_bar",
    "agent_overview",
    "cost_display",
    "skills_mcp",
    "alerts",
]

separator = " │ "

[dashboard]
refresh_interval_ms = 500
default_layout = "grid-2x2"    # grid-2x2 | sidebar | focus（仪表盘内按 `l` 循环切换并持久化到此键）

# Widget 级配置
[widgets.context_bar]
bar_width = "18"
gradient = "true"
warn_threshold = "80"
critical_threshold = "95"

[widgets.cost_display]
currency_symbol = "¥"
warn_threshold_usd = "10.0"

[widgets.agent_overview]
stall_threshold_sec = "30"

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
| `l` | 循环布局 grid-2x2 → sidebar → focus（best-effort 持久化到 config.toml `dashboard.default_layout`） |
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
