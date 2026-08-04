# Changelog

## [Unreleased]

- 安装使用流程简化：一键安装/卸载脚本、`setup` 自动合并、`uninstall`/`doctor` 子命令、`icon_set = "auto"` 零依赖字体决议

## [0.3.0] - 2026-08-03 (Phase 4 — batch C remainder)

### Added
- `claude-hud history` 子命令（Weekly stats / Recent sessions / Daily cost，空库显示 `—`）
- render 会话切换自动结账：transcript 路径变化 → 上一会话写入 SQLite 历史库
- serve `/api/data` 增加 `weekly` 字段 + 前端 This Week 卡片（不可用时 `available:false` 显示 `—`）
- `claude-hud update check` 子命令（占位符仓库零网络短路）+ doctor `update:` 信息项
- `completion bash/zsh/fish/powershell` 真实补全脚本（clap_complete；不支持 shell 报错）
- compact 零宽度感知：COLUMNS 宽度源 + `fit_line` 组级截断 + 字段级 24 字符截断
- dashboard `l` 布局循环（grid-2x2/sidebar/focus）+ config.toml 持久化 + `?` 帮助面板 + 底部提示
- dashboard 代理结束 / 代理卡顿桌面通知接线（进程内去重）
- setup 时间戳备份 `settings.json.hud.bak-<epoch>`（不再覆盖固定名备份，永不删除）
- 写配置命令（mod use/reset/save/delete/import、theme import）输出 `(applies to all windows)` 提示
- install.sh/ps1 占位符仓库检测（未发布报错）+ 三态安装输出（installing / up to date / upgrading）

### Fixed
- Shell Widget Windows 分支：`cmd /C`（Unix 保持 `sh -c`）；删除无调用者的 probe/system.rs
- dashboard `'1'..='9'` 空分支删除；`last_agent_count` 不再恒 0（退出结账携带真实代理数）

## [0.2.0] - 2026-08-03 (Phase 3)

### Added
- theme 支持字符串预设 / [theme] 表 / preset+overrides 三种引用形态
- mod use 校验、mod use - 往返切换、@scene 场景别名、mod pick 序号选择器
- mod save 真实配置快照（compact_widgets 字段）
- theme import 落盘 config.toml [theme] 段

### Fixed
- 4 个 widget ANSI 空字符串上色（数字/符号整体入色）
- 坏 config 不再静默（stderr 警告 + doctor [!!]）

## v0.1.0 (unreleased)

### Phase 1 — Core Skeleton
- CLI entry point with 18 subcommands (render, dashboard, serve, setup, mod, theme, widget, completion)
- 7 compact-mode Widgets: context_bar, model_display, cost_display, agent_overview, skills_mcp, rate_limits, git_status
- 6 built-in Theme presets: Nord (default), Dracula, Tokyo Night, Catppuccin, Monochrome, Solarized Dark
- 20 style tokens with 3-level configuration depth (preset → overrides → full custom)
- 3 icon sets: Nerd Fonts, ASCII, Minimal
- Compact mode rendering engine with 3-line presets (Full / Essential / Minimal)
- 5 P1 animation effects: True Color gradient, neon breathing, pseudo-3D panels, cinematic reveal, CRT scanlines
- 6 factory preset Mods: Glacier Workstation, Obsidian Command Center, Ember Night Shift, Matrix Surveillance, Noir Precision, Noir Tabbed
- `claude-hud setup` auto-configuration

### Phase 2 — Deep Diagnostics
- Transcript JSONL incremental parser (agent events, tool calls, skill/MCP detection, token attribution)
- 5 P2 Widgets: agent_detail, token_attribution, agent_timeline, session_stats, skills_mcp_dynamic
- Smart alerts with compaction prediction
- Cross-session SQLite history (weekly stats, daily cost trends)
- OS native desktop notifications
- 10 P2 animation effects: eased counters, spark trails, braille spectrum, heatmap, wave distortion, liquid fill, glitch, barber pole, marquee, RGB spectrum cycle
- Full dashboard with 3 layout modes (grid-2x2, sidebar, focus)

### Phase 3 — Extension Ecosystem
- Rhai scripting engine for user-defined widgets
- Shell command widget (arbitrary command → status bar output)
- HTTP polling widget (remote API → status bar output)
- Web dashboard HTTP server (localhost:9527)
- Mod management with 5-layer quick switching (fuzzy match, tab completion, quick undo `-`, scene aliases `@`, interactive picker)
- Mod export/import for community sharing

### Design
- Full DESIGN.md with 16 chapters covering competitive analysis, widget catalog, layout system, theme engine, animation effects, Mod system, and phased implementation plan
