mod compact;
mod core;
mod dashboard;
mod doctor;
mod notify;
mod probe;
mod serve;
mod alert;
mod widgets;

use clap::{CommandFactory, Parser, Subcommand};
use compact::{ACTIVITY_WIDGETS, MINIMAL_WIDGETS};
use core::cc_config;
use core::history::HistoryStore;
use core::config::AppConfig;
use core::theme::{BorderStyle, IconSet, Theme};
use core::state::{StateFile, now_secs, write_atomic};
use core::widget::WidgetRegistry;
use std::collections::HashMap;

#[derive(Parser)]
#[command(name = "claude-hud", version)]
#[command(about = "Dual-mode terminal HUD for Claude Code")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compact mode: read stdin JSON, output ANSI status line
    Render {
        /// Debug: print stdin JSON with recognized/unknown top-level key classification
        #[arg(long)]
        dump: bool,
    },
    /// Full-screen TUI dashboard
    Dashboard,
    /// Web dashboard (HTTP server on localhost:9527)
    Serve,
    /// Auto-configure Claude Code settings.json
    Setup,
    /// Remove statusLine from Claude Code settings and delete config dir
    Uninstall,
    /// Run self-checks and print a health report
    Doctor,
    /// Mod management
    #[command(subcommand)]
    Mod(ModCommands),
    /// Theme management
    #[command(subcommand)]
    Theme(ThemeCommands),
    /// Widget management
    #[command(subcommand)]
    Widget(WidgetCommands),
    /// Generate shell completions
    Completion {
        shell: String,
    },
    /// Cross-session usage history (weekly stats, recent sessions, daily cost)
    History {
        /// ㉑ 周报五指标：会话数/成本/token 总量/最长时长/最高单会话
        #[arg(long)]
        weekly: bool,
    },
    /// Upgrade checks
    Update {
        #[command(subcommand)]
        cmd: UpdateCommands,
    },
}

#[derive(Subcommand)]
enum UpdateCommands {
    /// Check for a new release (placeholder repo: reports not published)
    Check,
}

#[derive(Subcommand)]
enum ModCommands {
    /// List all installed mods
    List,
    /// Switch to a mod
    Use {
        name: String,
    },
    /// Preview a mod without switching
    Preview {
        name: String,
    },
    /// Show current active mod
    Current,
    /// Save current config as a new mod
    Save {
        name: String,
    },
    /// Export a mod to stdout
    Export {
        name: String,
    },
    /// Import a mod from file
    Import {
        file: String,
    },
    /// Delete a user-installed mod
    Delete {
        name: String,
    },
    /// Reset to factory default mod
    Reset,
    /// Interactive mod picker
    Pick,
}

#[derive(Subcommand)]
enum ThemeCommands {
    /// Export current theme
    Export,
    /// Import a theme file
    Import { file: String },
}

#[derive(Subcommand)]
enum WidgetCommands {
    /// List available widgets
    List,
    /// Test a single widget
    Test { name: String },
}

