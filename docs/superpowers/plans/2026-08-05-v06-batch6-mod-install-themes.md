# v0.6 批次 VI — mod install 插件市场 + 主题预设 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完成 v0.6 最后批次 VI：⑰ `mod install <user/repo>`（GitHub `mods/` 目录整目录安装 + 供应链警告 + 联动激活）与 ⑳ 四个新主题预设（6 → 10），全量验证后文档收尾。

**Architecture:** 新增独立模块 `src/core/mod_install.rs`：纯函数（repo 参数/contents 过滤/名字校验/script 检测）+ 注入 fetch（`FetchError` 区分 404 与网络错误）+ 两阶段编排（`fetch_mods` 拉取校验 → `write_mods` 落盘报告）。CLI 在 `main.rs` 接线，激活复用既有 `write_active_mod` + `StateFile` 路径。`theme.rs` 沿用硬编码预设函数模式扩 4 个。i18n en/zh 双表新增 key。

**Tech Stack:** Rust + clap 4 + serde/toml + serde_json + ureq（既有依赖，零新增）；Python 黑盒套件（scripts/test_hud.py）。

**提交纪律（用户硬性约束）：** 本计划**不含 git commit 步骤**——用户禁止自动 `git add/commit/push`；批次完成且全量验证通过后，由用户批量授权提交（只 stage 批次 VI 实现文件；**绝不**包含用户并行工作文件 compact.rs/config.rs/pricing.rs/widget.rs/context_bar.rs/cost_display.rs，**绝不**包含 fixtures/、reports/、docs/superpowers/）。

**环境纪律：** 所有 cargo 命令前缀 `export PATH="$HOME/.cargo/bin:$PATH" &&`；**禁止**运行 `cargo fmt`。

**验证路径（全量）：** `cargo test`（单测 209 → 预计 ~236）+ `python scripts/test_hud.py`（黑盒 191 → 194）。

---

## 文件结构

| 文件 | 责任 |
|------|------|
| 新建 `src/core/mod_install.rs` | 纯函数 + fetch 注入编排 + 报告结构 + 单测（Task 1/2） |
| 修改 `src/core/mod.rs` | 注册 `pub mod mod_install;` |
| 修改 `src/main.rs` | `ModCommands::Install` + `handle_mod` 分支 + `inject_help` |
| 修改 `locales/en.toml` / `locales/zh.toml` | 6 个新 key |
| 修改 `src/core/theme.rs` | 4 个预设 fn + `preset_names`/`load_preset` + 单测 |
| 修改 `scripts/hudlib/cases.py` | P8 3 例 + CASES 191 → 194 |
| 修改 `CHANGELOG.md` / `DEPLOY.md` / `COMPLETE.md` / `DESIGN.md` | 文档收尾 |

**不触碰：** `src/core/config.rs`（用户并行工作文件，只读引用 `AppConfig::mods_dir`/`ModPackage`）、`src/compact.rs`、`src/core/pricing.rs`、`src/widgets/widget.rs`、`src/widgets/context_bar.rs`、`src/widgets/cost_display.rs`。

---

### Task 1: mod_install 纯函数层（repo 解析 / contents 过滤 / 名字校验 / script 检测）

**Files:**
- Create: `src/core/mod_install.rs`
- Modify: `src/core/mod.rs`（注册模块）

- [ ] **Step 1: 创建模块骨架 + 全部测试（stub 签名，测试先行）**

写入 `src/core/mod_install.rs`：

