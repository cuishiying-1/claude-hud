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

/// GitHub contents API JSON → type=file 且 .toml 结尾的文件名，按字典序升序。
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

/// mod_info.name 落盘安全校验：非空、≤64 字符、仅 [A-Za-z0-9._-]、非内置名。
pub fn validate_mod_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 || name == "." || name == ".." {
        return Err("invalid mod name".to_string());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err("invalid mod name".to_string());
    }
    // 主题预设名同样是内置名：load_mod 先命中内置分支，同名用户 mod 永不生效
    if BUILTIN_MODS.contains(&name)
        || crate::core::theme::Theme::preset_names().contains(&name)
    {
        return Err("conflicts with built-in mod".to_string());
    }
    Ok(())
}

/// widgets 表任一条目 type ∈ {rhai_script, shell_output, http_output} → 激活即执行远程代码。
pub fn contains_script_widget(pkg: &ModPackage) -> bool {
    pkg.widgets.values().any(|v| {
        matches!(
            v.get("type").and_then(|t| t.as_str()),
            Some("rhai_script") | Some("shell_output") | Some("http_output")
        )
    })
}

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
    let name = pkg.mod_info.name.clone();
    let has_script = contains_script_widget(&pkg);
    Ok(ParsedMod {
        file: file.to_string(),
        name,
        content: content.to_string(),
        has_script,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::ModInfo;
    use std::collections::HashMap;
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
        let fetch = |_url: &str| -> Result<String, FetchError> {
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
        r.insert(
            raw_url("u", "r", "a.toml"),
            Err(FetchError::Other("boom".into())),
        );
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
        let fetch = |_url: &str| -> Result<String, FetchError> {
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
        assert_eq!(
            report.installed,
            vec!["zeta".to_string(), "alpha".to_string()]
        );
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
            theme: None,
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
        assert!(validate_mod_name("dracula").is_err(), "主题预设名也是内置名");
        assert!(validate_mod_name("nord").is_err());
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