fn main() {
    let cli = Cli::parse();

    // ⑤ 失败不再静默：解析失败 → stderr 警告 + 回退默认（doctor 可查）
    let config = match AppConfig::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[claude-hud] warning: config.toml parse failed ({}); using defaults",
                e
            );
            AppConfig::default()
        }
    };
    let mut theme = config.resolve_theme().theme;
    theme.icon_set = theme.resolve_icon_set();
    let mut registry = WidgetRegistry::new();
    widgets::register_all(&mut registry, &config);
    widgets::register_script_widgets(&mut registry, &config);

    // Inject probe results as env vars for widgets to use
    inject_probe_env();

    let result = match cli.command {
        Commands::Render { dump } => {
            if dump {
                match compact::dump_stdin() {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        let state_path = AppConfig::state_path().unwrap_or_default();
                        StateFile::write_last_error(&state_path, &e);
                        println!("{}", compact::hud_err_marker(&e));
                        Err(e)
                    }
                }
            } else {
                match compact::render(&registry, &config, &theme) {
                    Ok(output) => {
                        print!("{}", output);
                        Ok(())
                    }
                    Err(e) => {
                        // ⑬ 状态栏静默失效修复：错误写进 state.json（doctor 可查），
                        // 同时在 stdout 打印可读标记（statusLine 输出原样上屏）。
                        let state_path = AppConfig::state_path().unwrap_or_default();
                        StateFile::write_last_error(&state_path, &e);
                        println!("{}", compact::hud_err_marker(&e));
                        Err(e)
                    }
                }
            }
        }
        Commands::Dashboard => dashboard::run(&registry, &config, &theme),
        Commands::Serve => serve::run(
            Box::leak(Box::new(registry)),
            Box::leak(Box::new(config.clone())),
            Box::leak(Box::new(theme.clone())),
        ),
        Commands::Setup => run_setup(),
        Commands::Uninstall => run_uninstall(),
        Commands::Doctor => doctor::run(&registry, &config, &theme),
        Commands::Mod(cmd) => handle_mod(cmd, &config),
        Commands::Theme(cmd) => handle_theme(cmd, &config),
        Commands::Widget(cmd) => handle_widget(cmd, &registry),
        Commands::Completion { shell } => generate_completion(&shell),
        Commands::History { weekly } => run_history(&config, weekly),
        Commands::Update { cmd } => match cmd {
            UpdateCommands::Check => {
                let status = core::update::check_update();
                println!("{}", core::update::describe(&status));
                Ok(())
            }
        },
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn inject_probe_env() {
    let skill_count = probe::filesystem::count_skills();
    let mcp_count = probe::filesystem::count_mcp_servers();
    std::env::set_var("CLAUDE_HUD_SKILL_COUNT", skill_count.to_string());
    std::env::set_var("CLAUDE_HUD_MCP_COUNT", mcp_count.to_string());
}

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

