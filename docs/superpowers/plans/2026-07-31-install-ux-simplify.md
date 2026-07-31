# Claude HUD 安装使用流程简化 — 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Claude HUD 的安装/卸载简化为每平台一行命令、运行时零依赖，并提供 `doctor` 自检。

**Architecture:** ① GitHub Actions 矩阵产出 3 平台预编译二进制（GitHub Releases）；② `scripts/` 下 4 个脚本（install/uninstall × Unix/Windows）做下载、PATH、setup 自动化，支持 `HUD_LOCAL_BIN`/`HUD_LOCAL_STUB` 本地模式供 CI 冒烟测试；③ Rust 侧：`setup` 改为自动合并 settings.json、新增 `uninstall`/`doctor` 子命令、`IconSet::Auto` 在 main 加载主题时决议一次（widgets 不改逻辑）、`git_status` 抽纯函数。plugin.json 只注册斜杠命令，不承担安装。

**Tech Stack:** Rust 2021 / clap / serde_json / GitHub Actions / bash + PowerShell

**约定**（全局，所有任务遵守）：
- 提交按项目规范**由用户手动执行**：每任务末尾的 "Checkpoint" 步骤列出建议的 `git add`/`git commit` 命令，用户确认后自行执行，实施者**不得**自动提交。
- 测试命令：`cargo test`（项目无独立 tests 目录，全部用文件内 `#[cfg(test)]` 模块）。
- 仓库占位符：当前 Cargo.toml 的 repository 为 `github.com/user/claude-hud`。脚本与 plugin.json 中 `user/claude-hud` 为**发布前必须替换**的真实仓库地址（任务 6/7/8 有显式替换步骤）。

---

### Task 1: `core/cc_config.rs` — settings.json 合并/移除纯函数（TDD）

**Files:**
- Create: `src/core/cc_config.rs`
- Modify: `src/core/mod.rs`（追加 `pub mod cc_config;`）

- [ ] **Step 1: 写失败测试**

创建 `src/core/cc_config.rs`，先写测试：

```rust
use serde_json::{Map, Value};

/// Merge the Claude HUD statusLine into Claude Code settings.json content.
/// Returns the pretty-printed merged JSON. Empty input starts from {}.
pub fn merge_status_line(existing: &str) -> Result<String, String> {
    let mut root = parse_root(existing)?;
    root["statusLine"] = serde_json::json!({
        "type": "command",
        "command": "claude-hud render",
        "refreshInterval": 5
    });
    pretty(&root)
}

/// Remove the Claude HUD statusLine key from settings.json content.
/// Returns the pretty-printed JSON without the statusLine key.
pub fn remove_status_line(existing: &str) -> Result<String, String> {
    let mut root = parse_root(existing)?;
    if let Some(obj) = root.as_object_mut() {
        obj.remove("statusLine");
    }
    pretty(&root)
}

fn parse_root(existing: &str) -> Result<Value, String> {
    if existing.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_str(existing).map_err(|e| format!("parse settings.json: {}", e))
}

fn pretty(root: &Value) -> Result<String, String> {
    serde_json::to_string_pretty(root).map_err(|e| format!("serialize settings.json: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECTED_COMMAND: &str = "\"command\": \"claude-hud render\"";

    #[test]
    fn merge_empty_input_creates_status_line() {
        let out = merge_status_line("").unwrap();
        assert!(out.contains(EXPECTED_COMMAND));
        assert!(out.contains("\"statusLine\""));
    }

    #[test]
    fn merge_preserves_existing_keys() {
        let out = merge_status_line(r#"{"apiKeyHelper":{"alwaysAllowedTools":[]}}"#).unwrap();
        let root: Value = serde_json::from_str(&out).unwrap();
        assert!(root.get("apiKeyHelper").is_some());
        assert!(root.get("statusLine").is_some());
        assert_eq!(root["statusLine"]["command"], "claude-hud render");
        assert_eq!(root["statusLine"]["refreshInterval"], 5);
    }

    #[test]
    fn merge_replaces_existing_status_line_without_duplication() {
        let out = merge_status_line(r#"{"statusLine":{"type":"command","command":"old-cmd","refreshInterval":1}}"#).unwrap();
        let root: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(root["statusLine"]["command"], "claude-hud render");
        assert_eq!(root.as_object().unwrap().get("statusLine").unwrap().as_object().unwrap().len(), 3);
    }

    #[test]
    fn merge_invalid_json_returns_err() {
        assert!(merge_status_line("{not json").is_err());
    }

    #[test]
    fn remove_empty_status_line_keeps_other_keys() {
        let out = remove_status_line(r#"{"statusLine":{},"permissions":{}}"#).unwrap();
        let root: Value = serde_json::from_str(&out).unwrap();
        assert!(root.get("statusLine").is_none());
        assert!(root.get("permissions").is_some());
    }

    #[test]
    fn remove_missing_status_line_is_noop() {
        let out = remove_status_line(r#"{"permissions":{}}"#).unwrap();
        let root: Value = serde_json::from_str(&out).unwrap();
        assert!(root.get("permissions").is_some());
    }

    #[test]
    fn remove_empty_input_returns_empty_object() {
        let out = remove_status_line("").unwrap();
        assert_eq!(out, "{}");
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test cc_config`
Expected: 编译错误 `unresolved module 'cc_config'`（core/mod.rs 尚未声明）或测试失败。

- [ ] **Step 3: 注册模块**

在 `src/core/mod.rs` 末尾追加：

```rust
pub mod cc_config;
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test cc_config`
Expected: 7 个测试全部 PASS。

- [ ] **Step 5: Checkpoint（用户手动提交）**

```bash
git add src/core/cc_config.rs src/core/mod.rs
git commit -m "feat(core): add settings.json statusLine merge/remove pure functions"
```

---

