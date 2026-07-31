# Claude HUD 安装使用流程简化 — 设计文档

日期：2026-07-31
状态：已获用户批准（分三节确认）

## 1. 背景与目标

当前安装使用流程前置操作和依赖过多，用户体验差：

1. 必须安装 Rust 工具链（国内还要配镜像）才能编译
2. 编译产物不在 PATH，`claude-hud` 直接 command not found
3. `claude-hud setup` 在 `~/.claude/settings.json` 已存在时只打印 JSON 让用户手动粘贴，不做合并
4. 图标依赖 Nerd Font，无字体时显示乱码，且 `icon_set = "minimal"` 是隐藏配置
5. 运行时依赖 git（Windows 需 Git Bash/WSL）
6. 验证链路长，不显示时只能瞎猜排查

**目标**：公开分发。安装/卸载各一条命令，运行时零依赖，问题可自查。

**分发渠道**：一键安装脚本（主）+ Claude Code 插件市场 plugin.json（轻量补充，不承担安装职责）。

## 2. 分发管线：预编译二进制

GitHub Actions 矩阵构建，tag `v*` 触发（复用 PLUGIN.md 既有矩阵设计）：

| 资产 | 目标平台 |
|------|----------|
| `claude-hud-macos-x64.tar.gz` | macOS Intel |
| `claude-hud-macos-arm64.tar.gz` | macOS Apple Silicon |
| `claude-hud-linux-x64.tar.gz` | Linux x64 |
| `claude-hud-windows-x64.zip` | Windows x64 |

- 安装脚本始终从 `releases/latest` 拉取，不写死版本号
- 本地已有安装时对比版本，同版本跳过覆盖（幂等）

## 3. 安装脚本（一行命令，免管理员）

```
macOS/Linux:   curl -fsSL https://github.com/<user>/claude-hud/raw/main/scripts/install.sh | bash
Windows:       irm https://raw.githubusercontent.com/<user>/claude-hud/main/scripts/install.ps1 | iex
```

> `<user>` 为发布仓库的 GitHub 用户名。当前 Cargo.toml 中 repository 为占位值 `github.com/user/claude-hud`，发布前必须替换为真实地址并保证 scripts/ 目录在 main 分支。

脚本流程：

1. 识别平台/架构，下载对应资产到用户目录：
   - Unix：`~/.local/bin`
   - Windows：`%LOCALAPPDATA%\claude-hud\bin`（避免 Program Files 需要管理员）
2. PATH 配置：
   - Unix：检查 `~/.local/bin` 已在 PATH 则跳过；缺失才追加到 `~/.bashrc`/`.zshrc`，并打印提示
   - Windows：写入 `HKCU\Environment`（用户级注册表，免管理员）+ 广播 WM_SETTINGCHANGE
3. 自动执行 `claude-hud setup`
4. 打印验证指引：样例 JSON 管道渲染测试 + "重启 Claude Code 或 /reload-plugins"

## 4. 卸载脚本（一行命令）

```
macOS/Linux:   curl -fsSL https://github.com/<user>/claude-hud/raw/main/scripts/uninstall.sh | bash
Windows:       irm https://raw.githubusercontent.com/<user>/claude-hud/main/scripts/uninstall.ps1 | iex
```

流程（顺序重要）：

1. 调用 `claude-hud uninstall`（二进制内置）：从 settings.json 移除 statusLine、删除配置目录 —— **先摘掉 statusLine 再删二进制**，避免 Claude Code 每 5 秒调用已删除的命令
2. 移除 PATH 条目
3. 删除二进制（Windows 遇文件占用则重试，仍失败提示手动删除）
4. 输出"已卸载完成"

## 5. `claude-hud setup` — 自动合并

现状问题：`~/.claude/settings.json` 已存在时只打印 JSON 让用户手动粘贴（src/main.rs:196-199）。

新行为：

1. 读现有 settings.json（解析失败则先备份 `.bak` 再以最小 JSON 重建，不覆盖用户数据）
2. 合并 `statusLine` 键，保留用户已有其他所有键
3. 写回前先备份 `settings.json.bak`（一层安全网）
4. 合并逻辑抽成纯函数 `merge_status_line(existing: &str) -> String`，配单元测试

## 6. `claude-hud uninstall`（二进制内置）