/// Merge the HUD statusLine into ~/.claude/settings.json. A timestamped
/// backup (settings.json.hud.bak-<epoch>) is written only when an existing
/// statusLine or unparseable JSON would be overwritten; the fixed-name
/// json.bak is gone and .hud.bak-* is never deleted by setup/uninstall.
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

    let valid_json = serde_json::from_str::<serde_json::Value>(&original).is_ok();
    if !original.trim().is_empty() && (cc_config::has_status_line(&original) || !valid_json) {
        let backup = settings_path.with_file_name(format!(
            "settings.json.hud.bak-{}",
            now_secs()
        ));
        std::fs::write(&backup, &original)
            .map_err(|e| format!("backup settings.json: {}", e))?;
        if cc_config::has_status_line(&original) {
            println!("replacing existing statusLine (backup at {:?})", backup);
        } else {
            println!(
                "warning: settings.json is not valid JSON — original saved to {:?}; rebuilding with minimal config (restore other settings from the backup)",
                backup
            );
        }
    }

    let merged = if valid_json {
        cc_config::merge_status_line(&original)?
    } else {
        cc_config::merge_status_line("")?
    };
    write_atomic(&settings_path, &merged)?;
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
        match cc_config::remove_status_line(&content) {
            Ok(updated) => {
                if updated != content {
                    write_atomic(&settings_path, &updated)?;
                    println!("Removed statusLine from {:?}", settings_path);
                } else {
                    println!("No statusLine found in {:?}; nothing to remove", settings_path);
                }
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
    println!("Your original settings backup (if any) is at ~/.claude/settings.json.hud.bak-* — copy it back over ~/.claude/settings.json to restore.");
    println!("Done. The claude-hud binary can now be safely deleted.");
    Ok(())
}

fn handle_mod(cmd: ModCommands, config: &AppConfig) -> Result<(), String> {
    match cmd {
        ModCommands::List => {
            println!("Built-in presets:");
            for name in Theme::preset_names() {
                let active = if name == &config.active_mod { " [active]" } else { "" };
                println!("  {} ({}){}", name, "builtin", active);
            }
            println!("\nUser mods:");
            let mods_dir = AppConfig::mods_dir()?;
            if mods_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&mods_dir) {
                    for entry in entries.filter_map(|e| e.ok()) {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let name = name.strip_suffix(".toml").unwrap_or(&name);
                        let active = if name == config.active_mod { " [active]" } else { "" };
                        println!("  {} ({}){}", name, "user", active);
                    }
                }
            }
        }
        ModCommands::Use { name } => {
            if name == "-" {
                let state_path = AppConfig::state_path()?;
                let mut st = StateFile::read(&state_path);
                let prev = match st.previous_mod.clone() {
                    Some(p) => p,
                    None => return Err("no previous mod recorded (run 'mod use <name>' first)".into()),
                };
                st.previous_mod = Some(config.active_mod.clone());
                st.write(&state_path)
                    .map_err(|e| format!("write state: {}", e))?;
                write_active_mod(config, &prev)?;
                println!("Switched back to mod '{}' ✓ (applies to all windows)", prev);
                return Ok(());
            }
            // ① 校验 + ③ @scene 解析：失败不写 config
            let target = resolve_mod_target(&name)?;
            let state_path = AppConfig::state_path()?;
            let mut st = StateFile::read(&state_path);
            st.previous_mod = Some(config.active_mod.clone());
            st.write(&state_path)
                .map_err(|e| format!("write state: {}", e))?;
            write_active_mod(config, &target)?;
            println!("Switched to mod '{}' ✓ (applies to all windows)", target);
        }
        ModCommands::Preview { name } => {
            let mod_pkg = AppConfig::load_mod(&name)?;
            println!("Preview: {}", mod_pkg.mod_info.name);
            println!("  Scene: {}", mod_pkg.mod_info.scene);
            if let Some(layout) = &mod_pkg.layout {
                println!("  Layout: {} + {}", layout.compact, layout.dashboard);
            }
            if let Some(theme) = &mod_pkg.theme {
                println!("  Theme: {}", theme.preset);
            }
            if let Some(anim) = &mod_pkg.animation {
                println!(
                    "  Animation: {} (effects: {:?})",
                    anim.enabled,
                    anim.effects
                );
            }
        }
        ModCommands::Current => {
            println!("Active mod: {}", config.active_mod);
            if let Ok(mod_pkg) = AppConfig::load_mod(&config.active_mod) {
                println!("Name: {}", mod_pkg.mod_info.name);
                println!("Description: {}", mod_pkg.mod_info.description);
                println!("Scene: {}", mod_pkg.mod_info.scene);
            }
        }
        ModCommands::Save { name } => {
            let mods_dir = AppConfig::mods_dir()?;
            std::fs::create_dir_all(&mods_dir).map_err(|e| format!("mkdir: {}", e))?;
            let path = mods_dir.join(format!("{}.toml", name));
            // Build a mod package from current config
            let mod_pkg = config_to_mod(config, &name);
            let toml_str =
                toml::to_string_pretty(&mod_pkg).map_err(|e| format!("serialize: {}", e))?;
            std::fs::write(&path, toml_str).map_err(|e| format!("write: {}", e))?;
            println!("Saved mod '{}' to {:?} (applies to all windows)", name, path);
        }
        ModCommands::Export { name } => {
            let mod_pkg = AppConfig::load_mod(&name)?;
            let toml_str =
                toml::to_string_pretty(&mod_pkg).map_err(|e| format!("serialize: {}", e))?;
            print!("{}", toml_str);
        }
        ModCommands::Import { file } => {
            let content =
                std::fs::read_to_string(&file).map_err(|e| format!("read: {}", e))?;
            let mod_pkg: crate::core::config::ModPackage =
                toml::from_str(&content).map_err(|e| format!("parse: {}", e))?;
            let mods_dir = AppConfig::mods_dir()?;
            std::fs::create_dir_all(&mods_dir).map_err(|e| format!("mkdir: {}", e))?;
            let name = mod_pkg.mod_info.name.clone();
            let path = mods_dir.join(format!("{}.toml", name));
            std::fs::write(&path, &content).map_err(|e| format!("write: {}", e))?;
            println!("Imported mod '{}' to {:?} (applies to all windows)", name, path);
        }
        ModCommands::Delete { name } => {
            let mods_dir = AppConfig::mods_dir()?;
            let path = mods_dir.join(format!("{}.toml", name));
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| format!("remove: {}", e))?;
                println!("Deleted mod '{}' (applies to all windows)", name);
            } else {
                println!("Mod '{}' not found (built-in mods cannot be deleted)", name);
            }
        }
        ModCommands::Reset => {
            let config_path = AppConfig::config_path()?;
            let default = AppConfig::default();
            let toml_str =
                toml::to_string_pretty(&default).map_err(|e| format!("serialize: {}", e))?;
            std::fs::write(&config_path, toml_str)
                .map_err(|e| format!("write: {}", e))?;
            println!("Reset to factory default (Glacier Workstation) (applies to all windows)");
        }
        ModCommands::Pick => {
            let mut items: Vec<String> = Vec::new();
            for name in BUILTIN_MODS {
                if AppConfig::load_mod(name).is_ok() {
                    items.push(name.to_string());
                }
            }
            let mods_dir = AppConfig::mods_dir()?;
            if mods_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&mods_dir) {
                    for entry in entries.filter_map(|e| e.ok()) {
                        let fname = entry.file_name().to_string_lossy().to_string();
                        if let Some(name) = fname.strip_suffix(".toml") {
                            items.push(name.to_string());
                        }
                    }
                }
            }
            if items.is_empty() {
                return Err("no mods available".into());
            }
            for (i, name) in items.iter().enumerate() {
                let active = if *name == config.active_mod { " [active]" } else { "" };
                println!("  {}. {}{}", i + 1, name, active);
            }
            print!("Select mod [1-{}]: ", items.len());
            use std::io::Write;
            std::io::stdout().flush().map_err(|e| format!("flush: {}", e))?;
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| format!("read: {}", e))?;
            let idx: usize = line
                .trim()
                .parse()
                .map_err(|_| "invalid number".to_string())?;
            if idx == 0 || idx > items.len() {
                return Err(format!("invalid choice: {} (1-{})", idx, items.len()));
            }
            let target = items[idx - 1].clone();
            let state_path = AppConfig::state_path()?;
            let mut st = StateFile::read(&state_path);
            st.previous_mod = Some(config.active_mod.clone());
            st.write(&state_path)
                .map_err(|e| format!("write state: {}", e))?;
            write_active_mod(config, &target)?;
            println!("Switched to mod '{}' ✓ (applies to all windows)", target);
        }
    }
    Ok(())
}