```rust
use std::collections::HashMap;
use std::path::Path;

use crate::core::config::ModPackage;

/// 与内置出厂 mod 冲突则跳过（内置优先，用户 mod 同名永不生效）。
pub const BUILTIN_MODS: &[&str] = &[
    "glacier-workstation",
    "obsidian-command",
    "ember-night",
    "matrix-surveillance",
    "noir-precision",
    "noir-tabbed",
];

/// 校验 <user>/<repo>：恰好一个 '/'、无协议前缀、无空白、无 '..'。
pub fn parse_repo_arg(input: &str) -> Result<(String, String), String> {
    todo!("implement in Step 3")
}

/// GitHub contents API JSON → type=file 且 .toml 结尾的文件名，按字典序升序。
pub fn filter_mod_entries(json: &str) -> Vec<String> {
    todo!("implement in Step 3")
}

/// mod_info.name 落盘安全校验：非空、≤64 字符、仅 [A-Za-z0-9._-]、非内置名。
pub fn validate_mod_name(name: &str) -> Result<(), String> {
    todo!("implement in Step 3")
}

/// widgets 表任一条目 type ∈ {rhai_script, shell_output, http_output} → 激活即执行远程代码。
pub fn contains_script_widget(pkg: &ModPackage) -> bool {
    todo!("implement in Step 3")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::ModInfo;

    fn widget_with_type(t: &str) -> HashMap<String, toml::Value> {
        let mut table = toml::map::Map::new();
        table.insert("type".to_string(), toml::Value::String(t.to_string()));
        let mut widgets = HashMap::new();
        widgets.insert("w".to_string(), toml::Value::Table(table));
        widgets
    }

    fn pkg_with(widgets: HashMap<String, toml::Value>) -> ModPackage {
        ModPackage {
            mod_info: ModInfo::default(),
            layout: None,
            compact_widgets: None,
            animation: None,
            widgets,
        }
    }

    #[test]
    fn parse_repo_ok() {
        assert_eq!(
            parse_repo_arg("user/repo"),
            Ok(("user".to_string(), "repo".to_string()))
        );
    }

    #[test]
    fn parse_repo_no_slash() {
        assert!(parse_repo_arg("user").is_err());
    }

    #[test]
    fn parse_repo_too_many_slashes() {
        assert!(parse_repo_arg("a/b/c").is_err());
    }

    #[test]
    fn parse_repo_protocol_prefix() {
        assert!(parse_repo_arg("https://github.com/a/b").is_err());
    }

    #[test]
    fn parse_repo_whitespace() {
        assert!(parse_repo_arg("a b/c").is_err());
    }

    #[test]
    fn parse_repo_empty_parts() {
        assert!(parse_repo_arg("/repo").is_err());
        assert!(parse_repo_arg("user/").is_err());
        assert!(parse_repo_arg("").is_err());
    }

    #[test]
    fn filter_entries_keeps_toml_files_sorted() {
        let json = r#"[
            {"name":"zeta.toml","type":"file"},
            {"name":"alpha.toml","type":"file"},
            {"name":"sub","type":"dir"},
            {"name":"readme.md","type":"file"}
        ]"#;
        assert_eq!(
            filter_mod_entries(json),
            vec!["alpha.toml".to_string(), "zeta.toml".to_string()]
        );
    }

    #[test]
    fn filter_entries_empty_and_invalid() {
        assert!(filter_mod_entries("[]").is_empty());
        assert!(filter_mod_entries("not json").is_empty());
        assert!(filter_mod_entries(r#"[{"name":"a.toml"}]"#).is_empty());
    }

    #[test]
    fn validate_name_ok() {
        assert!(validate_mod_name("my-mod_1.x").is_ok());
    }

    #[test]
    fn validate_name_rejects_path_and_space() {
        assert!(validate_mod_name("a/b").is_err());
        assert!(validate_mod_name("..").is_err());
        assert!(validate_mod_name("a b").is_err());
        assert!(validate_mod_name("").is_err());
    }

    #[test]
    fn validate_name_rejects_too_long() {
        assert!(validate_mod_name(&"x".repeat(65)).is_err());
        assert!(validate_mod_name(&"x".repeat(64)).is_ok());
    }

    #[test]
    fn validate_name_rejects_builtin() {
        assert!(validate_mod_name("glacier-workstation").is_err());
    }

    #[test]
    fn script_widget_types_detected() {
        for t in ["rhai_script", "shell_output", "http_output"] {
            assert!(
                contains_script_widget(&pkg_with(widget_with_type(t))),
                "{} should be detected",
                t
            );
        }
    }

    #[test]
    fn plain_widget_not_script() {
        assert!(!contains_script_widget(&pkg_with(widget_with_type("agent_overview"))));
    }

    #[test]
    fn empty_widgets_not_script() {
        assert!(!contains_script_widget(&pkg_with(HashMap::new())));
    }
}
```

- [ ] **Step 2: 注册模块并跑测试（期望 FAIL）**

在 `src/core/mod.rs` 的 `pub mod` 列表中追加一行（保持字母序，在 `i18n` 之后）：

```rust
pub mod mod_install;
```

运行并确认失败：

```bash
export PATH="$HOME/.cargo/bin:$PATH" && cargo test mod_install 2>&1 | tail -20
```

Expected: 编译通过但 17 个测试全部 FAIL（`todo!()` panic）。若编译报错（如 `toml::map::Map` 路径不对），修正后再继续。

- [ ] **Step 3: 实现纯函数（替换 Step 1 的 `todo!()`）**

```rust
pub fn parse_repo_arg(input: &str) -> Result<(String, String), String> {
    let s = input.trim();
    if s.is_empty()
        || s.contains(char::is_whitespace)
        || s.contains("://")
        || s.contains("..")
    {
        return Err("expected <user>/<repo>".to_string());
    }
    let mut parts = s.split('/');
    let user = parts.next().unwrap_or_default();
    let repo = parts.next().unwrap_or_default();
    if user.is_empty() || repo.is_empty() || parts.next().is_some() {
        return Err("expected <user>/<repo>".to_string());
    }
    Ok((user.to_string(), repo.to_string()))
}

pub fn filter_mod_entries(json: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    let mut names: Vec<String> = arr
        .iter()
        .filter_map(|e| {
            if e.get("type").and_then(|v| v.as_str()) != Some("file") {
                return None;
            }
            let name = e.get("name").and_then(|v| v.as_str())?;
            if name.ends_with(".toml") {
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect();
    names.sort();
    names
}

pub fn validate_mod_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err("invalid mod name".to_string());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err("invalid mod name".to_string());
    }
    if BUILTIN_MODS.contains(&name) {
        return Err("conflicts with built-in mod".to_string());
    }
    Ok(())
}

pub fn contains_script_widget(pkg: &ModPackage) -> bool {
    pkg.widgets.values().any(|v| {
        matches!(
            v.get("type").and_then(|t| t.as_str()),
            Some("rhai_script") | Some("shell_output") | Some("http_output")
        )
    })
}
```