```
claude-hud uninstall
```

1. 从 settings.json 移除 `statusLine` 键（`remove_status_line`，合并函数反向操作）
2. 删除配置目录 `~/.claude/plugins/claude-hud/`（config.toml、mods/、历史库）
3. 打印"二进制可安全删除"

与卸载脚本的配合：脚本先跑此命令摘掉 statusLine，再删 PATH 和二进制。

## 7. `claude-hud doctor` — 自检命令

逐项检查输出 ✅/⚠️/❌ + 修复建议，exit code 0 表示全部正常：

| 检查项 | 判定 |
|--------|------|
| 二进制版本 & 安装位置 | 在 PATH 中的路径 |
| config.toml 存在 & 可解析 | 缺失则提示先跑 `setup` |
| settings.json 的 statusLine | 指向的 `claude-hud render` 与当前版本一致 |
| 图标集决议 | `auto` 模式实际解析到哪个图标集 |
| git 可用性 | 影响 git_status widget 显示 |
| 样例渲染 | 内部跑一次 render 流水线验证无 panic |

## 8. 运行时零依赖

### 8a. 字体：`icon_set = "auto"`（新默认值）

现状：默认 `nerd`，无字体直接乱码。新默认值 `auto`：

1. `IconSet` 枚举新增 `Auto` 变体（src/core/theme.rs）
2. 检测逻辑（每次 `render` 进程启动时执行一次，快探针，不缓存）：
   - Windows：查询注册表 `HKLM\...\Fonts` 键名含 "Nerd"（<1ms）
   - Linux：`fc-list | grep -i nerd`（fc-list 不存在 → 视为无 → minimal）
   - macOS：扫描 `~/Library/Fonts`、`/Library/Fonts` 文件名含 "Nerd"
3. **任何检测失败/无字体 → Minimal 图标集**，绝不出乱码
4. 用户仍可显式指定 `icon_set = "nerd"` 强制覆盖
5. 检测函数抽象为可注入的闭包/trait，保证纯函数可单测

### 8b. git 优雅降级

- `probe_git` 已返回 `None`（src/probe/git.rs:13-14）
- 要求：`git_status` widget 对 `None` 显示占位符或完全隐藏，不 panic、不输出乱码
- Linux 通知 D-Bus 缺失已静默（notify-rust 错误被忽略），保持现状

## 9. plugin.json — 轻量注册

- 只注册命令：`/claude-hud:dashboard`、`/claude-hud:web`、`/claude-hud:doctor`
- **不写** `defaultSettings` statusLine（避免与脚本管理的 statusLine 双写冲突）
- 文档标注：可选渠道，安装仍走一键脚本

## 10. 测试与验证

- Rust 单测：
  - `merge_status_line` / `remove_status_line`：空文件 / 已含 statusLine / 含其他配置键 / 非法 JSON 四类用例
  - `IconSet::Auto` 决议逻辑：注入式检测函数（有字体 / 无字体 / 检测失败 → minimal）
- 脚本检查（CI）：
  - `install.sh`/`uninstall.sh`：`bash -n` + shellcheck
  - Windows 脚本：GitHub Actions windows runner 真装真卸冒烟测试（临时目录 + 假 HOME）
- 文档：更新 DEPLOY.md、README

## 11. 涉及文件清单

| 文件 | 变更 |
|------|------|
| `.github/workflows/release.yml` | 新建：矩阵构建 + Release + 脚本检查 |
| `scripts/install.sh` / `uninstall.sh` / `install.ps1` / `uninstall.ps1` | 新建 |
| `src/main.rs` | `setup` 重写、`uninstall`/`doctor` 子命令 |
| `src/core/theme.rs` | `IconSet::Auto` + 检测逻辑 |
| `src/widgets/git_status.rs` | `None` 降级确认/补齐 |
| `plugin.json` | 新建（轻量） |
| `DEPLOY.md` / `README.md` | 更新 |
| 对应 `#[cfg(test)]` | 新建单测 |

## 12. 不做的事（YAGNI）

- 不做 statusLine 双写保护机制之外的复杂版本协商
- 不做 Homebrew tap / Scoop / AUR 等包管理器分发（后续需要再加）
- 不做字体自动安装（只降级，不替你装字体）
- 插件市场提交审核（plugin.json 先落地，审核后议）