/// 内置 6 个 mod 的出厂顺序（find_mod_by_scene 与 mod pick 共用）。
const BUILTIN_MODS: [&str; 6] = [
    "glacier-workstation",
    "obsidian-command",
    "ember-night",
    "matrix-surveillance",
    "noir-precision",
    "noir-tabbed",
];

/// @scene 别名 → scene 名（DESIGN.md 定义的 4 个别名）。
fn scene_alias(alias: &str) -> Option<&'static str> {
    match alias {
        "daily" => Some("daily-dev"),
        "night" => Some("night-coding"),
        "agent" => Some("heavy-agent"),
        "ssh" => Some("ssh-remote"),
        _ => None,
    }
}

/// 按 scene 名查找第一个匹配的 mod（内置 6 个按出厂顺序，再用户 mods）。
fn find_mod_by_scene(scene: &str) -> Result<String, String> {
    for name in BUILTIN_MODS {
        if let Ok(pkg) = AppConfig::load_mod(name) {
            if pkg.mod_info.scene == scene {
                return Ok(name.to_string());
            }
        }
    }
    let mods_dir = AppConfig::mods_dir()?;
    if let Ok(entries) = std::fs::read_dir(&mods_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let fname = entry.file_name().to_string_lossy().to_string();
            if let Some(name) = fname.strip_suffix(".toml") {
                if let Ok(pkg) = AppConfig::load_mod(name) {
                    if pkg.mod_info.scene == scene {
                        return Ok(name.to_string());
                    }
                }
            }
        }
    }
    Err(format!("mod/scene '{}' not found", scene))
}

