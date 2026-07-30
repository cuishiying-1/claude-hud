# Claude HUD — 插件打包 & 市场发布文档

## 概述

将 Claude HUD 打包为 Claude Code 官方插件，通过 `plugin.json` 清单文件发布到 Claude Code 插件市场，用户可通过 `/plugin install` 一键安装。

## 1. 插件清单文件

在项目根目录创建 `plugin.json`：

```json
{
  "name": "claude-hud",
  "version": "0.1.0",
  "description": "Dual-mode terminal HUD for Claude Code: compact status bar + full-screen TUI dashboard with agent observability, token attribution, and theme engine",
  "author": "your-github-username",
  "license": "MIT",
  "repository": "https://github.com/your-username/claude-hud",
  "homepage": "https://github.com/your-username/claude-hud#readme",
  "keywords": [
    "statusline",
    "dashboard",
    "monitoring",
    "tui",
    "agent-observability",
    "token-tracking",
    "themes"
  ],
  "categories": ["developer-tools", "monitoring"],
  "minimum_claude_code_version": "1.0.80",
  "platforms": ["macos", "linux", "windows"],

  "installation": {
    "type": "binary",
    "prebuilt": {
      "macos-x64": "https://github.com/your-username/claude-hud/releases/download/v{version}/claude-hud-macos-x64",
      "macos-arm64": "https://github.com/your-username/claude-hud/releases/download/v{version}/claude-hud-macos-arm64",
      "linux-x64": "https://github.com/your-username/claude-hud/releases/download/v{version}/claude-hud-linux-x64",
      "windows-x64": "https://github.com/your-username/claude-hud/releases/download/v{version}/claude-hud-windows-x64.exe"
    },
    "build_from_source": {
      "command": "cargo install --path .",
      "requirements": ["rust >= 1.75"]
    }
  },

  "setup": {
    "command": "claude-hud setup",
    "description": "Auto-configure Claude Code status line and create default config"
  },

  "configuration": {
    "file": "~/.claude/plugins/claude-hud/config.toml",
    "format": "toml",
    "schema": {
      "active_mod": { "type": "string", "default": "glacier-workstation", "description": "Active Mod package name" },
      "compact_layout": { "type": "string[]", "description": "Widget order for compact status bar" },
      "dashboard.refresh_interval_ms": { "type": "number", "default": 500 },
      "dashboard.default_layout": { "type": "string", "default": "grid-2x2", "enum": ["grid-2x2", "sidebar", "tabbed", "focus", "hex-2x3", "freeform"] }
    }
  },

  "defaultSettings": {
    "statusLine": {
      "type": "command",
      "command": "claude-hud render",
      "refreshInterval": 5
    }
  },

  "commands": {
    "claude-hud:configure": {
      "command": "claude-hud mod pick",
      "description": "Interactive mod picker: select and preview UI presets"
    },
    "claude-hud:dashboard": {
      "command": "claude-hud dashboard",
      "description": "Open full-screen TUI dashboard"
    },
    "claude-hud:web": {
      "command": "claude-hud serve",
      "description": "Start web dashboard on localhost:9527"
    }
  },

  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Skill",
        "hooks": [
          {
            "type": "command",
            "command": "claude-hud render"
          }
        ]
      }
    ]
  }
}
```

### 字段说明

| 字段 | 必需 | 说明 |
|------|------|------|
| `name` | 是 | 插件唯一标识，小写字母和连字符 |
| `version` | 是 | 语义化版本（SemVer） |
| `description` | 是 | 一句话描述 |
| `repository` | 是 | GitHub 仓库地址 |
| `minimum_claude_code_version` | 否 | 最低 Claude Code 版本要求 |
| `platforms` | 否 | 支持的平台列表 |
| `installation` | 是 | 安装方式：prebuilt 二进制 或 build_from_source |
| `setup` | 否 | 安装后自动执行的配置命令 |
| `configuration` | 否 | 配置文件说明 |
| `defaultSettings` | 否 | 安装后自动写入 settings.json 的配置 |
| `commands` | 否 | 注册 `/plugin-name:command` 斜杠命令 |
| `hooks` | 否 | 插件 hooks 配置 |

## 2. 发布前检查清单

