use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::config::AppConfig;
use super::i18n::Language;
use super::pricing::{ModelEntry, PriceEntry};

pub const REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/cuishiying-1/claude-hud/master/registry/models.toml";

/// 测试钩子：HUD_REGISTRY_URL 环境变量覆盖拉取地址。
pub fn registry_url() -> String {
    std::env::var("HUD_REGISTRY_URL").unwrap_or_else(|_| REGISTRY_URL.to_string())
}

/// 注册表文件（registry/models.toml）：registry_version + [models] 段。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegistryFile {
    pub registry_version: String,
    #[serde(default)]
    pub models: HashMap<String, ModelEntry>,
}

use std::path::Path;

/// 拉取注册表（ureq，10s 超时，UA 与 update.rs 一致）。失败带原因返回。
pub fn fetch_registry(url: &str) -> Result<String, String> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(10))
        .set("User-Agent", "claude-hud")
        .call()
        .map_err(|e| format!("fetch registry: {}", e))?;
    resp.into_string()
        .map_err(|e| format!("read registry response: {}", e))
}

/// 解析校验注册表：TOML 合法、registry_version 非空、窗口 >0、价格 ≥0。
/// 失败带模型定位错误，调用方不写任何文件。
pub fn parse_registry(content: &str) -> Result<RegistryFile, String> {
    let reg: RegistryFile =
        toml::from_str(content).map_err(|e| format!("parse registry: {}", e))?;
    if reg.registry_version.trim().is_empty() {
        return Err("registry_version missing".to_string());
    }
    for (id, entry) in &reg.models {
        if entry.context_window == Some(0) {
            return Err(format!("model {}: context_window must be > 0", id));
        }
        for (cur, p) in [("usd", &entry.price_usd), ("cny", &entry.price_cny)] {
            if let Some(p) = p {
                if p.input < 0.0 || p.output < 0.0 || p.cache_read < 0.0 || p.cache_creation < 0.0 {
                    return Err(format!("model {}: negative price in {}", id, cur));
                }
            }
        }
    }
    Ok(reg)
}

/// 把注册表条目合并进 config.toml 的 [models] 段（toml::Value 手术，保留
/// 其他配置段与用户手写条目）：远程覆盖/补齐本地，写 synced_at 时间戳；
/// 不删除本地任何条目。返回被更新（含新增）的模型 id。
pub fn merge_into_config(
    config_path: &Path,
    reg: &RegistryFile,
    synced_at: &str,
) -> Result<Vec<String>, String> {
    let existing = std::fs::read_to_string(config_path).unwrap_or_default();
    let mut root: toml::Value = if existing.trim().is_empty() {
        toml::Value::Table(toml::Table::new())
    } else {
        toml::from_str(&existing).map_err(|e| format!("parse config.toml: {}", e))?
    };
    let models = root
        .as_table_mut()
        .ok_or("config.toml must be a table")?
        .entry("models")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let table = models
        .as_table_mut()
        .ok_or("config.toml [models] must be a table")?;
    let mut updated = Vec::new();
    for (id, entry) in &reg.models {
        let mut e = entry.clone();
        e.synced_at = Some(synced_at.to_string());
        let v = toml::Value::try_from(&e).map_err(|err| format!("serialize model {}: {}", id, err))?;
        table.insert(id.clone(), v);
        updated.push(id.clone());
    }
    let out = toml::to_string_pretty(&root).map_err(|e| format!("serialize config: {}", e))?;
    std::fs::write(config_path, out).map_err(|e| format!("write config: {}", e))?;
    Ok(updated)
}

/// ~/.claude/settings.json（与 doctor.rs status_line_ok 同路径解析）。
pub fn settings_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("cannot find home directory")?;
    Ok(home.join(".claude").join("settings.json"))
}

/// sync 路径注入（测试用临时目录，CLI 用真实路径）。
#[derive(Debug, Clone)]
pub struct SyncPaths {
    pub config: PathBuf,
    pub settings: PathBuf,
}

/// sync 结果（CLI 打印用，测试断言用）。
#[derive(Debug)]
pub struct SyncResult {
    pub version: String,
    pub updated: Vec<String>,
    pub env_written: bool,
    pub env_failed: Option<String>,
}

/// `model sync` 入口（CLI 用）：URL 来自 HUD_REGISTRY_URL 钩子或默认地址。
pub fn run_sync(
    paths: &SyncPaths,
    lang: Language,
    prompt: &mut dyn FnMut(&str) -> bool,
) -> Result<SyncResult, String> {
    run_sync_at(&registry_url(), paths, lang, prompt)
}