/// 校验 + 解析 mod 目标名（@ 场景别名 → 实际 mod 名），失败返回 Err。
fn resolve_mod_target(input: &str) -> Result<String, String> {
    let name = input.strip_prefix('@').unwrap_or(input);
    if let Some(scene) = scene_alias(name) {
        return find_mod_by_scene(scene);
    }
    if AppConfig::load_mod(name).is_ok() {
        return Ok(name.to_string());
    }
    find_mod_by_scene(name)
}

fn write_active_mod(config: &AppConfig, name: &str) -> Result<(), String> {
    let mut new_config = config.clone();
    new_config.active_mod = name.to_string();
    let config_path = AppConfig::config_path()?;
    let toml_str =
        toml::to_string_pretty(&new_config).map_err(|e| format!("serialize: {}", e))?;
    std::fs::write(&config_path, toml_str)
        .map_err(|e| format!("write config: {}", e))
}

/// 当前生效主题与基底 preset 的 20 字段差异 → overrides。
fn diff_theme(base: &Theme, merged: &Theme, out: &mut HashMap<String, toml::Value>) {
    macro_rules! diff {
        ($field:ident) => {
            if base.$field != merged.$field {
                out.insert(stringify!($field).to_string(),
                    toml::Value::String(merged.$field.clone()));
            }
        };
    }
    diff!(bg); diff!(fg); diff!(accent); diff!(success); diff!(warning);
    diff!(danger); diff!(muted); diff!(border); diff!(skill_color);
    diff!(mcp_color); diff!(model_color); diff!(separator);
    let f = merged.bar_filled.to_string();
    if base.bar_filled != merged.bar_filled {
        out.insert("bar_filled".into(), toml::Value::String(f));
    }
    let e = merged.bar_empty.to_string();
    if base.bar_empty != merged.bar_empty {
        out.insert("bar_empty".into(), toml::Value::String(e));
    }
    if base.bar_width != merged.bar_width {
        out.insert("bar_width".into(), toml::Value::Integer(merged.bar_width as i64));
    }
    if base.padding != merged.padding {
        out.insert("padding".into(), toml::Value::Integer(merged.padding as i64));
    }
    if base.compact_lines != merged.compact_lines {
        out.insert("compact_lines".into(),
            toml::Value::Integer(merged.compact_lines as i64));
    }
    if base.dashboard_grid != merged.dashboard_grid {
        out.insert("dashboard_grid".into(),
            toml::Value::Integer(merged.dashboard_grid as i64));
    }
    if base.icon_set != merged.icon_set {
        let s = match merged.icon_set {
            IconSet::Auto => "auto", IconSet::Nerd => "nerd",
            IconSet::Ascii => "ascii", IconSet::Minimal => "minimal",
        };
        out.insert("icon_set".into(), toml::Value::String(s.into()));
    }
    if base.border_style != merged.border_style {
        let s = match merged.border_style {
            BorderStyle::Single => "single", BorderStyle::Double => "double",
            BorderStyle::Rounded => "rounded", BorderStyle::Thick => "thick",
            BorderStyle::Hidden => "hidden",
        };
        out.insert("border_style".into(), toml::Value::String(s.into()));
    }
}