- [ ] **Step 4: 跑测试（期望 PASS）**

```bash
export PATH="$HOME/.cargo/bin:$PATH" && cargo test mod_install 2>&1 | tail -8
```

Expected: `test result: ok. 17 passed`。

---

### Task 2: fetch_mods / write_mods 编排 + FetchError + fetch_http

**Files:**
- Modify: `src/core/mod_install.rs`（追加编排层 + 测试）

- [ ] **Step 1: 追加编排层测试（stub 先行）**

在 `src/core/mod_install.rs` 的 `#[cfg(test)] mod tests` 内、`use super::*` 之后追加：

```rust
    use std::collections::HashMap as Map;

    fn list_url(user: &str, repo: &str) -> String {
        format!(
            "https://api.github.com/repos/{}/{}/contents/mods",
            user, repo
        )
    }

    fn raw_url(user: &str, repo: &str, file: &str) -> String {
        format!(
            "https://raw.githubusercontent.com/{}/{}/HEAD/mods/{}",
            user, repo, file
        )
    }

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "hud-mod-install-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn fetch_mods_happy_path_sorted_and_script_flag() {
        let mut r: Map<String, Result<String, FetchError>> = Map::new();
        r.insert(
            list_url("u", "r"),
            Ok(r#"[{"name":"b.toml","type":"file"},{"name":"a.toml","type":"file"}]"#.into()),
        );
        r.insert(
            raw_url("u", "r", "a.toml"),
            Ok("[mod_info]\nname = \"alpha\"\n".into()),
        );
        r.insert(
            raw_url("u", "r", "b.toml"),
            Ok("[mod_info]\nname = \"beta\"\n[widgets.sys]\ntype = \"shell_output\"\ncommand = \"uptime\"\n".into()),
        );
        let fetch = |url: &str| {
            r.get(url)
                .cloned()
                .unwrap_or_else(|| Err(FetchError::Other("unexpected url".into())))
        };
        let (mods, skipped) = fetch_mods(&fetch, "u/r").unwrap();
        assert_eq!(mods.len(), 2);
        assert_eq!(mods[0].name, "alpha");
        assert!(!mods[0].has_script);
        assert_eq!(mods[1].name, "beta");
        assert!(mods[1].has_script);
        assert!(skipped.is_empty());
    }

    #[test]
    fn fetch_mods_list_404() {
        let fetch = |url: &str| -> Result<String, FetchError> {
            if url.contains("/contents/mods") {
                Err(FetchError::NotFound)
            } else {
                unreachable!()
            }
        };
        let err = fetch_mods(&fetch, "u/r").unwrap_err();
        assert!(err.contains("no mods/ directory"), "got: {}", err);
    }

    #[test]
    fn fetch_mods_list_network_error() {
        let fetch = |url: &str| -> Result<String, FetchError> {
            Err(FetchError::Other("timeout".into()))
        };
        let err = fetch_mods(&fetch, "u/r").unwrap_err();
        assert!(err.contains("unavailable"), "got: {}", err);
    }

    #[test]
    fn fetch_mods_empty_dir() {
        let fetch = |url: &str| -> Result<String, FetchError> {
            if url.contains("/contents/mods") {
                Ok("[]".into())
            } else {
                unreachable!()
            }
        };
        let err = fetch_mods(&fetch, "u/r").unwrap_err();
        assert!(err.contains("no mods found"), "got: {}", err);
    }

    #[test]
    fn fetch_mods_partial_failure_skips() {
        let mut r: Map<String, Result<String, FetchError>> = Map::new();
        r.insert(
            list_url("u", "r"),
            Ok(r#"[{"name":"a.toml","type":"file"},{"name":"b.toml","type":"file"},{"name":"c.toml","type":"file"}]"#.into()),
        );
        r.insert(
            raw_url("u", "r", "a.toml"),
            Ok("[mod_info]\nname = \"alpha\"\n".into()),
        );
        // b.toml 拉取失败（缺路由 → Other("fetch failed")）
        r.insert(raw_url("u", "r", "c.toml"), Ok("not toml".into()));
        let fetch = |url: &str| {
            r.get(url)
                .cloned()
                .unwrap_or_else(|| Err(FetchError::Other("fetch failed".into())))
        };
        let (mods, skipped) = fetch_mods(&fetch, "u/r").unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].name, "alpha");
        assert_eq!(skipped.len(), 2);
        assert!(skipped.iter().any(|(f, _)| f == "b.toml"));
        assert!(skipped.iter().any(|(f, _)| f == "c.toml"));
    }

    #[test]
    fn fetch_mods_all_failed() {
        let mut r: Map<String, Result<String, FetchError>> = Map::new();
        r.insert(
            list_url("u", "r"),
            Ok(r#"[{"name":"a.toml","type":"file"}]"#.into()),
        );
        r.insert(raw_url("u", "r", "a.toml"), Err(FetchError::Other("boom".into())));
        let fetch = |url: &str| {
            r.get(url)
                .cloned()
                .unwrap_or_else(|| Err(FetchError::Other("fetch failed".into())))
        };
        let err = fetch_mods(&fetch, "u/r").unwrap_err();
        assert!(err.contains("no mods installed"), "got: {}", err);
    }

    #[test]
    fn fetch_mods_bad_repo_arg_never_fetches() {
        let fetch = |url: &str| -> Result<String, FetchError> {
            let _ = url;
            unreachable!("fetch must not be called for bad repo arg")
        };
        assert!(fetch_mods(&fetch, "nope").is_err());
        assert!(fetch_mods(&fetch, "").is_err());
    }

    #[test]
    fn write_mods_reports_installed_updated_activated() {
        let dir = tmp_dir("write1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("alpha.toml"), "old").unwrap();
        let parsed = vec![
            ParsedMod { file: "a.toml".into(), name: "alpha".into(), content: "new-alpha".into(), has_script: false },
            ParsedMod { file: "b.toml".into(), name: "beta".into(), content: "beta-body".into(), has_script: false },
        ];
        let report = write_mods(&parsed, &dir);
        assert_eq!(report.updated, vec!["alpha".to_string()]);
        assert_eq!(report.installed, vec!["beta".to_string()]);
        assert_eq!(report.activated.as_deref(), Some("beta"));
        assert_eq!(
            std::fs::read_to_string(dir.join("alpha.toml")).unwrap(),
            "new-alpha"
        );
    }

    #[test]
    fn write_mods_activated_is_lexicographic_max() {
        let dir = tmp_dir("write2");
        std::fs::create_dir_all(&dir).unwrap();
        let parsed = vec![
            ParsedMod { file: "x.toml".into(), name: "zeta".into(), content: "1".into(), has_script: false },
            ParsedMod { file: "y.toml".into(), name: "alpha".into(), content: "2".into(), has_script: false },
        ];
        let report = write_mods(&parsed, &dir);
        assert_eq!(report.installed, vec!["zeta".to_string(), "alpha".to_string()]);
        assert_eq!(report.activated.as_deref(), Some("zeta"));
    }

    #[test]
    fn write_mods_dir_blocked_marks_all_skipped() {
        let parent = tmp_dir("write3");
        std::fs::create_dir_all(&parent).unwrap();
        let conflict = parent.join("blocker");
        std::fs::write(&conflict, "i am a file").unwrap();
        let parsed = vec![ParsedMod {
            file: "a.toml".into(),
            name: "alpha".into(),
            content: "x".into(),
            has_script: false,
        }];
        let report = write_mods(&parsed, &conflict);
        assert!(report.installed.is_empty());
        assert!(!report.skipped.is_empty());
        assert_eq!(report.activated, None);
    }
```