- [ ] `Cargo.toml` 版本号与 `plugin.json` 一致
- [ ] `plugin.json` 语法正确（`jq . plugin.json` 验证）
- [ ] 所有二进制路径可访问（GitHub Release）
- [ ] `claude-hud setup` 命令可正常运行
- [ ] 默认配置与文档描述一致
- [ ] LICENSE 文件存在
- [ ] README 包含安装说明、截图、常见问题
- [ ] 所有 Widget 注册且可正常渲染
- [ ] `claude-hud render` 无 panic
- [ ] `claude-hud dashboard` 可正常退出

## 3. CI/CD 发布流程

### GitHub Actions 配置

`.github/workflows/release.yml`：

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    strategy:
      matrix:
        include:
          - target: x86_64-apple-darwin
            os: macos-latest
            name: macos-x64
          - target: aarch64-apple-darwin
            os: macos-latest
            name: macos-arm64
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-latest
            name: linux-x64
          - target: x86_64-pc-windows-msvc
            os: windows-latest
            name: windows-x64

    runs-on: ${{ matrix.os }}

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Build
        run: cargo build --release --target ${{ matrix.target }}

      - name: Package (Linux/macOS)
        if: matrix.os != 'windows-latest'
        run: |
          cd target/${{ matrix.target }}/release
          tar -czf claude-hud-${{ matrix.name }}.tar.gz claude-hud

      - name: Package (Windows)
        if: matrix.os == 'windows-latest'
        run: |
          cd target/${{ matrix.target }}/release
          7z a claude-hud-${{ matrix.name }}.zip claude-hud.exe

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: claude-hud-${{ matrix.name }}
          path: target/${{ matrix.target }}/release/claude-hud-*

  release:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@v4

      - name: Create Release
        uses: softprops/action-gh-release@v2
        with:
          name: "Claude HUD v${{ github.ref_name }}"
          body_path: CHANGELOG.md
          files: |
            claude-hud-macos-x64/claude-hud-macos-x64.tar.gz
            claude-hud-macos-arm64/claude-hud-macos-arm64.tar.gz
            claude-hud-linux-x64/claude-hud-linux-x64.tar.gz
            claude-hud-windows-x64/claude-hud-windows-x64.zip
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

### 发布步骤

```bash
# 1. 更新版本号
# Cargo.toml: version = "0.2.0"
# plugin.json: "version": "0.2.0"

# 2. 更新 CHANGELOG.md

# 3. 提交并打 tag
git add -A
git commit -m "chore: bump to v0.2.0"
git tag v0.2.0
git push origin main --tags

# 4. GitHub Actions 自动构建并创建 Release

# 5. 验证 Release
curl -L https://github.com/your-username/claude-hud/releases/download/v0.2.0/claude-hud-linux-x64.tar.gz
```

## 4. 发布到 Claude Code 插件市场

### 方式 A：官方插件市场

```bash
# 用户在 Claude Code 中安装
/plugin marketplace add your-username/claude-hud
/plugin install claude-hud
/claude-hud:setup
```

提交到官方市场需要：
1. 在 https://github.com/anthropics/claude-plugins 提交 PR
2. 将 `plugin.json` 添加到市场索引
3. 通过审核后，用户即可通过 `/plugin marketplace add` 搜索和安装

### 方式 B：自托管（独立分发）

用户直接从 GitHub 安装：

```bash
# 方式 1：cargo install
cargo install --git https://github.com/your-username/claude-hud

# 方式 2：下载预编译二进制
curl -L https://github.com/your-username/claude-hud/releases/latest/download/claude-hud-linux-x64 -o ~/.local/bin/claude-hud
chmod +x ~/.local/bin/claude-hud

# 方式 3：Homebrew (需额外配置 tap)
brew tap your-username/claude-hud
brew install claude-hud

# 安装后执行配置
claude-hud setup
```

### 方式 C：安装脚本

`install.sh`：