fn config_to_mod(config: &AppConfig, name: &str) -> crate::core::config::ModPackage {
    let resolved = config.resolve_theme();
    let base = resolved
        .preset
        .as_deref()
        .and_then(Theme::load_preset)
        .unwrap_or_default();
    let mut overrides: HashMap<String, toml::Value> = HashMap::new();
    diff_theme(&base, &resolved.theme, &mut overrides);
    let layout_id = if config.compact_layout == MINIMAL_WIDGETS {
        "minimal".to_string()
    } else if config.compact_layout == ACTIVITY_WIDGETS {
        "activity".to_string()
    } else {
        "custom".to_string()
    };
    crate::core::config::ModPackage {
        mod_info: crate::core::config::ModInfo {
            name: name.into(),
            version: "1.0.0".into(),
            description: String::new(),
            scene: String::new(),
        },
        layout: Some(crate::core::config::ModLayout {
            compact: layout_id,
            dashboard: config.dashboard.default_layout.clone(),
            compact_lines: config
                .runtime_overrides
                .as_ref()
                .and_then(|o| o.compact_lines)
                .unwrap_or(resolved.theme.compact_lines),
        }),
        compact_widgets: Some(config.compact_layout.clone()),
        theme: Some(crate::core::config::ModTheme {
            preset: resolved.preset.unwrap_or_else(|| "nord".into()),
            overrides: if overrides.is_empty() { None } else { Some(overrides) },
        }),
        animation: Some(crate::core::config::ModAnimation {
            enabled: true,
            effects: vec![],
        }),
        widgets: config.widgets.clone(),
    }
}

fn handle_theme(cmd: ThemeCommands, config: &AppConfig) -> Result<(), String> {
    match cmd {
        ThemeCommands::Export => {
            let theme = config.resolve_theme().theme;
            let toml_str =
                toml::to_string_pretty(&theme).map_err(|e| format!("serialize: {}", e))?;
            print!("{}", toml_str);
        }
        ThemeCommands::Import { file } => {
            let content =
                std::fs::read_to_string(&file).map_err(|e| format!("read: {}", e))?;
            let parsed: toml::Value =
                toml::from_str(&content).map_err(|e| format!("parse theme: {}", e))?;
            // 顶层 {theme: {...}} 或散表体两种都接受
            let table = match parsed.get("theme") {
                Some(t) => t.clone(),
                None => parsed,
            };
            if !table.is_table() {
                return Err("theme file must contain a [theme] table".into());
            }
            let config_path = AppConfig::config_path()?;
            let mut root: toml::Value = std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|c| toml::from_str(&c).ok())
                .unwrap_or_else(|| toml::Value::Table(Default::default()));
            root.as_table_mut()
                .ok_or_else(|| "config.toml is not a table".to_string())?
                .insert("theme".into(), table);
            let out = toml::to_string_pretty(&root)
                .map_err(|e| format!("serialize config: {}", e))?;
            std::fs::write(&config_path, out)
                .map_err(|e| format!("write config: {}", e))?;
            println!("Theme imported to config.toml [theme] section (applies to all windows)");
        }
    }
    Ok(())
}

fn handle_widget(cmd: WidgetCommands, registry: &WidgetRegistry) -> Result<(), String> {
    match cmd {
        WidgetCommands::List => {
            for w in registry.list() {
                println!("  {} — {}", w.id(), w.display_name());
            }
        }
        WidgetCommands::Test { name } => {
            if let Some(w) = registry.get(&name) {
                let mut theme = Theme::default();
                theme.icon_set = theme.resolve_icon_set();
                let config = crate::core::widget::WidgetConfig::default();
                let data = crate::core::session::SessionData::default();
                let output = w.render_compact(&data, &theme, &config);
                println!("Widget '{}': {}", name, output);
            } else {
                println!("Widget '{}' not found", name);
            }
        }
    }
    Ok(())
}