- [ ] **Step 2: 跑测试（期望 FAIL — 类型未定义）**

```bash
export PATH="$HOME/.cargo/bin:$PATH" && cargo test mod_install 2>&1 | tail -12
```

Expected: 编译错误（`FetchError`/`ParsedMod`/`InstallReport`/`fetch_mods`/`write_mods` 未定义）。

- [ ] **Step 3: 实现编排层（追加在 `contains_script_widget` 之后、`#[cfg(test)]` 之前）**

```rust
/// fetch 错误：区分「资源不存在（404）」与「网络/其他」。
#[derive(Debug, Clone, PartialEq)]
pub enum FetchError {
    NotFound,
    Other(String),
}

/// 真实网络 fetch（ureq；User-Agent claude-hud / 10s timeout，与 update.rs 同形状）。
pub fn fetch_http(url: &str) -> Result<String, FetchError> {
    let resp = ureq::get(url)
        .set("User-Agent", "claude-hud")
        .timeout(std::time::Duration::from_secs(10))
        .call();
    match resp {
        Ok(r) => r.into_string().map_err(|e| FetchError::Other(e.to_string())),
        Err(ureq::Error::Status(404, _)) => Err(FetchError::NotFound),
        Err(e) => Err(FetchError::Other(e.to_string())),
    }
}

/// 通过校验、等待落盘的 mod。
#[derive(Debug, Clone)]
pub struct ParsedMod {
    /// mods/ 目录中的原始文件名（报告/跳过明细用）。
    pub file: String,
    /// 校验后的 mod_info.name（落盘文件名，已通过安全校验）。
    pub name: String,
    pub content: String,
    pub has_script: bool,
}

/// 落盘结果报告。
#[derive(Debug, Default)]
pub struct InstallReport {
    pub installed: Vec<String>,
    pub updated: Vec<String>,
    pub skipped: Vec<(String, String)>,
    /// 激活目标：installed+updated 中字典序最大的名字。
    pub activated: Option<String>,
}

/// Phase 1-2：列出 mods/ 目录 → 拉取全部 .toml → 解析 + 校验。
/// 返回 (通过校验的 mod, 跳过明细)；列表级失败/全部跳过 → Err（失败可见）。
pub fn fetch_mods<F>(
    fetch: &F,
    repo_arg: &str,
) -> Result<(Vec<ParsedMod>, Vec<(String, String)>), String>
where
    F: Fn(&str) -> Result<String, FetchError>,
{
    let (user, repo) = parse_repo_arg(repo_arg)?;
    let list_url = format!(
        "https://api.github.com/repos/{}/{}/contents/mods",
        user, repo
    );
    let body = match fetch(&list_url) {
        Ok(b) => b,
        Err(FetchError::NotFound) => {
            return Err(format!("no mods/ directory found in {}", repo_arg));
        }
        Err(FetchError::Other(e)) => {
            return Err(format!("mod install unavailable: {}", e));
        }
    };
    let names = filter_mod_entries(&body);
    if names.is_empty() {
        return Err(format!("no mods found in {}", repo_arg));
    }
    let mut parsed: Vec<ParsedMod> = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();
    for file in &names {
        let raw_url = format!(
            "https://raw.githubusercontent.com/{}/{}/HEAD/mods/{}",
            user, repo, file
        );
        let content = match fetch(&raw_url) {
            Ok(c) => c,
            Err(_) => {
                skipped.push((file.clone(), "fetch failed".to_string()));
                continue;
            }
        };
        match parse_and_validate(file, &content) {
            Ok(pm) => parsed.push(pm),
            Err(reason) => skipped.push((file.clone(), reason)),
        }
    }
    if parsed.is_empty() {
        let details: Vec<String> = skipped
            .iter()
            .map(|(f, r)| format!("{}: {}", f, r))
            .collect();
        return Err(format!(
            "no mods installed from {}: {}",
            repo_arg,
            details.join(", ")
        ));
    }
    Ok((parsed, skipped))
}

fn parse_and_validate(file: &str, content: &str) -> Result<ParsedMod, String> {
    let pkg: ModPackage =
        toml::from_str(content).map_err(|e| format!("parse: {}", e))?;
    validate_mod_name(&pkg.mod_info.name)?;
    Ok(ParsedMod {
        file: file.to_string(),
        name: pkg.mod_info.name,
        content: content.to_string(),
        has_script: contains_script_widget(&pkg),
    })
}

/// Phase 4：统一落盘 → 报告（不失败：写盘错误记入 skipped）。
pub fn write_mods(parsed: &[ParsedMod], mods_dir: &Path) -> InstallReport {
    let mut report = InstallReport::default();
    if std::fs::create_dir_all(mods_dir).is_err() {
        for pm in parsed {
            report
                .skipped
                .push((pm.file.clone(), "mkdir failed".to_string()));
        }
        return report;
    }
    for pm in parsed {
        let path = mods_dir.join(format!("{}.toml", pm.name));
        let existed = path.exists();
        match std::fs::write(&path, &pm.content) {
            Ok(()) => {
                if existed {
                    report.updated.push(pm.name.clone());
                } else {
                    report.installed.push(pm.name.clone());
                }
            }
            Err(e) => report
                .skipped
                .push((pm.file.clone(), format!("write: {}", e))),
        }
    }
    let mut ok: Vec<&str> = report
        .installed
        .iter()
        .chain(&report.updated)
        .map(String::as_str)
        .collect();
    ok.sort();
    report.activated = ok.last().map(|s| s.to_string());
    report
}
```