### Task 2: `setup` 自动合并 + `uninstall` 子命令 + `--version`

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: 给 Cli 加 version**

`src/main.rs` 中 `#[derive(Parser)]` 上方：

```rust
#[command(name = "claude-hud", version)]
```

- [ ] **Step 2: 新增 Uninstall 子命令**

`enum Commands`（`Setup,` 之后）追加：

```rust
    /// Remove statusLine from Claude Code settings and delete config dir
    Uninstall,
```

- [ ] **Step 3: dispatch 处接入**

`let result = match cli.command {` 中 `Commands::Setup => run_setup(),` 后追加：

```rust
        Commands::Uninstall => run_uninstall(),
```

- [ ] **Step 4: 重写 `run_setup`**

替换 `src/main.rs` 中现有 `run_setup` 整个函数（约 166-203 行）：

```rust
fn run_setup() -> Result<(), String> {
    let config_path = AppConfig::config_path()?;
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
    }
    if !config_path.exists() {
        let default_config = toml::to_string_pretty(&AppConfig::default())
            .map_err(|e| format!("serialize config: {}", e))?;
        std::fs::write(&config_path, default_config)
            .map_err(|e| format!("write config: {}", e))?;
        println!("Config written to {:?}", config_path);
    } else {
        println!("Config already exists at {:?}", config_path);
    }
    setup_cc_settings()?;
    Ok(())
}

/// Merge the HUD statusLine into ~/.claude/settings.json, backing up
/// the original content first. Invalid existing JSON is backed up and
/// rebuilt from {} rather than overwriting user data.
fn setup_cc_settings() -> Result<(), String> {
    let home = dirs::home_dir()
        .ok_or_else(|| "cannot find home directory".to_string())?;
    let settings_path = home.join(".claude").join("settings.json");
    let original = if settings_path.exists() {
        std::fs::read_to_string(&settings_path)
            .map_err(|e| format!("read settings.json: {}", e))?
    } else {
        String::new()
    };

    if !original.trim().is_empty() {
        let backup = settings_path.with_extension("json.bak");
        std::fs::write(&backup, &original)
            .map_err(|e| format!("backup settings.json: {}", e))?;
        println!("Backup saved to {:?}", backup);
    }

    let merged = match core::cc_config::merge_status_line(&original) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("warning: {} — rebuilding from backup", e);
            core::cc_config::merge_status_line("")?
        }
    };
    std::fs::write(&settings_path, merged)
        .map_err(|e| format!("write settings.json: {}", e))?;
    println!("Claude Code status line configured in {:?}", settings_path);
    Ok(())
}

fn run_uninstall() -> Result<(), String> {
    let home = dirs::home_dir()
        .ok_or_else(|| "cannot find home directory".to_string())?;
    let settings_path = home.join(".claude").join("settings.json");
    if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)
            .map_err(|e| format!("read settings.json: {}", e))?;
        match core::cc_config::remove_status_line(&content) {
            Ok(updated) => {
                std::fs::write(&settings_path, updated)
                    .map_err(|e| format!("write settings.json: {}", e))?;
                println!("Removed statusLine from {:?}", settings_path);
            }
            Err(e) => eprintln!("warning: skip settings.json cleanup ({})", e),
        }
    }
    let config_dir = home.join(".claude").join("plugins").join("claude-hud");
    if config_dir.exists() {
        std::fs::remove_dir_all(&config_dir)
            .map_err(|e| format!("remove config dir: {}", e))?;
        println!("Removed config dir {:?}", config_dir);
    }
    println!("Done. The claude-hud binary can now be safely deleted.");
    Ok(())
}
```

注意：若 `use crate::core::cc_config;` 不存在，因 main.rs 有 `use core::config::AppConfig;` 的既有风格（`mod core;` 声明后同 crate 内直接 `core::cc_config` 路径即可，无需新 use；如编译报 unresolved，在文件头部 `use core::config::AppConfig;` 行附近补 `use core::cc_config;`）。

- [ ] **Step 5: 编译检查**

Run: `cargo build`
Expected: 编译通过，无 warning 新增。

- [ ] **Step 6: 手工冒烟**

Run: `cargo run -- setup`
Expected: 输出 Config/Backup/configured 三行信息；`~/.claude/settings.json` 含 `"statusLine"` 且原有键保留。

Run: `cargo run -- uninstall`
Expected: 输出 Removed statusLine + Removed config dir + Done。

Run: `cargo run -- setup`（再次）
Expected: 能重新写入配置（幂等）。

- [ ] **Step 7: Checkpoint（用户手动提交）**

```bash
git add src/main.rs
git commit -m "feat(cli): setup auto-merges settings.json; add uninstall subcommand and --version"
```

---

### Task 3: `compact.rs` 重构 + `doctor` 自检命令（TDD）