/// `model sync` 主流程（URL 显式传入，测试用本地 tiny_http 地址）：
/// 拉取 → 解析校验 → 合并写 config.toml → 逐模型询问 env（prompt 可注入，
/// 测试传 |_| false 跳过）。任何失败不写任何文件。
fn run_sync_at(
    url: &str,
    paths: &SyncPaths,
    lang: Language,
    prompt: &mut dyn FnMut(&str) -> bool,
) -> Result<SyncResult, String> {
    let body = fetch_registry(url)?;
    let reg = parse_registry(&body)?;
    let synced_at = now_iso();
    let updated = merge_into_config(&paths.config, &reg, &synced_at)?;
    let mut env_written = false;
    let mut env_failed: Option<String> = None;
    for id in &updated {
        let Some(window) = reg.models.get(id).and_then(|m| m.context_window) else {
            continue;
        };
        let ask = super::i18n::tr(lang, "runtime.model_sync_env_prompt")
            .replace("{id}", id)
            .replace("{window}", &window.to_string());
        if !prompt(&ask) {
            continue;
        }
        let content = std::fs::read_to_string(&paths.settings).unwrap_or_default();
        let merged = crate::core::cc_config::set_env_window(&content, window)
            .map_err(|e| format!("prepare env: {}", e))?;
        match crate::core::state::write_atomic(&paths.settings, &merged) {
            Ok(()) => env_written = true,
            Err(e) => env_failed = Some(e),
        }
    }
    Ok(SyncResult { version: reg.registry_version, updated, env_written, env_failed })
}

/// `model env [<window>|off]`：查看 / 设置 / 清除 settings.json env。
pub fn model_env_cmd(
    _config: &AppConfig,
    arg: Option<&str>,
    lang: Language,
) -> Result<(), String> {
    let settings = settings_path()?;
    let content = std::fs::read_to_string(&settings).unwrap_or_default();
    match arg {
        None => match crate::core::cc_config::get_env_window(&content) {
            Some(v) => println!("{}", super::i18n::tr(lang, "runtime.model_env_view").replace("{window}", &v)),
            None => println!("{}", super::i18n::tr(lang, "runtime.model_env_none")),
        },
        Some("off") => {
            let updated = crate::core::cc_config::remove_env_window(&content)?;
            crate::core::state::write_atomic(&settings, &updated)?;
            println!("{}", super::i18n::tr(lang, "runtime.model_env_off"));
        }
        Some(w) => {
            let window: u64 = w
                .parse()
                .map_err(|_| super::i18n::tr(lang, "runtime.model_env_bad").to_string())?;
            let updated = crate::core::cc_config::set_env_window(&content, window)?;
            crate::core::state::write_atomic(&settings, &updated)?;
            println!("{}", super::i18n::tr(lang, "runtime.model_env_set").replace("{window}", w));
        }
    }
    Ok(())
}