- [ ] **Step 4: 跑测试（期望 PASS）**

```bash
export PATH="$HOME/.cargo/bin:$PATH" && cargo test mod_install 2>&1 | tail -8
```

Expected: `test result: ok. 29 passed`（17 + 12）。

---

### Task 3: CLI 接线 + i18n（main.rs + locales）

**Files:**
- Modify: `src/main.rs`（`ModCommands` 枚举 + `handle_mod` 分支 + `inject_help`）
- Modify: `locales/en.toml` / `locales/zh.toml`

- [ ] **Step 1: 新增 `ModCommands::Install` 变体**

在 `src/main.rs` 的 `ModCommands::Import` 变体之后（约 121 行）追加：

```rust
    /// Install mods from a GitHub repository's mods/ directory
    Install {
        repo: String,
    },
```

- [ ] **Step 2: 新增 `handle_mod` 分支**

在 `ModCommands::Import { file } => { ... }` 分支之后、`ModCommands::Delete { name }` 之前追加：

```rust
        ModCommands::Install { repo } => {
            let (mods, skipped) = crate::core::mod_install::fetch_mods(
                &crate::core::mod_install::fetch_http,
                &repo,
            )?;
            if mods.iter().any(|m| m.has_script) {
                println!("{}", tr(lang, "runtime.mod_install_script_warning"));
            }
            let mut report =
                crate::core::mod_install::write_mods(&mods, &AppConfig::mods_dir()?);
            report.skipped.extend(skipped);
            if report.installed.is_empty() && report.updated.is_empty() {
                let details: Vec<String> = report
                    .skipped
                    .iter()
                    .map(|(f, r)| format!("{}: {}", f, r))
                    .collect();
                return Err(format!(
                    "no mods installed from {}: {}",
                    repo,
                    details.join(", ")
                ));
            }
            for name in &report.installed {
                println!(
                    "{}",
                    tr(lang, "runtime.mod_install_installed").replace("{name}", name)
                );
            }
            for name in &report.updated {
                println!(
                    "{}",
                    tr(lang, "runtime.mod_install_updated").replace("{name}", name)
                );
            }
            for (file, reason) in &report.skipped {
                println!(
                    "{}",
                    tr(lang, "runtime.mod_install_skipped")
                        .replace("{name}", file)
                        .replace("{reason}", reason)
                );
            }
            let n = report.installed.len() + report.updated.len();
            println!(
                "{}",
                tr(lang, "runtime.mod_install_summary")
                    .replace("{n}", &n.to_string())
                    .replace("{repo}", &repo)
            );
            if let Some(active) = &report.activated {
                let state_path = AppConfig::state_path()?;
                let mut st = StateFile::read(&state_path);
                st.previous_mod = Some(config.active_mod.clone());
                st.write(&state_path)
                    .map_err(|e| format!("write state: {}", e))?;
                write_active_mod(config, active)?;
                println!(
                    "{}",
                    tr(lang, "runtime.mod_switched").replace("{name}", active)
                );
            }
            Ok(())
        }
```