/// ⑨ `history`：本周统计 / 最近会话 / 近 7 天日费用。空库显示 —，不显示 0。
fn run_history(config: &AppConfig, weekly: bool) -> Result<(), String> {
    let store = HistoryStore::open()?;
    if weekly {
        return print_weekly_report(&store, &config.currency_symbol);
    }
    let symbol = &config.currency_symbol;
    let weekly = store.weekly_stats()?;
    println!("Weekly stats:");
    if weekly.total_sessions == 0 {
        println!("  Cost: — | Sessions: — | Tokens: — | Avg duration: — | Avg agents: —");
    } else {
        println!(
            "  Cost: {}{:.2} | Sessions: {} | Tokens: {} | Avg duration: {:.1}m | Avg agents: {:.1}",
            symbol, weekly.total_cost, weekly.total_sessions, weekly.total_tokens,
            weekly.avg_duration_min, weekly.avg_agents_per_session,
        );
    }
    println!("Recent sessions:");
    let recent = store.recent_sessions(5)?;
    if recent.is_empty() {
        println!("  —");
    } else {
        for r in recent {
            println!(
                "  #{}  {}  {}{:.2}  {}  {} agents  {} tok",
                r.id, r.started_at, symbol, r.total_cost_usd,
                format_history_duration(r.duration_secs), r.agent_count,
                format_history_tokens(r.total_tokens),
            );
        }
    }
    println!("Daily cost (last 7 days):");
    let trend = store.daily_cost_trend()?;
    if trend.is_empty() {
        println!("  —");
    } else {
        for (day, cost) in trend {
            println!("  {}  {}{:.2}", day, symbol, cost);
        }
    }
    Ok(())
}

/// ㉑ 周报输出：空库全 —（不显示 0）；成本带 ≈（结账值可能为估算）。
fn print_weekly_report(store: &HistoryStore, symbol: &str) -> Result<(), String> {
    let r = store.weekly_report()?;
    println!("Weekly report (last 7 days):");
    if r.sessions == 0 {
        println!("  —");
        return Ok(());
    }
    println!(
        "  ≈{}{:.2} total | {} sessions | {} tok | longest {} | top session {}{:.2}",
        symbol,
        r.total_cost,
        r.sessions,
        format_history_tokens(r.total_tokens),
        format_history_duration(r.longest_duration_secs),
        symbol,
        r.highest_cost_usd,
    );
    Ok(())
}

/// 时长人类化：≥60s 显示 "Nm"，否则 "Ns"。
fn format_history_duration(secs: u64) -> String {
    if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}

/// 千位缩写（spec 样例口径）：45000 → "45k"。
fn format_history_tokens(tokens: u64) -> String {
    if tokens >= 1000 {
        format!("{}k", (tokens as f64 / 1000.0).round() as u64)
    } else {
        tokens.to_string()
    }
}

/// Generate shell completions for the given shell name.
fn generate_completion(shell: &str) -> Result<(), String> {
    // clap_complete 4.6 移除了 from_shell_name；Shell 的 FromStr 按
    // 名称解析（大小写不敏感），失败走统一错误路径 exit 1。
    let sh = shell
        .parse::<clap_complete::Shell>()
        .map_err(|_| format!("unsupported shell: {}", shell))?;
    clap_complete::generate(sh, &mut Cli::command(), "claude-hud", &mut std::io::stdout());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::scene_alias;

    #[test]
    fn scene_alias_maps_four_scenes() {
        assert_eq!(scene_alias("daily"), Some("daily-dev"));
        assert_eq!(scene_alias("night"), Some("night-coding"));
        assert_eq!(scene_alias("agent"), Some("heavy-agent"));
        assert_eq!(scene_alias("ssh"), Some("ssh-remote"));
        assert_eq!(scene_alias("unknown"), None);
    }
}