**Files:**
- Modify: `src/compact.rs`
- Create: `src/doctor.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: 重构 compact::render，抽出 render_with_data**

`src/compact.rs` 中，把现有 `render` 函数（约 11-68 行）拆为两层——`render` 保留"读 stdin → 解析 → 调用"：

```rust
/// Render the compact status bar from stdin JSON data.
pub fn render(
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> Result<String, String> {
    let stdin_data = read_stdin()?;
    let data = SessionData::from_stdin_json(&stdin_data)
        .map_err(|e| format!("parse stdin JSON: {}", e))?;
    render_with_data(&data, registry, config, theme)
}

/// Render the compact status bar from an already-parsed session snapshot.
/// Shared by `render` (stdin) and `doctor` (sample data).
pub fn render_with_data(
    data: &SessionData,
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> Result<String, String> {
    parse_and_push_transcript(data, registry);

    let layout = &config.compact_layout;
    if layout.is_empty() {
        return Ok(String::new());
    }
    // ……其余原 render 函数体原样保留（lines/sep/循环/输出）……
    Ok(output.trim_end().to_string())
}
```

（即：原函数体中 `let stdin_data = ...` 与 `let data = ...` 两行移入 `render`，其余全部移入 `render_with_data`，并把 `&data` 形参替换原局部 `data`。）

- [ ] **Step 2: 写 doctor 失败测试**

创建 `src/doctor.rs`：

```rust
use crate::compact;
use crate::core::config::AppConfig;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::widget::WidgetRegistry;

/// Run all self-checks, print a report, and return Err with the failure
/// count when any check fails (main exits non-zero).
pub fn run(
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> Result<(), String> {
    let mut failures = 0usize;

    let exe = std::env::current_exe().unwrap_or_default();
    failures += check(
        "binary",
        true,
        &format!("{} (v{})", exe.display(), env!("CARGO_PKG_VERSION")),
        "",
    );
    failures += check(
        "config.toml",
        AppConfig::load().is_ok(),
        "parses",
        "run 'claude-hud setup' to create it",
    );
    failures += check(
        "statusLine configured",
        status_line_ok(),
        "points at claude-hud render",
        "run 'claude-hud setup' to merge it into ~/.claude/settings.json",
    );
    failures += check("icon set", true, &format!("{:?}", theme.icon_set), "");

    match crate::probe::git::probe_git() {
        Some(s) => println!("  [ok] git: branch '{}' readable", s.branch),
        None => println!("  [..] git: unavailable or not a repo (widget degrades silently)"),
    }

    failures += check(
        "sample render",
        sample_render(registry, config, theme).is_ok(),
        "renders without panic",
        "check 'claude-hud render' with real stdin JSON",
    );

    if failures == 0 {
        println!("All checks passed.");
        Ok(())
    } else {
        Err(format!("{} check(s) failed — see hints above", failures))
    }
}

fn status_line_ok() -> bool {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return false,
    };
    let path = home.join(".claude").join("settings.json");
    match std::fs::read_to_string(path) {
        Ok(content) => content.contains("claude-hud render"),
        Err(_) => false,
    }
}

fn sample_render(
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> Result<String, String> {
    let sample = serde_json::json!({
        "model": {"id": "test", "display_name": "Test"},
        "context_window": {
            "used_percentage": 50,
            "total_input_tokens": 1000,
            "context_window_size": 200000
        },
        "cost": {"total_cost_usd": 0.1, "total_duration_ms": 60000}
    });
    let data = SessionData::from_stdin_json(&sample.to_string())
        .map_err(|e| format!("parse sample JSON: {}", e))?;
    compact::render_with_data(&data, registry, config, theme)
}

fn check(label: &str, ok: bool, ok_detail: &str, hint: &str) -> usize {
    if ok {
        println!("  [ok] {}: {}", label, ok_detail);
    } else {
        println!("  [!!] {}: fix: {}", label, hint);
    }
    usize::from(!ok)
}
```

（`report` 的 `detail` 同时充当错误提示文本，保持函数少而简单；`git` 检查项输出探测结果但不算失败——git 缺失只是降级，非错误。若你觉得 `status_line_ok` 等行为需调整，在实施时直接改。）

- [ ] **Step 3: 接入 main.rs**

`src/main.rs` 顶部 `mod compact;` 附近追加 `mod doctor;`；`enum Commands` 的 `Uninstall,` 后追加：

```rust
    /// Run self-checks and print a health report
    Doctor,
```

dispatch 中 `Commands::Uninstall => run_uninstall(),` 后追加：

```rust
        Commands::Doctor => doctor::run(&registry, &config, &theme),
```

- [ ] **Step 4: 编译并测试**

Run: `cargo build`
Expected: 编译通过。

Run: `cargo run -- doctor`
Expected: 输出 6 行 `[ok]`/`[!!]` 报告；config.toml 缺失时 `[!!] config.toml` 并 exit 1。

- [ ] **Step 5: Checkpoint（用户手动提交）**

```bash
git add src/compact.rs src/doctor.rs src/main.rs
git commit -m "feat(cli): add doctor self-check command; extract render_with_data"
```

---

### Task 4: `IconSet::Auto` 零依赖字体决议（TDD）

**Files:**
- Modify: `src/core/theme.rs`
- Modify: `src/main.rs`
- Modify: `src/widgets/model_display.rs`
- Modify: `src/widgets/skills_mcp.rs`
- Modify: `src/widgets/skills_mcp_dynamic.rs`

- [ ] **Step 1: 写失败测试**

`src/core/theme.rs` 末尾追加测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_icon_set_is_auto() {
        assert!(matches!(Theme::default().icon_set, IconSet::Auto));
    }

    #[test]
    fn auto_with_nerd_font_resolves_nerd() {
        let theme = Theme::default();
        assert!(matches!(theme.resolve_icon_set_with(true), IconSet::Nerd));
    }

    #[test]
    fn auto_without_nerd_font_resolves_minimal() {
        let theme = Theme::default();
        assert!(matches!(theme.resolve_icon_set_with(false), IconSet::Minimal));
    }

    #[test]
    fn explicit_nerd_is_never_downgraded() {
        let mut theme = Theme::default();
        theme.icon_set = IconSet::Nerd;
        assert!(matches!(theme.resolve_icon_set_with(false), IconSet::Nerd));
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test theme::tests`
Expected: 编译错误（`IconSet::Auto` 不存在 / `resolve_icon_set_with` 不存在）。

- [ ] **Step 3: 实现 Auto 变体与决议**

`src/core/theme.rs` 的 `IconSet` 枚举改为：

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IconSet {
    Auto,
    Nerd,
    Ascii,
    Minimal,
}
```

`default_icon_set` 改为：

```rust
fn default_icon_set() -> IconSet { IconSet::Auto }
```

`impl Theme` 内（`interpolate_hex` 之后）追加：

```rust
    /// Resolve Auto to a concrete set using the real font probe.
    pub fn resolve_icon_set(&self) -> IconSet {
        self.resolve_icon_set_with(detect_nerd_font())
    }

    /// Pure resolution: Auto → Nerd iff a Nerd Font is installed,
    /// otherwise Minimal. Explicit choices are never downgraded.
    pub fn resolve_icon_set_with(&self, has_nerd_font: bool) -> IconSet {
        match self.icon_set {
            IconSet::Auto => {
                if has_nerd_font {
                    IconSet::Nerd
                } else {
                    IconSet::Minimal
                }
            }
            other => other,
        }
    }
```

`impl Theme` 外、模块末尾追加平台探测器（三个平台 + 兜底）：

```rust
/// Probe whether a Nerd Font is installed. Never panics: any probe
/// failure means "no Nerd Font" so callers fall back to Minimal.
pub fn detect_nerd_font() -> bool {
    detect_nerd_font_platform()
}

#[cfg(target_os = "windows")]
fn detect_nerd_font_platform() -> bool {
    let output = std::process::Command::new("reg")
        .args(["query", r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts"])
        .output();
    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .to_lowercase()
            .contains("nerd"),
        Err(_) => false,
    }
}

#[cfg(target_os = "linux")]
fn detect_nerd_font_platform() -> bool {
    let output = std::process::Command::new("fc-list").output();
    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .to_lowercase()
            .contains("nerd"),
        Err(_) => false,
    }
}

#[cfg(target_os = "macos")]
fn detect_nerd_font_platform() -> bool {
    let mut dirs: Vec<std::path::PathBuf> = vec![
        "/System/Library/Fonts".into(),
        "/Library/Fonts".into(),
    ];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("Library").join("Fonts"));
    }
    dirs.iter().any(|dir| {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .any(|e| e.file_name().to_string_lossy().to_lowercase().contains("nerd"))
            })
            .unwrap_or(false)
    })
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn detect_nerd_font_platform() -> bool { false }
```

- [ ] **Step 4: main.rs 加载时决议一次**

`src/main.rs` 中：

```rust
    let theme = load_theme(&config);