- [ ] **Step 3: `inject_help` 增加 install 子命令帮助**

在 `src/main.rs` `inject_help` 的 `mod` 闭包内（`mod_import` 行之后）追加：

```rust
                .mut_subcommand("install", |cc| cc.about(tr(lang, "cli.mod_install")))
```

- [ ] **Step 4: i18n en.toml 追加 key**

在 `locales/en.toml` 的 `[runtime]` 表 `mod_reset = "..."` 行之后追加：

```toml
mod_install_installed = "  {name} — installed"
mod_install_updated = "  {name} — updated"
mod_install_skipped = "  {name} — skipped: {reason}"
mod_install_summary = "Installed {n} mod(s) from {repo} (applies to all windows)"
mod_install_script_warning = "⚠ third-party mods may contain executable script widgets (rhai/shell/http) — verify the source repo before use"
```

在 `[cli]` 表 `mod_import = "Import a mod from file"` 行之后追加：

```toml
mod_install = "Install mods from a GitHub repository's mods/ directory"
```

- [ ] **Step 5: i18n zh.toml 追加 key**

在 `locales/zh.toml` 的 `[runtime]` 表 `mod_reset = "..."` 行之后追加：

```toml
mod_install_installed = "  {name} — 已安装"
mod_install_updated = "  {name} — 已更新"
mod_install_skipped = "  {name} — 跳过：{reason}"
mod_install_summary = "已从 {repo} 安装 {n} 个 Mod（全局生效）"
mod_install_script_warning = "⚠ 第三方 Mod 可能包含可执行脚本组件（rhai/shell/http）——请确认来源仓库可信"
```

在 `[cli]` 表 `mod_import = "从文件导入 Mod"` 行之后追加：

```toml
mod_install = "从 GitHub 仓库的 mods/ 目录安装 Mod"
```

> 若 zh.toml 的 `mod_import` 文案与此处不同，以 zh.toml 实际行锚点为准（新增行必须紧随对应英文 key 的 zh 译本之后）。

- [ ] **Step 6: 编译 + 手动冒烟**

```bash
export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1 | tail -5
```

Expected: 编译通过、零新 warning。然后：

```bash
export PATH="$HOME/.cargo/bin:$PATH" && target/debug/claude-hud mod install foo; echo "exit=$?"
```

Expected: `error: expected <user>/<repo>` 输出到 stderr、`exit=1`（网络前校验，零网络）。

再验证帮助文本接线：

```bash
export PATH="$HOME/.cargo/bin:$PATH" && target/debug/claude-hud mod --help 2>&1 | grep install
```

Expected: 含 `install` 与帮助文案。

---

### Task 4: 主题 4 预设（theme.rs 6 → 10）

**Files:**
- Modify: `src/core/theme.rs`

- [ ] **Step 1: 追加测试（在 `#[cfg(test)] mod tests` 内、`apply_theme_keys_char_tokens` 之后）**

```rust
    #[test]
    fn preset_names_has_ten_with_four_new() {
        let names = Theme::preset_names();
        assert_eq!(names.len(), 10);
        for n in ["gruvbox-dark", "one-dark", "github-dark", "palenight"] {
            assert!(names.contains(&n), "missing preset {}", n);
        }
    }

    #[test]
    fn new_presets_load_with_expected_colors() {
        let cases = [
            ("gruvbox-dark", "#282828", "#ebdbb2", "#fabd2f"),
            ("one-dark", "#282c34", "#abb2bf", "#61afef"),
            ("github-dark", "#0d1117", "#c9d1d9", "#58a6ff"),
            ("palenight", "#292d3e", "#a6accd", "#82aaff"),
        ];
        for (name, bg, fg, accent) in cases {
            let t = Theme::load_preset(name).expect("preset loads");
            assert_eq!(t.bg, bg, "{} bg", name);
            assert_eq!(t.fg, fg, "{} fg", name);
            assert_eq!(t.accent, accent, "{} accent", name);
        }
    }

    #[test]
    fn new_presets_not_placeholder_defaults() {
        let def = Theme::default();
        for name in ["gruvbox-dark", "one-dark", "github-dark", "palenight"] {
            let t = Theme::load_preset(name).unwrap();
            assert_ne!(t.bg, def.bg, "{} is placeholder", name);
            assert_ne!(t.accent, def.accent, "{} is placeholder", name);
        }
    }
```

- [ ] **Step 2: 跑测试（期望 FAIL）**

```bash
export PATH="$HOME/.cargo/bin:$PATH" && cargo test theme 2>&1 | tail -8
```

Expected: 新增 3 个测试 FAIL（preset_names 长度 6、load_preset 返回 None）。

