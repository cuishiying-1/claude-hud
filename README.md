# Claude HUD

Dual-mode terminal HUD for Claude Code. Compact status bar for daily use + full-screen TUI dashboard for deep diagnostics. Zero runtime dependencies, one-line install.

## Features

- **Compact Status Bar**: model, context, agents, skills/MCP, cost, git — all in 1-3 lines
- **Full-screen Dashboard**: agent overview, token attribution, timeline, trends
- **Agent Observability**: see what sub-agents are doing, detect stalls, find token hogs
- **6 Theme Presets** + custom themes and Mod packages
- **Web Dashboard**: second-screen monitoring at localhost:9527
- **Zero-dependency**: auto-fallback icons (no Nerd Font required), graceful degradation without git
- **Cross-platform**: macOS, Linux, Windows — single binary

## Install

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/cuishiying-1/claude-hud/main/scripts/install.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/cuishiying-1/claude-hud/main/scripts/install.ps1 | iex
```

The installer downloads a prebuilt binary, adds it to PATH, and configures the Claude Code status line. Restart Claude Code or run `/reload-plugins` to see the HUD.

> **No release yet** — until the first GitHub release is cut, the install
> scripts report that no release exists. Use `cargo build --release` locally for now.

## Upgrade

Re-run the install command to upgrade — the installer detects the installed
version and upgrades automatically. `config.toml` and session history are kept
in `~/.claude/plugins/claude-hud/` and survive upgrades.

## Uninstall

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/cuishiying-1/claude-hud/main/scripts/uninstall.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/cuishiying-1/claude-hud/main/scripts/uninstall.ps1 | iex
```

## Usage

```bash
claude-hud doctor          # self-check: config, status line, icons, git
claude-hud mod list        # list available UI mods
claude-hud mod use <name>  # switch UI mod (e.g. glacier-workstation)
claude-hud dashboard       # full-screen TUI dashboard (q/Esc to exit)
claude-hud serve           # web dashboard at localhost:9527
claude-hud history         # weekly stats / recent sessions / daily cost
claude-hud history --weekly  # weekly five-metric report (cost/sessions/tokens/longest/top session)
claude-hud update check    # check for a new release
claude-hud completion bash # generate shell completions (bash/zsh/fish/powershell)
```

## Configuration

Config lives at `~/.claude/plugins/claude-hud/config.toml`. Full reference (widgets, themes, mods, Rhai scripting) is in [DEPLOY.md](DEPLOY.md).

## Building from source (developers)

```bash
cargo build --release
```

Requires Rust toolchain. End users don't need Rust — install via the one-line scripts above.

## License

MIT
