# Changelog

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