```bash
#!/bin/bash
set -e

REPO="your-username/claude-hud"
PLATFORM=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$PLATFORM-$ARCH" in
  linux-x86_64)  TARGET="linux-x64" ;;
  linux-aarch64) TARGET="linux-arm64" ;;
  darwin-x86_64) TARGET="macos-x64" ;;
  darwin-arm64)  TARGET="macos-arm64" ;;
  *) echo "Unsupported platform: $PLATFORM-$ARCH"; exit 1 ;;
esac

echo "Installing Claude HUD for $TARGET..."
DOWNLOAD_URL="https://github.com/$REPO/releases/latest/download/claude-hud-$TARGET.tar.gz"

INSTALL_DIR="${HOME}/.local/bin"
mkdir -p "$INSTALL_DIR"

curl -fsSL "$DOWNLOAD_URL" | tar xz -C "$INSTALL_DIR"
chmod +x "$INSTALL_DIR/claude-hud"

echo "Claude HUD installed to $INSTALL_DIR/claude-hud"
echo "Running setup..."
"$INSTALL_DIR/claude-hud" setup

echo "Done! Restart Claude Code or run /reload-plugins."
```

用户使用：
```bash
curl -fsSL https://raw.githubusercontent.com/your-username/claude-hud/main/install.sh | bash
```

## 5. README 模板

```markdown
# Claude HUD

Dual-mode terminal HUD for Claude Code. Compact status bar for daily use + full-screen TUI dashboard for deep diagnostics.

## Features

- **Compact Status Bar**: model, context, agents, skills/MCP, cost, git — all in 1-3 lines
- **Full-screen Dashboard**: agent overview, token attribution, timeline, trends
- **Agent Observability**: see what sub-agents are doing, detect stalls, find token hogs
- **6 Theme Presets**: Noir, Obsidian Neon, Ember Warmth, Glacier Steel, Matrix, and custom
- **Mod System**: save, switch, and share complete UI configurations
- **Rhai Scripting**: write custom widgets in 10 lines of Rhai
- **Web Dashboard**: second-screen monitoring at localhost:9527
- **Cross-platform**: macOS, Linux, Windows — single binary, zero runtime deps

## Quick Install

### Via Plugin Marketplace
```
/plugin marketplace add your-username/claude-hud
/plugin install claude-hud
/claude-hud:setup
```

### Via Cargo
```bash
cargo install claude-hud
claude-hud setup
```

### Via Install Script
```bash
curl -fsSL https://raw.githubusercontent.com/your-username/claude-hud/main/install.sh | bash
```

## Usage

```bash
# Switch UI presets
claude-hud mod use @daily     # Glacier Workstation
claude-hud mod use @night     # Ember Night Shift
claude-hud mod use @agent     # Obsidian Command Center

# Open dashboard
claude-hud dashboard

# Web dashboard (second screen)
claude-hud serve
```

## Screenshots

<!-- Add screenshots here -->

## License

MIT
```

## 6. 版本管理策略

| 版本号 | 规则 |
|--------|------|
| **Major** (x.0.0) | 破坏性 API 变更、配置文件格式变更 |
| **Minor** (0.x.0) | 新 Widget、新主题、新 CLI 命令 |
| **Patch** (0.0.x) | Bug 修复、性能优化 |

首次发布版本：`v0.1.0`

## 7. 打包脚本

`scripts/package.sh`：

```bash
#!/bin/bash
set -e

VERSION=$(grep 'version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
echo "Packaging Claude HUD v$VERSION"

# Build
cargo build --release

# Create dist directory
rm -rf dist
mkdir -p dist

# Copy binary and config
cp target/release/claude-hud dist/
cp config.toml dist/
cp plugin.json dist/
cp README.md dist/
cp LICENSE dist/

# Create archive
cd dist
tar -czf "../claude-hud-v$VERSION-$(uname -s)-$(uname -m).tar.gz" *
cd ..

echo "Package created: claude-hud-v$VERSION-$(uname -s)-$(uname -m).tar.gz"
```

## 8. 常见问题

### Q: 插件安装后状态栏没有变化？
A: 运行 `/reload-plugins` 或重启 Claude Code。

### Q: 如何回滚到默认状态栏？
A: 在 `~/.claude/settings.json` 中删除 `statusLine` 字段。

### Q: 预编译二进制报 "cannot execute binary file"？
A: 确认下载了正确平台的二进制（`uname -m` 查看架构）。

### Q: 如何提交到官方插件市场？
A: 1) Fork https://github.com/anthropics/claude-plugins 2) 在 `plugins/` 目录添加你的 `plugin.json` 3) 提交 PR 4) 等待审核。