```

改为：

```rust
    let mut theme = load_theme(&config);
    theme.icon_set = theme.resolve_icon_set();
```

- [ ] **Step 5: 补 3 处穷举匹配（编译要求）**

`src/widgets/model_display.rs` 约 18-20 行：

```rust
        let (icon, suffix) = match theme.icon_set {
            IconSet::Auto => ("> ", ""),   // 防御分支：main 已决议，正常不可达
            IconSet::Nerd | IconSet::Minimal => ("▸ ", ""),
            IconSet::Ascii => ("[", "]"),
        };
```

`src/widgets/skills_mcp.rs` 约 19-23 行：

```rust
        let si = match theme.icon_set {
            IconSet::Auto => "◇ ",         // 防御分支：正常不可达
            IconSet::Nerd => "🧩 ", IconSet::Ascii => "[SK] ", IconSet::Minimal => "◇ ",
        };
        let mi = match theme.icon_set {
            IconSet::Auto => "◆ ",         // 防御分支：正常不可达
            IconSet::Nerd => "🔌 ", IconSet::Ascii => "[MC] ", IconSet::Minimal => "◆ ",
        };
```

`src/widgets/skills_mcp_dynamic.rs` 约 103/107 行两处：

```rust
    match theme.icon_set {
        IconSet::Auto => "◇",             // 防御分支：正常不可达
        IconSet::Nerd => "🧩", IconSet::Ascii => "[SK]", IconSet::Minimal => "◇" }
    match theme.icon_set {
        IconSet::Auto => "◆",             // 防御分支：正常不可达
        IconSet::Nerd => "🔌", IconSet::Ascii => "[MC]", IconSet::Minimal => "◆" }
```

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo test`
Expected: 全部 PASS（含 Task 1 的 cc_config 测试）。

Run: `cargo run -- doctor`
Expected: `icon set` 行显示 `Nerd` 或 `Minimal`（由本机字体决定），不会是 `Auto`。

- [ ] **Step 7: Checkpoint（用户手动提交）**

```bash
git add src/core/theme.rs src/main.rs src/widgets/model_display.rs src/widgets/skills_mcp.rs src/widgets/skills_mcp_dynamic.rs
git commit -m "feat(theme): add icon_set auto-detection with zero-dependency fallback"
```

---

### Task 5: `git_status` 抽纯函数 + 单测（TDD）

**Files:**
- Modify: `src/widgets/git_status.rs`

- [ ] **Step 1: 写失败测试**