- [ ] **Step 3: 实现（preset_names 扩到 10 + load_preset 加 4 分支 + 4 个预设 fn）**

`preset_names` 替换为：

```rust
    /// Return the 10 built-in preset names.
    pub fn preset_names() -> &'static [&'static str] {
        &[
            "dracula",
            "nord",
            "tokyo-night",
            "catppuccin",
            "monochrome",
            "solarized-dark",
            "gruvbox-dark",
            "one-dark",
            "github-dark",
            "palenight",
        ]
    }
```

`load_preset` 的 match 增加 4 分支：

```rust
            "gruvbox-dark" => Some(Self::gruvbox_dark()),
            "one-dark" => Some(Self::one_dark()),
            "github-dark" => Some(Self::github_dark()),
            "palenight" => Some(Self::palenight()),
```

在 `solarized_dark()` 函数之后追加 4 个预设函数：

```rust
    fn gruvbox_dark() -> Self {
        Self {
            bg: "#282828".into(), fg: "#ebdbb2".into(),
            accent: "#fabd2f".into(), success: "#b8bb26".into(),
            warning: "#d79921".into(), danger: "#fb4934".into(),
            muted: "#928374".into(), border: "#3c3836".into(),
            skill_color: "#d3869b".into(), mcp_color: "#8ec07c".into(),
            model_color: "#83a598".into(),
            ..Default::default()
        }
    }

    fn one_dark() -> Self {
        Self {
            bg: "#282c34".into(), fg: "#abb2bf".into(),
            accent: "#61afef".into(), success: "#98c379".into(),
            warning: "#e5c07b".into(), danger: "#e06c75".into(),
            muted: "#5c6370".into(), border: "#3e4451".into(),
            skill_color: "#c678dd".into(), mcp_color: "#56b6c2".into(),
            model_color: "#61afef".into(),
            ..Default::default()
        }
    }

    fn github_dark() -> Self {
        Self {
            bg: "#0d1117".into(), fg: "#c9d1d9".into(),
            accent: "#58a6ff".into(), success: "#3fb950".into(),
            warning: "#d29922".into(), danger: "#f85149".into(),
            muted: "#8b949e".into(), border: "#21262d".into(),
            skill_color: "#bc8cff".into(), mcp_color: "#39c5cf".into(),
            model_color: "#58a6ff".into(),
            ..Default::default()
        }
    }

    fn palenight() -> Self {
        Self {
            bg: "#292d3e".into(), fg: "#a6accd".into(),
            accent: "#82aaff".into(), success: "#c3e88d".into(),
            warning: "#ffcb6b".into(), danger: "#f07178".into(),
            muted: "#676e95".into(), border: "#32374d".into(),
            skill_color: "#c792ea".into(), mcp_color: "#89ddff".into(),
            model_color: "#82aaff".into(),
            ..Default::default()
        }
    }
```

- [ ] **Step 4: 跑测试（期望 PASS）**

```bash
export PATH="$HOME/.cargo/bin:$PATH" && cargo test theme 2>&1 | tail -8
```

Expected: `test result: ok.`（含新增 3 个；既有 theme 测试全部 PASS）。

---

### Task 5: 黑盒用例 3 例 + CASES 断言 194

**Files:**
- Modify: `scripts/hudlib/cases.py`

- [ ] **Step 1: 新增 P8 列表（在 P7 列表 `]` 之后）**

```python
P8 = [
    render_case("P8-01", "⑰ mod install 无斜杠拒绝", "P8",
                {"exit": 1, "stderr_contains": ["error: expected <user>/<repo>"]},
                args=["mod", "install", "foo"], config=DEFAULT_CONFIG,
                note="⑰ 网络前校验：缺 '/' → 明确错误 + exit 1，零网络"),
    render_case("P8-02", "⑰ mod install 协议前缀拒绝", "P8",
                {"exit": 1, "stderr_contains": ["error: expected <user>/<repo>"]},
                args=["mod", "install", "https://github.com/a/b"], config=DEFAULT_CONFIG,
                note="⑰ 网络前校验：协议前缀拒绝，零网络"),
    render_case("P8-03", "⑰ mod install 含空白拒绝", "P8",
                {"exit": 1, "stderr_contains": ["error: expected <user>/<repo>"]},
                args=["mod", "install", "a b/c"], config=DEFAULT_CONFIG,
                note="⑰ 网络前校验：空白拒绝，零网络"),
]
```

- [ ] **Step 2: CASES 表达式追加 P8 并更新断言**

`CASES = ...` 行追加 `+ P8`：

```python
CASES = D1 + D2 + D3 + D4 + D5 + D6 + D7 + D8 + P1 + P2 + P3 + P4 + P5 + P6 + P7 \
    + b1_cases() + b2_cases() + b3_cases() + b4_cases() + b5_cases() + b6_cases() + P8
```

注释块（`#   + 2（D6-07/14 ⑭ 周环比）= 191`）追加一行并改断言：

```python
#   + 3（P8-01..03 ⑰ mod install 网络前校验）= 194
assert len(CASES) == 194, f"expected 194 cases, got {len(CASES)}"
```

- [ ] **Step 3: 定向验证 3 例**