/// `model list`：合并视图（内置 + config，config 覆盖），标注来源/窗口/双币种。
pub fn model_list_cmd(config: &AppConfig, lang: Language) -> Result<(), String> {
    let builtin = super::pricing::builtin_models();
    let mut ids: Vec<String> = Vec::new();
    for id in config.models.keys().chain(builtin.keys()) {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids.sort();
    for id in ids {
        let cfg = config.models.get(&id);
        let entry = builtin.get(&id);
        let source = match cfg {
            Some(m) if m.synced_at.is_some() => super::i18n::tr(lang, "runtime.model_src_synced"),
            Some(_) => super::i18n::tr(lang, "runtime.model_src_user"),
            None => super::i18n::tr(lang, "runtime.model_src_builtin"),
        };
        let window = cfg
            .and_then(|m| m.context_window)
            .or_else(|| entry.and_then(|m| m.context_window));
        let usd = cfg
            .and_then(|m| m.price_usd.as_ref())
            .or_else(|| entry.and_then(|m| m.price_usd.as_ref()))
            .map(fmt_price)
            .unwrap_or_else(|| "—".to_string());
        let cny = cfg
            .and_then(|m| m.price_cny.as_ref())
            .or_else(|| entry.and_then(|m| m.price_cny.as_ref()))
            .map(fmt_price)
            .unwrap_or_else(|| "—".to_string());
        let synced = cfg.and_then(|m| m.synced_at.clone()).unwrap_or_default();
        println!(
            "{}",
            super::i18n::tr(lang, "runtime.model_list_line")
                .replace("{id}", &id)
                .replace("{source}", source)
                .replace(
                    "{window}",
                    &window.map(|w| w.to_string()).unwrap_or_else(|| "—".to_string())
                )
                .replace("{usd}", &usd)
                .replace("{cny}", &cny)
                .replace("{synced}", &synced)
        );
    }
    Ok(())
}

fn fmt_price(p: &PriceEntry) -> String {
    format!("{:.2}/{:.2}", p.input * 1e6, p.output * 1e6)
}

/// ISO 时间戳（chrono 无 clock feature，复用 state.rs 模式）。
pub fn now_iso() -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(super::state::now_secs() as i64, 0)
        .map(|d| d.to_rfc3339())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"
registry_version = "2026-08-05"
[models."deepseek-v4-flash"]
context_window = 1000000
[models."deepseek-v4-flash".price_usd]
input = 0.14e-6
output = 0.28e-6
cache_read = 0.0028e-6
cache_creation = 0.175e-6
[models."deepseek-v4-flash".price_cny]
input = 1.0e-6
output = 2.0e-6
cache_read = 0.02e-6
cache_creation = 1.25e-6
"#;

    #[test]
    fn parse_valid_dual_currency_fixture() {
        let reg = parse_registry(FIXTURE).unwrap();
        assert_eq!(reg.registry_version, "2026-08-05");
        let e = reg.models.get("deepseek-v4-flash").unwrap();
        assert_eq!(e.context_window, Some(1_000_000));
        assert!(e.price_usd.is_some() && e.price_cny.is_some());
    }

    #[test]
    fn parse_missing_optional_fields_ok() {
        let reg = parse_registry("registry_version = \"2026-08-05\"\n[models.\"m\"]\ncontext_window = 8000\n").unwrap();
        let e = reg.models.get("m").unwrap();
        assert_eq!(e.context_window, Some(8000));
        assert!(e.price_usd.is_none() && e.price_cny.is_none());
    }

    #[test]
    fn parse_zero_window_rejected() {
        let err = parse_registry("registry_version = \"2026-08-05\"\n[models.\"m\"]\ncontext_window = 0\n").unwrap_err();
        assert!(err.contains("context_window must be > 0"), "err: {}", err);
    }

    #[test]
    fn parse_negative_price_rejected() {
        let err = parse_registry("registry_version = \"2026-08-05\"\n[models.\"m\"]\n[models.\"m\".price_cny]\ninput = -1.0e-6\n").unwrap_err();
        assert!(err.contains("negative price"), "err: {}", err);
    }

    #[test]
    fn parse_invalid_toml_rejected() {
        assert!(parse_registry("registry_version = [").is_err());
    }

    #[test]
    fn parse_missing_version_rejected() {
        assert!(parse_registry("[models.\"m\"]\n").unwrap_err().contains("registry_version"));
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hud-ms-{}-{}", tag, std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d
    }

    #[test]
    fn merge_preserves_user_entries_and_other_sections() {
        let dir = temp_dir("merge1");
        let cfg_path = dir.join("config.toml");
        std::fs::write(
            &cfg_path,
            "[pricing.\"my-model\"]\ninput = 1.0e-6\noutput = 2.0e-6\n[models.\"user-hand\"]\ncontext_window = 1000\n",
        )
        .unwrap();
        let reg = parse_registry(FIXTURE).unwrap();
        let updated = merge_into_config(&cfg_path, &reg, "2026-08-05T00:00:00Z").unwrap();
        assert_eq!(updated, vec!["deepseek-v4-flash".to_string()]);
        let re: toml::Value = toml::from_str(&std::fs::read_to_string(&cfg_path).unwrap()).unwrap();
        assert_eq!(re["models"]["deepseek-v4-flash"]["context_window"], toml::Value::Integer(1_000_000));
        assert_eq!(re["models"]["deepseek-v4-flash"]["synced_at"], toml::Value::String("2026-08-05T00:00:00Z".into()));
        assert_eq!(re["models"]["user-hand"]["context_window"], toml::Value::Integer(1000), "user entry kept");
        assert_eq!(re["pricing"]["my-model"]["input"], toml::Value::Float(1.0e-6), "pricing section kept");
    }

    #[test]
    fn merge_overwrites_existing_registry_entry() {
        let dir = temp_dir("merge2");
        let cfg_path = dir.join("config.toml");
        std::fs::write(
            &cfg_path,
            "[models.\"deepseek-v4-flash\"]\ncontext_window = 200000\n",
        )
        .unwrap();
        let reg = parse_registry(FIXTURE).unwrap();
        merge_into_config(&cfg_path, &reg, "2026-08-05T00:00:00Z").unwrap();
        let re: toml::Value = toml::from_str(&std::fs::read_to_string(&cfg_path).unwrap()).unwrap();
        assert_eq!(re["models"]["deepseek-v4-flash"]["context_window"], toml::Value::Integer(1_000_000), "overwritten");
    }

    #[test]
    fn fetch_failure_returns_err() {
        assert!(fetch_registry("http://127.0.0.1:1/registry.toml").is_err());
    }

    #[test]
    fn registry_url_respects_env_hook() {
        std::env::set_var("HUD_REGISTRY_URL", "http://127.0.0.1:9999/x");
        assert_eq!(registry_url(), "http://127.0.0.1:9999/x");
        std::env::remove_var("HUD_REGISTRY_URL");
        assert_eq!(registry_url(), REGISTRY_URL);
    }

    fn local_server() -> String {
        // tiny_http 本地服务模拟 GitHub 注册表，返回 "http://127.0.0.1:PORT"
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = server.server_addr().to_string();
        let fixture = FIXTURE.to_string();
        std::thread::spawn(move || {
            if let Ok(req) = server.recv() {
                let _ = req.respond(tiny_http::Response::from_string(fixture));
            }
        });
        format!("http://{}/registry.toml", addr)
    }

    #[test]
    fn sync_prompt_no_writes_config_only() {
        let url = local_server();
        let dir = temp_dir("e2e1");
        let config_path = dir.join("config.toml");
        std::fs::write(&config_path, "[pricing.\"x\"]\ninput = 1.0e-6\n").unwrap();
        let settings_path = dir.join("settings.json");
        let paths = SyncPaths { config: config_path.clone(), settings: settings_path.clone() };
        let result = run_sync_at(&url, &paths, Language::En, &mut |_| false).unwrap();
        assert_eq!(result.version, "2026-08-05");
        assert_eq!(result.updated, vec!["deepseek-v4-flash".to_string()]);
        assert!(!result.env_written);
        assert!(result.env_failed.is_none());
        assert!(!settings_path.exists(), "env prompt declined → settings untouched");
        let re: toml::Value = toml::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert!(re["models"]["deepseek-v4-flash"]["synced_at"].is_str(), "synced_at written");
    }

    #[test]
    fn sync_prompt_yes_writes_env() {
        let url = local_server();
        let dir = temp_dir("e2e2");
        let paths = SyncPaths {
            config: dir.join("config.toml"),
            settings: dir.join("settings.json"),
        };
        let result = run_sync_at(&url, &paths, Language::En, &mut |_| true).unwrap();
        assert!(result.env_written);
        let content = std::fs::read_to_string(&paths.settings).unwrap();
        assert!(content.contains("CLAUDE_CODE_MAX_CONTEXT_TOKENS"));
        assert!(content.contains("1000000"));
    }

    #[test]
    fn repo_registry_file_parses_and_matches_builtin() {
        let content = std::fs::read_to_string("registry/models.toml")
            .expect("registry/models.toml exists");
        let reg = parse_registry(&content).unwrap();
        assert_eq!(reg.models.len(), 11, "9 claude + 2 deepseek");
        let builtin = super::super::pricing::builtin_models();
        // 内置表 cache 价由乘法推算（如 0.3e-6*0.1 = 3.0000000000000004e-7），
        // 与文件手写精确值存在浮点噪声 → 逐字段容差比较。
        let close = |a: &super::super::pricing::PriceEntry,
                     b: &super::super::pricing::PriceEntry| {
            (a.input - b.input).abs() < 1e-15
                && (a.output - b.output).abs() < 1e-15
                && (a.cache_read - b.cache_read).abs() < 1e-15
                && (a.cache_creation - b.cache_creation).abs() < 1e-15
        };
        for (id, e) in &reg.models {
            let b = builtin.get(id).expect("registry id in builtin");
            assert_eq!(e.context_window, b.context_window, "{id} window");
            match (&e.price_usd, &b.price_usd) {
                (Some(a), Some(b)) => assert!(close(a, b), "{id} usd"),
                (None, None) => {}
                _ => panic!("{id} usd presence mismatch"),
            }
            match (&e.price_cny, &b.price_cny) {
                (Some(a), Some(b)) => assert!(close(a, b), "{id} cny"),
                (None, None) => {}
                _ => panic!("{id} cny presence mismatch"),
            }
        }
    }

    #[test]
    fn sync_fetch_failure_returns_err_and_writes_nothing() {
        let dir = temp_dir("e2e3");
        let config_path = dir.join("config.toml");
        std::fs::write(&config_path, "[pricing.\"x\"]\ninput = 1.0e-6\n").unwrap();
        let paths = SyncPaths { config: config_path.clone(), settings: dir.join("settings.json") };
        let err = run_sync_at("http://127.0.0.1:1/registry.toml", &paths, Language::En, &mut |_| false).unwrap_err();
        assert!(err.contains("fetch"), "err: {}", err);
        assert_eq!(
            std::fs::read_to_string(&config_path).unwrap(),
            "[pricing.\"x\"]\ninput = 1.0e-6\n",
            "config untouched on failure"
        );
    }
}