`src/widgets/git_status.rs` 末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::default()
    }

    #[test]
    fn none_status_renders_placeholder() {
        let out = render_git_status(None, &theme());
        assert!(out.contains("—"));
    }

    #[test]
    fn some_status_renders_branch() {
        let s = GitStatus { branch: "main".into(), is_dirty: false, ahead: 0, behind: 0 };
        let out = render_git_status(Some(&s), &theme());
        assert!(out.contains("main"));
        assert!(!out.contains("*"));
    }

    #[test]
    fn dirty_ahead_renders_markers() {
        let s = GitStatus { branch: "dev".into(), is_dirty: true, ahead: 2, behind: 1 };
        let out = render_git_status(Some(&s), &theme());
        assert!(out.contains("*"));
        assert!(out.contains("↑2"));
        assert!(out.contains("↓1"));
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test git_status`
Expected: 编译错误（`render_git_status` 不存在）。

- [ ] **Step 3: 抽出纯函数并接线**

`src/widgets/git_status.rs` 中，把 `render_compact` 的渲染逻辑抽出为模块级函数（放 `impl Widget` 之前），并让 `render_compact` 调用它：

```rust
/// Render the compact git status text. None (no git repo / no git
/// binary) renders a muted placeholder instead of failing.
pub fn render_git_status(status: Option<&GitStatus>, theme: &Theme) -> String {
    let mut parts = vec![];
    if let Some(ref s) = status {
        parts.push(ansi::ansi_fg(&s.branch, &theme.accent));
        if s.is_dirty {
            parts.push(ansi::ansi_fg("*", &theme.warning));
        }
        if s.ahead > 0 {
            parts.push(ansi::ansi_fg(&format!("↑{}", s.ahead), &theme.muted));
        }
        if s.behind > 0 {
            parts.push(ansi::ansi_fg(&format!("↓{}", s.behind), &theme.muted));
        }
    } else {
        parts.push(ansi::ansi_fg("—", &theme.muted));
    }
    parts.join(" ")
}
```

`render_compact` 函数体改为：

```rust
    fn render_compact(&self, _data: &SessionData, theme: &Theme, _config: &WidgetConfig) -> String {
        let status = crate::probe::git::probe_git();
        let output = render_git_status(status.as_ref(), theme);
        if let Ok(ref mut guard) = self.cached.lock() {
            **guard = status;
        }
        output
    }
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test git_status`
Expected: 3 个测试 PASS。

- [ ] **Step 5: Checkpoint（用户手动提交）**

```bash
git add src/widgets/git_status.rs
git commit -m "test(widgets): extract git_status rendering for graceful-degradation tests"
```

---

### Task 6: `plugin.json` 轻量注册

**Files:**
- Create: `plugin.json`

- [ ] **Step 1: 创建文件**

创建 `plugin.json`：

```json
{
  "name": "claude-hud",
  "version": "0.1.0",
  "description": "Dual-mode terminal HUD for Claude Code: compact status bar + full-screen TUI dashboard with agent observability, token attribution, and theme engine",
  "author": "your-github-username",
  "license": "MIT",
  "repository": "https://github.com/your-username/claude-hud",
  "keywords": ["statusline", "dashboard", "monitoring", "tui", "themes"],
  "categories": ["developer-tools", "monitoring"],
  "platforms": ["macos", "linux", "windows"],
  "commands": {
    "claude-hud:dashboard": {
      "command": "claude-hud dashboard",
      "description": "Open full-screen TUI dashboard"
    },
    "claude-hud:web": {
      "command": "claude-hud serve",
      "description": "Start web dashboard on localhost:9527"
    },
    "claude-hud:doctor": {
      "command": "claude-hud doctor",
      "description": "Run self-checks and print a health report"
    }
  }
}
```

注意：**不含** `installation`/`setup`/`defaultSettings`/`hooks` 字段——statusLine 由一键脚本管理，避免双写冲突；插件市场渠道仅做命令注册。

- [ ] **Step 2: 替换占位符并校验**

把 `author` 与 `repository` 中的 `your-github-username` / `your-username` 替换为真实 GitHub 用户名（与发布仓库一致）。

Run: `python -m json.tool plugin.json`（或 `jq . plugin.json`）
Expected: 无语法错误输出。

- [ ] **Step 3: Checkpoint（用户手动提交）**

```bash
git add plugin.json
git commit -m "feat(plugin): add lightweight plugin.json registering slash commands only"
```

---

### Task 7: Unix 安装/卸载脚本

**Files:**
- Create: `scripts/install.sh`
- Create: `scripts/uninstall.sh`

- [ ] **Step 1: 创建目录**

Run: `mkdir -p scripts`

- [ ] **Step 2: 创建 install.sh**

创建 `scripts/install.sh`：

```bash
#!/usr/bin/env bash
# Claude HUD installer for macOS / Linux
set -euo pipefail

REPO="${HUD_REPO:-user/claude-hud}"          # 发布前替换为真实仓库（如 yourname/claude-hud）
INSTALL_DIR="${HUD_INSTALL_DIR:-${HOME}/.local/bin}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)  TARGET="linux-x64" ;;
  Darwin-x86_64) TARGET="macos-x64" ;;
  Darwin-arm64)  TARGET="macos-arm64" ;;
  *) echo "error: unsupported platform $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

mkdir -p "$INSTALL_DIR"
echo "Installing Claude HUD (${TARGET}) ..."

if [ -n "${HUD_LOCAL_BIN:-}" ]; then
  # 本地安装模式（开发/CI 冒烟）：不访问网络
  cp "$HUD_LOCAL_BIN" "$INSTALL_DIR/claude-hud"
  chmod +x "$INSTALL_DIR/claude-hud"
else
  LATEST="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)"
  [ -n "$LATEST" ] || { echo "error: cannot resolve latest release of ${REPO}" >&2; exit 1; }

  if [ -f "$INSTALL_DIR/version.txt" ] \
      && [ "$(cat "$INSTALL_DIR/version.txt")" = "$LATEST" ]; then
    echo "claude-hud ${LATEST} already installed — nothing to do."
    exit 0
  fi

  curl -fsSL "https://github.com/${REPO}/releases/download/${LATEST}/claude-hud-${TARGET}.tar.gz" \
    | tar xz -C "$INSTALL_DIR" claude-hud
  chmod +x "$INSTALL_DIR/claude-hud"
  printf '%s\n' "$LATEST" > "$INSTALL_DIR/version.txt"
fi

if ! echo ":$PATH:" | grep -q ":$INSTALL_DIR:"; then
  case "${SHELL:-}" in
    *zsh*) RC_FILE="${ZDOTDIR:-${HOME}}/.zshrc" ;;
    *)     RC_FILE="${HOME}/.bashrc" ;;
  esac
  if ! grep -qF "export PATH=\"$INSTALL_DIR" "$RC_FILE" 2>/dev/null; then
    printf '\nexport PATH="%s:$PATH"\n' "$INSTALL_DIR" >> "$RC_FILE"
  fi
  echo "Added $INSTALL_DIR to PATH in $RC_FILE (restart terminal or source it)"
fi

"$INSTALL_DIR/claude-hud" setup