```bash
python scripts/test_hud.py --case P8-01 --case P8-02 --case P8-03 2>&1 | tail -12
```

Expected: 3 例 PASS（需先完成 Task 3 的 `cargo build`，runner 指向 target/debug 二进制）。若 runner 不支持 `--case` 多选，则逐条 `python scripts/test_hud.py --case P8-01`。

---

### Task 6: 全量验证 + 文档收尾

**Files:**
- Modify: `CHANGELOG.md` / `DEPLOY.md` / `COMPLETE.md` / `DESIGN.md`

- [ ] **Step 1: 全量单元测试**

```bash
export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -5
```

Expected: 全部 PASS（209 + 32 ≈ 241；以实际为准，须 ≥ 209 且零失败）。

- [ ] **Step 2: 全量黑盒**

```bash
python scripts/test_hud.py 2>&1 | tail -10
```

Expected: 194 例全绿（若出现既有 B6-02/B6-06 类 flaky，单独复跑确认与批次 VI 无关后照既有口径处理）。

- [ ] **Step 3: CHANGELOG [Unreleased] 追加批次 VI 条目**

在 `CHANGELOG.md` 的 `[Unreleased]` 段顶部追加：

```markdown
- 批次 VI 引入类：⑰ `mod install <user/repo>` 插件市场（GitHub `mods/` 目录整目录拉取安装、contents API 列目录 + raw 拉取、`mod_info.name` 落盘安全校验、script widget 供应链警告、安装后联动激活字典序最大 mod、离线校验错误 exit 1）；⑳ 主题预设 6 → 10（gruvbox-dark / one-dark / github-dark / palenight）；⑱ Homebrew tap 砍除记录在案；黑盒 194 例
```

- [ ] **Step 4: DEPLOY.md 追加 mod install 小节**

在 DEPLOY.md 的 mod 相关章节后追加：

```markdown
### mod install 插件市场（⑰，v0.6）

`claude-hud mod install <user/repo>` 从 GitHub 仓库的 `mods/` 目录整目录安装 Mod（仅 GitHub）：
contents API 列出 .toml → raw 拉取 → 解析校验 → 落盘 `mods/` → 自动激活字典序最大的新 Mod。
供应链警告：Mod 可携带脚本组件（rhai/shell/http），激活即执行；含此类组件的 Mod 会先打印警告再安装。
重跑同仓库 = 更新（同名覆盖）；单条校验失败跳过并报告，全部失败 exit 1。
```

- [ ] **Step 5: COMPLETE.md / DESIGN.md 计数同步**

```bash
grep -n "6 built-in\|6 个主题\|六个主题\|6 preset\|主题预设" COMPLETE.md DESIGN.md
```

把命中的主题预设计数文案 6 → 10（COMPLETE.md 另需：mod 命令表加 `install` 行、✅ 段落追加批次 VI、roadmap 批次 VI 标记完成）。若 grep 无命中，检查 "Theme presets" 等英文措辞后同样处理。

- [ ] **Step 6: 工作区状态核对（提交准备，不提交）**

```bash
git status --short
```

Expected: 新增 `src/core/mod_install.rs`；修改 `src/core/mod.rs`、`src/main.rs`、`src/core/theme.rs`、`locales/en.toml`、`locales/zh.toml`、`scripts/hudlib/cases.py`、`CHANGELOG.md`、`DEPLOY.md`、`COMPLETE.md`、`DESIGN.md`。**不得出现** 用户并行工作文件（compact.rs/config.rs/pricing.rs/widget.rs/context_bar.rs/cost_display.rs）或 fixtures/、reports/ 的新增改动。交给用户批量授权提交（不带 Co-Authored-By）。

---

## 自审记录

**1. Spec 覆盖：** ⑰ 全部验收点（网络前校验 3 黑盒 ✓ / 单测六态 ✓ / 覆盖语义 write_mods ✓ / 供应链警告 fetch_mods has_script + CLI 打印 ✓ / 激活联动 Task 3 ✓ / i18n ✓ / 文档 ✓）；⑳ 全部验收点（preset_names 10 ✓ / 配色断言 ✓ / 非占位 ✓ / 文档计数 ✓）；⑱ 砍除不实现 ✓。

**2. 占位符扫描：** 无 TBD/TODO（Step 1 的 `todo!()` 是 TDD 红期桩，Step 3 全部替换）；每个代码步骤含完整代码；每步含命令与期望输出。

**3. 类型一致性：** `FetchError`（NotFound/Other）在 Task 2 定义、Task 2 测试与 `fetch_http` 使用一致；`ParsedMod`/`InstallReport` 字段在 Task 2/3 引用一致；`mod_install_*` key 名在 Task 3 Step 4/5 与 handle_mod 分支一致；`write_mods(&mods, &AppConfig::mods_dir()?)` 的 `&PathBuf → &Path` 由 deref coercion 满足；`report.skipped.extend(skipped)` 两侧均为 `Vec<(String, String)>`。

**4. 风险记录：** 黑盒 runner 的 `--case` 参数行为以实际为准（Step 3 已给 fallback）；`toml::map::Map` 路径在 Step 2 校验；zh.toml 锚点行若文案不同以实际文件为准（Task 3 Step 5 已注明）。
