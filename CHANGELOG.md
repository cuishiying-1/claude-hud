# Changelog

## [Unreleased]

- 安装使用流程简化：一键安装/卸载脚本、`setup` 自动合并、`uninstall`/`doctor` 子命令、`icon_set = "auto"` 零依赖字体决议

## [0.6.0] - 2026-08-04 (v0.4 视觉批次)

### Added
- 动画系统重建为时间相位纯函数（now_phase/breathe/gradient/ease_out/scanline_offset，`CLAUDE_HUD_PHASE` env 黑盒确定性）；删除 frame 制 AnimationState
- context_bar 渐变进度条：逐 cell truecolor 渐变替 3 档变色（接线既有 `gradient` 配置键，默认开）
- 新 widget `token_rate`：紧凑 `tok 3.1k/min` 速率文本 + 仪表盘最近 24 桶盲文频谱竖条（token_timeline 数据源；空数据 `—`）
- dashboard CRT 扫描线背景（`[dashboard] scanlines`，默认开）+ 伪 3D 面板（focus/tabbed accent 边框 + 偏移阴影）
- tabbed 布局补全：四态布局循环 + 顶部 tab 条 + `←`/`→` 切换（noir-tabbed mod 声明的 Tabbed 布局不再是 focus 别名）
- 缓动计数器（仪表盘 cost_display 0.8s ease-out；紧凑进程重生单帧不适用，拍板确认）

## [0.5.0] - 2026-08-04 (v0.3 性能与卫生批次)

### Added
- ⑳ 状态栏预算占比：`cap_usd > 0` 且成本 > 0 时 cost_display 组尾追加 `· NN%`（实时 ≈ 成本 ÷ cap；cap 默认 0 隐藏）

### Changed
- ⑨ 结账去重升级：单槽记忆（只记最后一次结账）在 path 振荡 A→B→A→B 下相位错位无法去重，改为 **path→ts 结账表**（`state.checkout_billed`）——同 path 在冷却期内最多结账一次，振荡不再 double-billing；冷却期外记录自动清理（表有界）

### Performance
- token_timeline 分桶上限 360（6h 滚动窗口；压缩预测只读首尾桶，不受影响）
- serve 历史聚合缓存：`/api/data` 2s 轮询不再每次重开 SQLite，30s TTL 命中即回（weekly/trend 分钟级统计）

### Cleanup
- 17 个构建 warning 清零：animation.rs 收缩为帧计数 + `neon_breathing`（9 个未接线原语 + `Spark`/`hsl_to_rgb` 按拍板删除，v0.4 时间相位重建蓝图保留）；Widget trait 删 `dashboard_size`/`needs_tick`（无调用点 + 6 处覆写）；SubagentInfo 删 3 未读字段；SessionRecord 删 3 未读字段（INSERT/SELECT 同步，CREATE TABLE 不变）；TranscriptEntry 删 `ToolResult`/`UserEntry` 变体（serde(other) 兜底解析不破）；4 处未用 import + `interpolate_hex` 等死代码

## [0.4.0] - 2026-08-04 (v0.2 成本哨兵批次)

### Added
- ⑲ 实时成本双轨：realtime_cost（stdin 累计 token × in/out 单价，无 cache → ≈）注入 render 路径；effective_cost（transcript 含 cache）保留 dashboard
- cost_display 合并单组 `≈$X.XX · Xk/Xk tok`（k 缩写 + 零数据 `—` 降级）
- serve `/api/data` 增 `pricing_configured`/`model_id` + 前端未配置单价提示；dashboard cost_display 行尾标注
- ⑳ `[budget]` 配置段（cap_usd + warn_pcts）+ check_budget 档位单调/跨进程冷却（复用 [alerts].cooldown_minutes）+ state.budget_tier + notify::budget + doctor budget_check
- ㉑ `history --weekly` 五指标周报（MAX 口径独立查询）+ serve `trend` 字段 + 前端周趋势曲线

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