echo
echo "Done! Verify:"
echo '  echo '"'"'{"model":{"id":"test","display_name":"Test"},"context_window":{"used_percentage":50},"cost":{"total_cost_usd":0.1,"total_duration_ms":60000}}'"'"' | claude-hud render'
echo '  Restart Claude Code or run /reload-plugins to see the HUD status bar.'
```

- [ ] **Step 3: 创建 uninstall.sh**

创建 `scripts/uninstall.sh`：

```bash
#!/usr/bin/env bash
# Claude HUD uninstaller for macOS / Linux
set -euo pipefail

INSTALL_DIR="${HUD_INSTALL_DIR:-${HOME}/.local/bin}"
BIN="$INSTALL_DIR/claude-hud"

# 1. 先摘掉 statusLine 并删除配置目录（二进制内置逻辑），
#    避免 Claude Code 每 5 秒调用已删除的命令
if [ -x "$BIN" ]; then
  "$BIN" uninstall || echo "warning: claude-hud uninstall reported an issue" >&2
fi

# 2. 移除安装脚本追加的 PATH 行（精确匹配，仅删该行）
for RC_FILE in "${HOME}/.bashrc" "${ZDOTDIR:-${HOME}}/.zshrc"; do
  if [ -f "$RC_FILE" ]; then
    sed -i.bak "\|export PATH=\"${INSTALL_DIR}:\$PATH\"|d" "$RC_FILE" || true
    rm -f "$RC_FILE.bak"
  fi
done

# 3. 删除二进制与版本标记
rm -f "$BIN" "$INSTALL_DIR/version.txt"
if [ -f "$BIN" ] || [ -f "$INSTALL_DIR/version.txt" ]; then
  echo "warning: some files could not be removed — delete them manually" >&2
else
  echo "Removed $BIN"
fi

echo "Claude HUD uninstalled."
```

- [ ] **Step 4: 语法检查**

Run: `bash -n scripts/install.sh && bash -n scripts/uninstall.sh`
Expected: 无输出（语法通过）。

- [ ] **Step 5: 本地冒烟（临时 HOME，不碰真实环境）**

```bash
set -euo pipefail
TMP="$(mktemp -d)"
mkdir -p "$TMP/.local/bin"
printf '#!/bin/sh\necho "fake-claude-hud"\n' > "$TMP/.local/bin/claude-hud"
chmod +x "$TMP/.local/bin/claude-hud"
HOME="$TMP" HUD_LOCAL_BIN="$TMP/.local/bin/claude-hud" bash scripts/install.sh
grep -q "$TMP/.local/bin" "$TMP/.bashrc"
HOME="$TMP" HUD_INSTALL_DIR="$TMP/.local/bin" bash scripts/uninstall.sh
test ! -f "$TMP/.local/bin/claude-hud"
echo "UNIX SMOKE OK"
rm -rf "$TMP"
```

Expected: 末尾输出 `UNIX SMOKE OK`。

- [ ] **Step 6: Checkpoint（用户手动提交）**

```bash
git add scripts/install.sh scripts/uninstall.sh
git commit -m "feat(scripts): add Unix one-line install/uninstall scripts"
```

---

### Task 8: Windows 安装/卸载脚本

**Files:**
- Create: `scripts/install.ps1`
- Create: `scripts/uninstall.ps1`

- [ ] **Step 1: 创建 install.ps1**

创建 `scripts/install.ps1`：

```powershell
# Claude HUD installer for Windows (no admin required)
$ErrorActionPreference = 'Stop'

$Repo = $env:HUD_REPO
if (-not $Repo) { $Repo = 'user/claude-hud' }   # 发布前替换为真实仓库

$InstallDir = Join-Path $env:LOCALAPPDATA 'claude-hud\bin'
New-Item -ItemType Directory -Force $InstallDir | Out-Null
Write-Host "Installing Claude HUD to $InstallDir ..."

$LocalStub = $env:HUD_LOCAL_STUB
if ($LocalStub) {
    # 本地安装模式（开发/CI 冒烟）：不访问网络
    Copy-Item $LocalStub (Join-Path $InstallDir 'claude-hud.cmd') -Force
} else {
    $Release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
    $Tag = $Release.tag_name
    $VersionFile = Join-Path $InstallDir 'version.txt'
    if ((Test-Path $VersionFile) -and ((Get-Content $VersionFile -Raw).Trim() -eq $Tag)) {
        Write-Host "claude-hud $($Tag.Replace('v','')) already installed - nothing to do."
        exit 0
    }
    $Zip = Join-Path $env:TEMP 'claude-hud-windows.zip'
    Invoke-WebRequest "https://github.com/$Repo/releases/download/$Tag/claude-hud-windows-x64.zip" -OutFile $Zip
    Expand-Archive $Zip -DestinationPath $InstallDir -Force
    Set-Content -Path $VersionFile -Value $Tag -Encoding ascii
}

# PATH（HKCU 用户级，免管理员）
$UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable('Path', "$UserPath;$InstallDir", 'User')
    Write-Host "Added $InstallDir to user PATH (new terminal windows pick it up)."
}

$Bin = Join-Path $InstallDir 'claude-hud.exe'
if (-not (Test-Path $Bin)) { $Bin = Join-Path $InstallDir 'claude-hud.cmd' }
& $Bin setup

Write-Host ''
Write-Host 'Done! Verify:'
Write-Host '  echo {"model":{"id":"test","display_name":"Test"},"context_window":{"used_percentage":50},"cost":{"total_cost_usd":0.1,"total_duration_ms":60000}} | claude-hud render'
Write-Host '  Restart Claude Code or run /reload-plugins to see the HUD status bar.'
```

- [ ] **Step 2: 创建 uninstall.ps1**

创建 `scripts/uninstall.ps1`：

```powershell
# Claude HUD uninstaller for Windows
$ErrorActionPreference = 'Stop'

