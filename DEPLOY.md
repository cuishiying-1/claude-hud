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

安装器自动完成：下载预编译二进制 → 加入 PATH → 运行 `claude-hud setup`（合并 statusLine 到 `~/.claude/settings.json`）。

重启 Claude Code 或执行 `/reload-plugins`，状态栏底部应出现 HUD 显示。

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
| `claude-hud doctor` | 自检：配置/状态行/图标/git/渲染健康报告 |
| `claude-hud uninstall` | 移除 statusLine 与配置目录（卸载脚本内部调用） |

### Mod 管理

| 命令 | 说明 |
|------|------|
| `claude-hud mod list` | 列出所有已安装 Mod |
| `claude-hud mod use <name>` | 切换 Mod（即时生效） |
| `claude-hud mod preview <name>` | 预览 Mod 效果 |
| `claude-hud mod current` | 显示当前激活的 Mod |
| `claude-hud mod save <name>` | 保存当前配置为新 Mod |
| `claude-hud mod pick` | 浏览所有 Mod（交互式选择器开发中） |
| `claude-hud mod export <name>` | 导出 Mod 为 .toml |
| `claude-hud mod import <file>` | 导入 .toml 到本地库 |
| `claude-hud mod delete <name>` | 删除用户 Mod |
| `claude-hud mod reset` | 恢复出厂默认（Glacier Workstation） |

### 主题和 Widget

| 命令 | 说明 |
|------|------|
| `claude-hud theme export` | 导出当前主题 |
| `claude-hud theme import <file>` | 校验主题文件（不持久化，需手动写入 config.toml） |
| `claude-hud widget list` | 列出可用 Widget |
| `claude-hud widget test <name>` | 测试单个 Widget |

### Shell 补全

`claude-hud completion` 为占位实现（仅输出示例文本），当前版本不提供可用补全，后续版本实现。

## 配置文件

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
default_layout = "grid-2x2"    # grid-2x2 | sidebar | tabbed | focus

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

# 恢复出厂默认
claude-hud mod reset
```

## 仪表盘快捷键

| 键 | 功能 |
|----|------|
| `q` / `Esc` | 退出仪表盘 |
| `1`-`9` | 切换标签页（未实现，Phase 4 规划） |

## Web 仪表盘

```bash
claude-hud serve
# 浏览器打开 http://localhost:9527
```

实时刷新（2s 间隔），显示模型、上下文、费用、时长 + 全部 Widget 输出。适合放在第二块屏幕。

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
└── history.db               # 跨会话历史 (Phase 2)
```