$InstallDir = Join-Path $env:LOCALAPPDATA 'claude-hud\bin'
$Bin = Join-Path $InstallDir 'claude-hud.exe'
if (-not (Test-Path $Bin)) { $Bin = Join-Path $InstallDir 'claude-hud.cmd' }

# 1. 先摘掉 statusLine 并删除配置目录，避免 Claude Code 继续调用已删除命令
if (Test-Path $Bin) {
    & $Bin uninstall
}

# 2. 移除 PATH 条目（逐段精确匹配）
$UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($UserPath -like "*$InstallDir*") {
    $NewPath = ($UserPath -split ';' | Where-Object { $_ -and $_ -ne $InstallDir }) -join ';'
    [Environment]::SetEnvironmentVariable('Path', $NewPath, 'User')
    Write-Host "Removed $InstallDir from user PATH."
}

# 3. 删除安装目录（二进制可能被占用，失败则提示手动删除）
$removed = $false
for ($i = 0; $i -lt 3; $i++) {
    Remove-Item $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
    if (-not (Test-Path $InstallDir)) { $removed = $true; break }
    Start-Sleep -Milliseconds 300
}
if ($removed) {
    Write-Host "Removed $InstallDir"
} else {
    Write-Host "warning: $InstallDir could not be fully removed - delete it manually" -ForegroundColor Yellow
}

Write-Host 'Claude HUD uninstalled.'
```

- [ ] **Step 3: PowerShell 语法检查**

Run: `pwsh -NoProfile -Command "Get-Command scripts/install.ps1 | Out-Null; [System.Management.Automation.PSParser]::Tokenize((Get-Content -Raw scripts/install.ps1), [ref]\$null) | Out-Null; [System.Management.Automation.PSParser]::Tokenize((Get-Content -Raw scripts/uninstall.ps1), [ref]\$null) | Out-Null; 'PS SYNTAX OK'"`
Expected: 输出 `PS SYNTAX OK`（若本机无 pwsh，跳过本步，由 Task 9 的 CI 兜底）。

- [ ] **Step 4: 本地冒烟（临时 LOCALAPPDATA，不碰真实环境）**

```powershell
$env:LOCALAPPDATA = Join-Path $env:TEMP ('hudtest-' + [guid]::NewGuid())
$stub = Join-Path $env:TEMP 'hud-stub.cmd'
"@echo off`r`necho fake-claude-hud" | Set-Content -Encoding ascii $stub
$env:HUD_LOCAL_STUB = $stub
& ./scripts/install.ps1
$UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($UserPath -notlike "*claude-hud*") { throw 'PATH not updated' }
& ./scripts/uninstall.ps1
$UserPath2 = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($UserPath2 -like "*claude-hud*") { throw 'PATH not cleaned' }
Write-Host 'WIN SMOKE OK'
```

Expected: 末尾输出 `WIN SMOKE OK`。

- [ ] **Step 5: Checkpoint（用户手动提交）**

```bash
git add scripts/install.ps1 scripts/uninstall.ps1
git commit -m "feat(scripts): add Windows one-line install/uninstall scripts"
```

---

### Task 9: CI — 矩阵构建 + 脚本检查 + 冒烟测试

**Files:**
- Create: `.github/workflows/release.yml`

- [ ] **Step 1: 创建工作流**

创建 `.github/workflows/release.yml`：

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
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - name: Build
        run: cargo build --release --target ${{ matrix.target }}
      - name: Package (Unix)
        if: matrix.os != 'windows-latest'
        run: |
          cd target/${{ matrix.target }}/release
          tar -czf claude-hud-${{ matrix.name }}.tar.gz claude-hud
      - name: Package (Windows)
        if: matrix.os == 'windows-latest'
        shell: pwsh
        run: |
          Compress-Archive target/${{ matrix.target }}/release/claude-hud.exe claude-hud-${{ matrix.name }}.zip
      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: claude-hud-${{ matrix.name }}
          path: claude-hud-${{ matrix.name }}.*

  shellcheck:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install shellcheck
        run: sudo apt-get update && sudo apt-get install -y shellcheck
      - name: Syntax + lint
        run: |
          bash -n scripts/install.sh scripts/uninstall.sh
          shellcheck scripts/install.sh scripts/uninstall.sh

  smoke-unix:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install/uninstall smoke test
        run: |
          set -euo pipefail
          TMP="$(mktemp -d)"
          mkdir -p "$TMP/.local/bin"
          printf '#!/bin/sh\necho "fake-claude-hud"\n' > "$TMP/.local/bin/claude-hud"
          chmod +x "$TMP/.local/bin/claude-hud"
          HOME="$TMP" HUD_LOCAL_BIN="$TMP/.local/bin/claude-hud" bash scripts/install.sh
          grep -q "$TMP/.local/bin" "$TMP/.bashrc"
          HOME="$TMP" HUD_INSTALL_DIR="$TMP/.local/bin" bash scripts/uninstall.sh
          test ! -f "$TMP/.local/bin/claude-hud"
          echo "UNIX SMOKE OK"

  smoke-windows:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install/uninstall smoke test
        shell: pwsh
        run: |
          $env:LOCALAPPDATA = Join-Path $env:RUNNER_TEMP 'hudtest'
          $stub = Join-Path $env:RUNNER_TEMP 'hud-stub.cmd'
          "@echo off`r`necho fake-claude-hud" | Set-Content -Encoding ascii $stub
          $env:HUD_LOCAL_STUB = $stub
          ./scripts/install.ps1
          $UserPath = [Environment]::GetEnvironmentVariable('Path','User')
          if ($UserPath -notlike "*claude-hud*") { throw "PATH not updated" }
          ./scripts/uninstall.ps1
          if (Test-Path (Join-Path $env:LOCALAPPDATA 'claude-hud')) { throw "install dir not removed" }
          $UserPath2 = [Environment]::GetEnvironmentVariable('Path','User')
          if ($UserPath2 -like "*claude-hud*") { throw "PATH not cleaned" }
          Write-Host "WIN SMOKE OK"

  release:
    needs: [build, shellcheck, smoke-unix, smoke-windows]
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@v4
        with:
          path: artifacts
          merge-multiple: true
      - uses: softprops/action-gh-release@v2
        with:
          name: "Claude HUD ${{ github.ref_name }}"
          body_path: CHANGELOG.md
          files: |
            artifacts/claude-hud-macos-x64.tar.gz
            artifacts/claude-hud-macos-arm64.tar.gz
            artifacts/claude-hud-linux-x64.tar.gz
            artifacts/claude-hud-windows-x64.zip
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

- [ ] **Step 2: 本地校验 YAML 结构**

Run: `python -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml')); print('YAML OK')"`（无 PyYAML 则用 `ruby -e 'require "yaml"; YAML.load_file(".github/workflows/release.yml"); puts "YAML OK"'`）
Expected: 输出 `YAML OK`。

- [ ] **Step 3: Checkpoint（用户手动提交）**

```bash
git add .github/workflows/release.yml
git commit -m "ci: add release matrix build with script checks and smoke tests"
```

---

### Task 10: 文档（README 新建 + DEPLOY.md 更新）

**Files:**
- Create: `README.md`
- Modify: `DEPLOY.md`

- [ ] **Step 1: 创建 README.md**

创建 `README.md`（覆盖安装/使用/卸载主线；完整功能清单可引用既有 DEPLOY.md）：

```markdown
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
curl -fsSL https://raw.githubusercontent.com/<user>/claude-hud/main/scripts/install.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/<user>/claude-hud/main/scripts/install.ps1 | iex
```

The installer downloads a prebuilt binary, adds it to PATH, and configures the Claude Code status line. Restart Claude Code or run `/reload-plugins` to see the HUD.

## Uninstall

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/<user>/claude-hud/main/scripts/uninstall.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/<user>/claude-hud/main/scripts/uninstall.ps1 | iex
```

## Usage

```bash
claude-hud doctor          # self-check: config, status line, icons, git
claude-hud mod use @daily  # switch UI preset (daily/night/agent/ssh/mini)
claude-hud mod pick        # interactive mod picker
claude-hud dashboard       # full-screen TUI dashboard (q/Esc to exit)
claude-hud serve           # web dashboard at localhost:9527
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
```

- [ ] **Step 2: 更新 DEPLOY.md**

把 "## 快速开始" 整节（第 9-55 行，含 Rust 安装、编译、一键配置、验证）替换为：

```markdown
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

检查 PATH、config.toml、statusLine 配置、图标集决议、git 可用性与样例渲染，输出 ✅/⚠️/❌ 报告。
```

同时更新 "CLI 命令参考" 基础命令表，追加两行：

```markdown
| `claude-hud doctor` | 自检：配置/状态行/图标/git/渲染健康报告 |
| `claude-hud uninstall` | 移除 statusLine 与配置目录（卸载脚本内部调用） |
```

并将 DEPLOY.md 中 "配置文件" 一节开头补一句：`icon_set` 默认 `auto`，无 Nerd Font 时自动降级为 minimal 图标，无需手动配置。

- [ ] **Step 3: 一致性检查**

Run: `grep -n "icon_set = \"auto\"\|IconSet::Auto\|icon_set.*auto" DEPLOY.md README.md docs/superpowers/specs/2026-07-31-install-ux-simplify-design.md`
Expected: 至少 1 处命中（说明文档间一致）。检查两处 `<user>` 占位符是否已替换为真实用户名。

- [ ] **Step 4: Checkpoint（用户手动提交）**

```bash
git add README.md DEPLOY.md
git commit -m "docs: add README with one-line install/uninstall; update DEPLOY quick start"
```

---

## 收尾验证

- [ ] `cargo test` 全绿（Tasks 1/4/5 的单测）
- [ ] `cargo run -- doctor` 输出健康报告无 panic
- [ ] `bash -n scripts/install.sh scripts/uninstall.sh` 通过（Task 7 冒烟输出 `UNIX SMOKE OK`）
- [ ] Windows 冒烟输出 `WIN SMOKE OK`（本机有 pwsh 时）
- [ ] `plugin.json` 通过 JSON 校验
- [ ] 真实 GitHub 用户名已替换进 `scripts/install.sh`、`scripts/install.ps1`、`plugin.json`、`README.md`、`DEPLOY.md`（5 处 `user/claude-hud` 或 `<user>` 占位符）
- [ ] 用户完成所有 Checkpoint 提交

## 计划自查

- **Spec 覆盖**：§2 预编译 CI→Task 9；§3 安装脚本→Task 7/8；§4 卸载脚本→Task 7/8；§5 setup 合并→Task 1/2；§6 uninstall 命令→Task 2；§7 doctor→Task 3；§8a Auto 图标→Task 4；§8b git 降级→Task 5；§9 plugin.json→Task 6；§10 测试→各任务 + Task 9 冒烟；§11 文件清单→Task 1-10 全部命中；§12 YAGNI 边界→无越界任务。
- **占位符**：`user/claude-hud`/`your-username` 为真实仓库配置值，在 Task 6 Step 2、Task 7 Step 2、Task 8 Step 1、Task 10 Step 3 均有显式替换步骤；计划本身无 "TBD/TODO/implement later"。
- **类型一致性**：`merge_status_line`/`remove_status_line`（Task 1 定义，Task 2 调用）、`render_with_data`（Task 3 定义，doctor 调用）、`resolve_icon_set_with`/`detect_nerd_font`（Task 4 定义，main 调用）、`render_git_status`（Task 5 定义，widget 调用）——跨任务签名一致。
