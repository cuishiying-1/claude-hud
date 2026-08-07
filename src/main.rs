mod compact;
mod config_tui;
mod core;
mod mod_preview_tui;
mod totals_render;
mod dashboard;
mod doctor;
mod notify;
mod probe;
mod serve;
mod alert;
mod widgets;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use compact::{ACTIVITY_WIDGETS, MINIMAL_WIDGETS};
use core::cc_config;
use core::history::HistoryStore;
use core::i18n::{tr, tr_dyn};
use core::config::AppConfig;
use core::theme::{BorderStyle, IconSet, Theme};
use core::state::{StateFile, now_secs, write_atomic};
use core::widget::WidgetRegistry;
use std::collections::HashMap;
use std::io::IsTerminal;

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
    /// Model registry management
    #[command(subcommand)]
    Model(ModelCommands),
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
    /// List recorded sessions (paginated)
    Sessions {
        /// Maximum number of sessions to list
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Skip the first N sessions
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Only sessions started on or after this date (YYYY-MM-DD)
        #[arg(long)]
        date: Option<String>,
    },
    /// Show details for a single session
    Session {
        /// Session id
        id: String,
    },
    /// All-session totals + active windows (multi-session monitor)
    Totals {
        /// Expand ended sessions (totals --all)
        #[arg(long)]
        all: bool,
    },
    /// Interactive config editor (keyboard form)
    Config,
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
    /// Preview a mod (no name: interactive browse with ↑/↓, Enter to switch)
    Preview {
        name: Option<String>,
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
    /// Install mods from a GitHub repository's mods/ directory
    Install {
        repo: String,
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

#[derive(Subcommand)]
enum ModelCommands {
    /// Sync model registry (windows & prices) from GitHub
    Sync,
    /// View/set/clear CLAUDE_CODE_MAX_CONTEXT_TOKENS in settings.json env
    Env {
        /// Window to set (omit to view, "off" to remove)
        arg: Option<String>,
    },
    /// List merged model registry (builtin + config, with source)
    List,
}

fn main() {
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
    if crate::core::i18n::Language::from_str(&config.language).is_none() {
        eprintln!(
            "[claude-hud] warning: invalid language '{}', falling back to en",
            config.language
        );
    }
    let lang = config.language();
    // 语言来自 config：手动解析路径让 clap 帮助文本能注入翻译
    let cmd = inject_help(Cli::command(), lang);
    let matches = cmd.get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
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
                match compact::dump_stdin(config.language()) {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        let state_path = AppConfig::state_path().unwrap_or_default();
                        StateFile::write_last_error(&state_path, &e);
                        println!("{}", compact::hud_err_marker(&e, config.language()));
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
                        println!("{}", compact::hud_err_marker(&e, config.language()));
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
        Commands::Setup => run_setup(lang),
        Commands::Uninstall => run_uninstall(lang),
        Commands::Doctor => doctor::run(&registry, &config, &theme),
        Commands::Mod(cmd) => handle_mod(cmd, &config, lang),
        Commands::Theme(cmd) => handle_theme(cmd, &config, lang),
        Commands::Widget(cmd) => handle_widget(cmd, &registry, lang),
        Commands::Model(cmd) => handle_model(cmd, &config, lang),
        Commands::Completion { shell } => generate_completion(&shell, lang),
        Commands::History { weekly } => run_history(&config, weekly, lang),
        Commands::Sessions { limit, offset, date } => {
            run_sessions(&config, limit, offset, date.as_deref(), lang)
        }
        Commands::Session { id } => run_session(&config, &id, lang),
        Commands::Totals { all } => run_totals(&config, lang, all),
        Commands::Config => config_tui::run(&registry, &config),
        Commands::Update { cmd } => match cmd {
            UpdateCommands::Check => {
                let status = core::update::check_update();
                println!("{}", core::update::describe(&status, config.language()));
                Ok(())
            }
        },
    };

    if let Err(e) = result {
        eprintln!("{}", tr(lang, "runtime.err").replace("{e}", &e));
        std::process::exit(1);
    }
}

/// 把语言注入 clap 帮助文本（命令名保持英文；文本走 tr）。
/// clap 4.6 的 mut_subcommand/mut_arg 只匹配单层名称：嵌套子命令与
/// 子命令参数必须在所属子命令的闭包内逐个注入。
fn inject_help(cmd: clap::Command, lang: crate::core::i18n::Language) -> clap::Command {
    cmd.about(tr(lang, "cli.about"))
        .mut_subcommand("render", |c| {
            c.about(tr(lang, "cli.render"))
                .mut_arg("dump", |a| a.help(tr(lang, "cli.render_dump")))
        })
        .mut_subcommand("dashboard", |c| c.about(tr(lang, "cli.dashboard")))
        .mut_subcommand("serve", |c| c.about(tr(lang, "cli.serve")))
        .mut_subcommand("setup", |c| c.about(tr(lang, "cli.setup")))
        .mut_subcommand("uninstall", |c| c.about(tr(lang, "cli.uninstall")))
        .mut_subcommand("doctor", |c| c.about(tr(lang, "cli.doctor")))
        .mut_subcommand("mod", |c| {
            c.about(tr(lang, "cli.mod"))
                .mut_subcommand("list", |cc| cc.about(tr(lang, "cli.mod_list")))
                .mut_subcommand("use", |cc| cc.about(tr(lang, "cli.mod_use")))
                .mut_subcommand("preview", |cc| cc.about(tr(lang, "cli.mod_preview")))
                .mut_subcommand("current", |cc| cc.about(tr(lang, "cli.mod_current")))
                .mut_subcommand("save", |cc| cc.about(tr(lang, "cli.mod_save")))
                .mut_subcommand("export", |cc| cc.about(tr(lang, "cli.mod_export")))
                .mut_subcommand("import", |cc| cc.about(tr(lang, "cli.mod_import")))
                .mut_subcommand("install", |cc| cc.about(tr(lang, "cli.mod_install")))
                .mut_subcommand("delete", |cc| cc.about(tr(lang, "cli.mod_delete")))
                .mut_subcommand("reset", |cc| cc.about(tr(lang, "cli.mod_reset")))
                .mut_subcommand("pick", |cc| cc.about(tr(lang, "cli.mod_pick")))
        })
        .mut_subcommand("theme", |c| {
            c.about(tr(lang, "cli.theme"))
                .mut_subcommand("export", |cc| cc.about(tr(lang, "cli.theme_export")))
                .mut_subcommand("import", |cc| cc.about(tr(lang, "cli.theme_import")))
        })
        .mut_subcommand("widget", |c| {
            c.about(tr(lang, "cli.widget"))
                .mut_subcommand("list", |cc| cc.about(tr(lang, "cli.widget_list")))
                .mut_subcommand("test", |cc| cc.about(tr(lang, "cli.widget_test")))
        })
        .mut_subcommand("model", |c| {
            c.about(tr(lang, "cli.model"))
                .mut_subcommand("sync", |cc| cc.about(tr(lang, "cli.model_sync")))
                .mut_subcommand("env", |cc| cc.about(tr(lang, "cli.model_env")))
                .mut_subcommand("list", |cc| cc.about(tr(lang, "cli.model_list")))
        })
        .mut_subcommand("completion", |c| {
            c.about(tr(lang, "cli.completion"))
                .mut_arg("shell", |a| a.help(tr(lang, "cli.completion_shell")))
        })
        .mut_subcommand("history", |c| {
            c.about(tr(lang, "cli.history"))
                .mut_arg("weekly", |a| a.help(tr(lang, "cli.history_weekly")))
        })
        .mut_subcommand("sessions", |c| {
            c.about(tr(lang, "cli.sessions"))
                .mut_arg("limit", |a| a.help(tr(lang, "cli.sessions_limit")))
                .mut_arg("offset", |a| a.help(tr(lang, "cli.sessions_offset")))
                .mut_arg("date", |a| a.help(tr(lang, "cli.sessions_date")))
        })
        .mut_subcommand("session", |c| {
            c.about(tr(lang, "cli.session"))
                .mut_arg("id", |a| a.help(tr(lang, "cli.session_id")))
        })
        .mut_subcommand("totals", |c| {
            c.about(tr(lang, "cli.totals"))
                .mut_arg("all", |a| a.help(tr(lang, "cli.totals_all")))
        })
        .mut_subcommand("config", |c| c.about(tr(lang, "cli.config")))
        .mut_subcommand("update", |c| {
            c.about(tr(lang, "cli.update"))
                .mut_subcommand("check", |cc| cc.about(tr(lang, "cli.update_check")))
        })
}

fn inject_probe_env() {
    let skill_count = probe::filesystem::count_skills();
    let mcp_count = probe::filesystem::count_mcp_servers();
    std::env::set_var("CLAUDE_HUD_SKILL_COUNT", skill_count.to_string());
    std::env::set_var("CLAUDE_HUD_MCP_COUNT", mcp_count.to_string());
}

fn run_setup(lang: crate::core::i18n::Language) -> Result<(), String> {
    let config_path = AppConfig::config_path()?;
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
    }
    if !config_path.exists() {
        let default_config = toml::to_string_pretty(&AppConfig::default())
            .map_err(|e| format!("serialize config: {}", e))?;
        std::fs::write(&config_path, default_config)
            .map_err(|e| format!("write config: {}", e))?;
        println!(
            "{}",
            tr(lang, "runtime.setup_written").replace("{path}", &format!("{:?}", config_path))
        );
    } else {
        println!(
            "{}",
            tr(lang, "runtime.setup_exists").replace("{path}", &format!("{:?}", config_path))
        );
    }
    setup_cc_settings(lang)?;
    Ok(())
}

/// Merge the HUD statusLine into ~/.claude/settings.json. A timestamped
/// backup (settings.json.hud.bak-<epoch>) is written only when an existing
/// statusLine or unparseable JSON would be overwritten; the fixed-name
/// json.bak is gone and .hud.bak-* is never deleted by setup/uninstall.
fn setup_cc_settings(lang: crate::core::i18n::Language) -> Result<(), String> {
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
            println!(
                "{}",
                tr(lang, "runtime.setup_replacing")
                    .replace("{path}", &format!("{:?}", backup))
            );
        } else {
            println!(
                "{}",
                tr(lang, "runtime.setup_bad_json")
                    .replace("{path}", &format!("{:?}", backup))
            );
        }
    }

    let merged = if valid_json {
        cc_config::merge_status_line(&original, &cc_config::default_status_line_command())?
    } else {
        cc_config::merge_status_line("", &cc_config::default_status_line_command())?
    };
    write_atomic(&settings_path, &merged)?;
    println!(
        "{}",
        tr(lang, "runtime.setup_done").replace("{path}", &format!("{:?}", settings_path))
    );
    Ok(())
}

fn run_uninstall(lang: crate::core::i18n::Language) -> Result<(), String> {
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
                    println!(
                        "{}",
                        tr(lang, "runtime.uninstall_removed")
                            .replace("{path}", &format!("{:?}", settings_path))
                    );
                } else {
                    println!(
                        "{}",
                        tr(lang, "runtime.uninstall_none")
                            .replace("{path}", &format!("{:?}", settings_path))
                    );
                }
            }
            Err(e) => eprintln!("warning: skip settings.json cleanup ({})", e),
        }
    }
    let config_dir = AppConfig::hud_dir()?;
    if config_dir.exists() {
        std::fs::remove_dir_all(&config_dir)
            .map_err(|e| format!("remove config dir: {}", e))?;
        println!(
            "{}",
            tr(lang, "runtime.uninstall_dir").replace("{path}", &format!("{:?}", config_dir))
        );
    }
    println!("{}", tr(lang, "runtime.uninstall_bak"));
    println!("{}", tr(lang, "runtime.uninstall_done"));
    Ok(())
}

fn handle_mod(
    cmd: ModCommands,
    config: &AppConfig,
    lang: crate::core::i18n::Language,
) -> Result<(), String> {
    match cmd {
        ModCommands::List => {
            println!("{}", tr(lang, "runtime.mod_builtins"));
            for name in Theme::preset_names() {
                let active = if name == &config.active_mod { " [active]" } else { "" };
                println!(
                    "{}",
                    tr(lang, "runtime.mod_list_line")
                        .replace("{name}", name)
                        .replace("{kind}", tr(lang, "runtime.mod_kind_builtin"))
                        .replace("{active}", active)
                );
            }
            println!("{}", tr(lang, "runtime.mod_users"));
            let mods_dir = AppConfig::mods_dir()?;
            if mods_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&mods_dir) {
                    for entry in entries.filter_map(|e| e.ok()) {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let name = name.strip_suffix(".toml").unwrap_or(&name);
                        let active = if name == config.active_mod { " [active]" } else { "" };
                        println!(
                            "{}",
                            tr(lang, "runtime.mod_list_line")
                                .replace("{name}", name)
                                .replace("{kind}", tr(lang, "runtime.mod_kind_user"))
                                .replace("{active}", active)
                        );
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
                    None => return Err(tr(lang, "runtime.mod_no_prev").to_string()),
                };
                st.previous_mod = Some(config.active_mod.clone());
                st.write(&state_path)
                    .map_err(|e| format!("write state: {}", e))?;
                write_active_mod(config, &prev)?;
                println!(
                    "{}",
                    tr(lang, "runtime.mod_switched_back").replace("{name}", &prev)
                );
                return Ok(());
            }
            // ① 校验 + ③ @scene 解析：失败不写 config
            let target = resolve_mod_target(&name, lang)?;
            let state_path = AppConfig::state_path()?;
            let mut st = StateFile::read(&state_path);
            st.previous_mod = Some(config.active_mod.clone());
            st.write(&state_path)
                .map_err(|e| format!("write state: {}", e))?;
            write_active_mod(config, &target)?;
            println!(
                "{}",
                tr(lang, "runtime.mod_switched").replace("{name}", &target)
            );
        }
        ModCommands::Preview { name } => {
            if let Err(e) = run_preview(name, config, lang) {
                return Err(e);
            }
        }
        ModCommands::Current => {
            println!(
                "{}",
                tr(lang, "runtime.mod_active").replace("{name}", &config.active_mod)
            );
            if let Ok(mod_pkg) = AppConfig::load_mod(&config.active_mod) {
                println!(
                    "{}",
                    tr(lang, "runtime.mod_name").replace("{name}", &mod_pkg.mod_info.name)
                );
                println!(
                    "{}",
                    tr(lang, "runtime.mod_desc")
                        .replace("{desc}", &mod_pkg.mod_info.description)
                );
                println!(
                    "{}",
                    tr(lang, "runtime.mod_scene2").replace("{scene}", &mod_pkg.mod_info.scene)
                );
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
            println!(
                "{}",
                tr(lang, "runtime.mod_saved")
                    .replace("{name}", &name)
                    .replace("{path}", &format!("{:?}", path))
            );
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
            println!(
                "{}",
                tr(lang, "runtime.mod_imported")
                    .replace("{name}", &name)
                    .replace("{path}", &format!("{:?}", path))
            );
        }
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
        }
        ModCommands::Delete { name } => {
            let mods_dir = AppConfig::mods_dir()?;
            let path = mods_dir.join(format!("{}.toml", name));
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| format!("remove: {}", e))?;
                println!(
                    "{}",
                    tr(lang, "runtime.mod_deleted").replace("{name}", &name)
                );
            } else {
                println!(
                    "{}",
                    tr(lang, "runtime.mod_not_found").replace("{name}", &name)
                );
            }
        }
        ModCommands::Reset => {
            let config_path = AppConfig::config_path()?;
            let default = AppConfig::default();
            let toml_str =
                toml::to_string_pretty(&default).map_err(|e| format!("serialize: {}", e))?;
            std::fs::write(&config_path, toml_str)
                .map_err(|e| format!("write: {}", e))?;
            println!("{}", tr(lang, "runtime.mod_reset"));
        }
        ModCommands::Pick => {
            let items = mod_preview_tui::candidates();
            if items.is_empty() {
                return Err(tr(lang, "runtime.mod_none").to_string());
            }
            for (i, name) in items.iter().enumerate() {
                let active = if *name == config.active_mod { " [active]" } else { "" };
                println!("  {}. {}{}", i + 1, name, active);
            }
            print!(
                "{}",
                tr(lang, "runtime.mod_select").replace("{n}", &items.len().to_string())
            );
            use std::io::Write;
            std::io::stdout().flush().map_err(|e| format!("flush: {}", e))?;
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| format!("read: {}", e))?;
            let idx: usize = line
                .trim()
                .parse()
                .map_err(|_| tr(lang, "runtime.mod_invalid_num").to_string())?;
            if idx == 0 || idx > items.len() {
                return Err(
                    tr(lang, "runtime.mod_invalid_choice")
                        .replace("{idx}", &idx.to_string())
                        .replace("{n}", &items.len().to_string()),
                );
            }
            let target = items[idx - 1].clone();
            let state_path = AppConfig::state_path()?;
            let mut st = StateFile::read(&state_path);
            st.previous_mod = Some(config.active_mod.clone());
            st.write(&state_path)
                .map_err(|e| format!("write state: {}", e))?;
            write_active_mod(config, &target)?;
            println!(
                "{}",
                tr(lang, "runtime.mod_switched").replace("{name}", &target)
            );
        }
    }
    Ok(())
}

/// 内置 6 个 mod 的出厂顺序（find_mod_by_scene / mod pick / mod preview 共用）。
pub(crate) const BUILTIN_MODS: [&str; 6] = [
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
fn find_mod_by_scene(
    scene: &str,
    lang: crate::core::i18n::Language,
) -> Result<String, String> {
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
    Err(tr(lang, "runtime.mod_scene_not_found").replace("{name}", scene))
}

/// `mod preview`：有名字 = 静态元数据 + 主题样例行；无名字 = 交互式浏览
/// （↑/↓ 预览、Enter 切换、q 退出；非 TTY 回退数字列表）。
fn run_preview(
    name: Option<String>,
    config: &AppConfig,
    lang: crate::core::i18n::Language,
) -> Result<(), String> {
    match name {
        Some(name) => {
            let mod_pkg = AppConfig::load_mod(&name)?;
            println!(
                "{}",
                tr(lang, "runtime.mod_preview").replace("{name}", &mod_pkg.mod_info.name)
            );
            println!(
                "{}",
                tr(lang, "runtime.mod_scene").replace("{scene}", &mod_pkg.mod_info.scene)
            );
            if let Some(layout) = &mod_pkg.layout {
                println!(
                    "{}",
                    tr(lang, "runtime.mod_layout_line")
                        .replace("{compact}", &layout.compact)
                        .replace("{dashboard}", &layout.dashboard)
                );
            }
            if let Some(theme) = &mod_pkg.theme {
                println!(
                    "{}",
                    tr(lang, "runtime.mod_theme").replace("{theme}", &theme.preset)
                );
            }
            if let Some(anim) = &mod_pkg.animation {
                println!(
                    "{}",
                    tr(lang, "runtime.mod_animation_line")
                        .replace("{enabled}", &anim.enabled.to_string())
                        .replace("{effects}", &format!("{:?}", anim.effects))
                );
            }
            let mut probe = config.clone();
            probe.active_mod = name.clone();
            let sample = mod_preview_tui::theme_sample(&probe.resolve_theme().theme);
            println!(
                "{}",
                tr(lang, "runtime.mod_preview_sample").replace("{sample}", &sample)
            );
            Ok(())
        }
        None => {
            let picked = mod_preview_tui::run(config)?;
            if let Some(target) = picked {
                let state_path = AppConfig::state_path()?;
                let mut st = StateFile::read(&state_path);
                st.previous_mod = Some(config.active_mod.clone());
                st.write(&state_path)
                    .map_err(|e| format!("write state: {}", e))?;
                write_active_mod(config, &target)?;
                println!(
                    "{}",
                    tr(lang, "runtime.mod_switched").replace("{name}", &target)
                );
            }
            Ok(())
        }
    }
}

/// 校验 + 解析 mod 目标名（@ 场景别名 → 实际 mod 名），失败返回 Err。
fn resolve_mod_target(
    input: &str,
    lang: crate::core::i18n::Language,
) -> Result<String, String> {
    let name = input.strip_prefix('@').unwrap_or(input);
    if let Some(scene) = scene_alias(name) {
        return find_mod_by_scene(scene, lang);
    }
    if AppConfig::load_mod(name).is_ok() {
        return Ok(name.to_string());
    }
    find_mod_by_scene(name, lang)
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

fn handle_theme(
    cmd: ThemeCommands,
    config: &AppConfig,
    lang: crate::core::i18n::Language,
) -> Result<(), String> {
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
                return Err(tr(lang, "runtime.theme_need_table").to_string());
            }
            let config_path = AppConfig::config_path()?;
            let mut root: toml::Value = std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|c| toml::from_str(&c).ok())
                .unwrap_or_else(|| toml::Value::Table(Default::default()));
            root.as_table_mut()
                .ok_or_else(|| tr(lang, "runtime.theme_bad_config").to_string())?
                .insert("theme".into(), table);
            let out = toml::to_string_pretty(&root)
                .map_err(|e| format!("serialize config: {}", e))?;
            std::fs::write(&config_path, out)
                .map_err(|e| format!("write config: {}", e))?;
            println!("{}", tr(lang, "runtime.theme_imported"));
        }
    }
    Ok(())
}

fn handle_widget(
    cmd: WidgetCommands,
    registry: &WidgetRegistry,
    lang: crate::core::i18n::Language,
) -> Result<(), String> {
    match cmd {
        WidgetCommands::List => {
            for w in registry.list() {
                // widget.<id> key 存在 → 翻译；否则回退原显示名（如脚本路径）
                let translated = tr_dyn(lang, w.id());
                let name = if translated == w.id() {
                    w.display_name().to_string()
                } else {
                    translated.into_owned()
                };
                println!("  {} — {}", w.id(), name);
            }
        }
        WidgetCommands::Test { name } => {
            if let Some(w) = registry.get(&name) {
                let mut theme = Theme::default();
                theme.icon_set = theme.resolve_icon_set();
                let config = crate::core::widget::WidgetConfig::default();
                let data = crate::core::session::SessionData::default();
                let output = w.render_compact(&data, &theme, &config);
                println!(
                    "{}",
                    tr(lang, "runtime.widget_test")
                        .replace("{name}", &name)
                        .replace("{out}", &output)
                );
            } else {
                println!(
                    "{}",
                    tr(lang, "runtime.widget_not_found").replace("{name}", &name)
                );
            }
        }
    }
    Ok(())
}

fn handle_model(
    cmd: ModelCommands,
    config: &AppConfig,
    lang: crate::core::i18n::Language,
) -> Result<(), String> {
    match cmd {
        ModelCommands::Sync => {
            let paths = crate::core::modelsync::SyncPaths {
                config: AppConfig::config_path()?,
                settings: crate::core::modelsync::settings_path()?,
            };
            let result = crate::core::modelsync::run_sync(&paths, lang, &mut prompt_yn)?;
            println!(
                "{}",
                tr(lang, "runtime.model_sync_ok")
                    .replace("{version}", &result.version)
                    .replace("{n}", &result.updated.len().to_string())
            );
            for id in &result.updated {
                println!("  - {}", id);
            }
            if let Some(e) = result.env_failed {
                println!("{}", tr(lang, "runtime.model_sync_env_fail").replace("{e}", &e));
            }
            Ok(())
        }
        ModelCommands::Env { arg } => crate::core::modelsync::model_env_cmd(config, arg.as_deref(), lang),
        ModelCommands::List => crate::core::modelsync::model_list_cmd(config, lang),
    }
}

/// 交互询问 [y/N]，默认 N（不写 env）。仅 Sync 命令使用。
fn prompt_yn(prompt: &str) -> bool {
    println!("{} [y/N]", prompt);
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map(|n| n > 0 && line.trim().eq_ignore_ascii_case("y"))
        .unwrap_or(false)
}

/// ⑨ `history`：本周统计 / 最近会话 / 近 7 天日费用。空库显示 —，不显示 0。
fn run_history(
    config: &AppConfig,
    weekly: bool,
    lang: crate::core::i18n::Language,
) -> Result<(), String> {
    let store = HistoryStore::open()?;
    if weekly {
        return print_weekly_report(&store, config.currency(), lang);
    }
    let symbol = config.currency();
    let weekly = store.weekly_stats()?;
    println!("{}", tr(lang, "runtime.h_weekly"));
    if weekly.total_sessions == 0 {
        println!("{}", tr(lang, "runtime.h_weekly_empty"));
    } else {
        println!(
            "{}",
            tr(lang, "runtime.h_weekly_line")
                .replace("{sym}", symbol)
                .replace("{cost}", &format!("{:.2}", weekly.total_cost))
                .replace("{sessions}", &weekly.total_sessions.to_string())
                .replace("{tokens}", &weekly.total_tokens.to_string())
                .replace("{dur}", &format!("{:.1}", weekly.avg_duration_min))
                .replace("{agents}", &format!("{:.1}", weekly.avg_agents_per_session))
        );
    }
    println!("{}", tr(lang, "runtime.h_recent"));
    let recent = store.recent_sessions(5)?;
    if recent.is_empty() {
        println!("  —");
    } else {
        for r in recent {
            println!(
                "{}",
                tr(lang, "runtime.h_session_line")
                    .replace("{id}", &r.id.to_string())
                    .replace("{start}", &r.started_at)
                    .replace("{sym}", symbol)
                    .replace("{cost}", &format!("{:.2}", r.total_cost_usd))
                    .replace("{dur}", &format_history_duration(r.duration_secs))
                    .replace("{n}", &r.agent_count.to_string())
                    .replace("{tok}", &format_history_tokens(r.total_tokens))
            );
        }
    }
    println!("{}", tr(lang, "runtime.h_daily"));
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

/// ⑤ `sessions`：分页会话列表。空库显示 —；行格式与 history 列表一致。
fn run_sessions(
    config: &AppConfig,
    limit: usize,
    offset: usize,
    date_from: Option<&str>,
    lang: crate::core::i18n::Language,
) -> Result<(), String> {
    let store = HistoryStore::open()?;
    println!("{}", tr(lang, "runtime.h_sessions_title"));
    let symbol = config.currency();
    let rows = store.sessions_page(limit, offset, date_from)?;
    if rows.is_empty() {
        println!("  —");
    } else {
        for r in rows {
            println!(
                "{}",
                tr(lang, "runtime.h_session_line")
                    .replace("{id}", &r.id.to_string())
                    .replace("{start}", &r.started_at)
                    .replace("{sym}", symbol)
                    .replace("{cost}", &format!("{:.2}", r.total_cost_usd))
                    .replace("{dur}", &format_history_duration(r.duration_secs))
                    .replace("{n}", &r.agent_count.to_string())
                    .replace("{tok}", &format_history_tokens(r.total_tokens))
            );
        }
    }
    Ok(())
}

/// ⑥ `session <id>`：单会话详情。transcript_path 存在 → 尾读补充
/// token 分解/代理列表/工具成本排行；未找到 → 明确报错（exit 1）。
fn run_session(
    config: &AppConfig,
    id: &str,
    lang: crate::core::i18n::Language,
) -> Result<(), String> {
    let store = HistoryStore::open()?;
    let sid: i64 = id.parse()
        .map_err(|_| tr(lang, "runtime.h_session_not_found").replace("{id}", id))?;
    let Some(r) = store.session_by_id(sid)? else {
        return Err(tr(lang, "runtime.h_session_not_found").replace("{id}", id));
    };
    let symbol = config.currency();
    println!("{}", tr(lang, "runtime.h_session_title").replace("{id}", &r.id.to_string()));
    println!("{}", tr(lang, "runtime.h_session_model").replace("{model}", &r.model));
    println!(
        "{}",
        tr(lang, "runtime.h_session_cost")
            .replace("{sym}", symbol)
            .replace("{cost}", &format!("{:.2}", r.total_cost_usd))
    );
    println!(
        "{}",
        tr(lang, "runtime.h_session_duration")
            .replace("{dur}", &format_history_duration(r.duration_secs))
    );
    println!(
        "{}",
        tr(lang, "runtime.h_session_agents").replace("{n}", &r.agent_count.to_string())
    );
    let tokens = format_history_tokens(r.total_tokens);
    let summary = match r.transcript_path.as_deref() {
        Some(path) if std::path::Path::new(path).exists() => {
            Some(crate::core::transcript::TranscriptReader::new(path.into()).read_updates())
        }
        _ => None,
    };
    match &summary {
        Some(s) => {
            println!(
                "{}",
                tr(lang, "runtime.h_session_tokens")
                    .replace("{tok}", &tokens)
                    .replace("{in}", &s.total_tokens.input.to_string())
                    .replace("{out}", &s.total_tokens.output.to_string())
            );
            println!("{}", tr(lang, "runtime.h_session_agent_list"));
            for a in &s.agents {
                println!(
                    "{}",
                    tr(lang, "runtime.h_session_agent_line")
                        .replace("{name}", &a.name)
                        .replace("{calls}", &a.tool_calls.to_string())
                );
            }
        }
        None => {
            println!(
                "{}",
                tr(lang, "runtime.h_session_tokens_plain").replace("{tok}", &tokens)
            );
        }
    }
    println!("{}", tr(lang, "runtime.h_tools_title"));
    match summary.as_ref().and_then(|s| {
        crate::core::pricing::tool_cost_ranking(
            s,
            &crate::core::pricing::merged_pricing(config),
            &r.model,
        )
    }) {
        Some(rows) if !rows.is_empty() => {
            for (tool, calls, cost) in rows.iter().take(5) {
                println!(
                    "{}",
                    tr(lang, "runtime.h_tool_line")
                        .replace("{tool}", tool)
                        .replace("{n}", &calls.to_string())
                        .replace("{sym}", symbol)
                        .replace("{cost}", &format!("{:.2}", cost))
                );
            }
        }
        _ => println!("{}", tr(lang, "runtime.h_tools_empty")),
    }
    Ok(())
}

/// 多会话监控 CLI:历史全量总和(COUNT/SUM/AVG)+ 最近 7 天按天 + 活跃窗口
/// 实时段(实时数据未计入总和,单独列出)。分段卡片式输出：主题色 +
/// 对齐列；已结束窗口折叠(--all 展开)；非 TTY 降级纯文本。
fn run_totals(
    config: &AppConfig,
    lang: crate::core::i18n::Language,
    all: bool,
) -> Result<(), String> {
    let theme = config.resolve_theme().theme;
    let color = std::io::stdout().is_terminal();
    let sym = config.currency().to_string();

    let store = HistoryStore::open()?;
    let t = store.totals()?;
    println!(
        "{}",
        totals_render::section_title(tr(lang, "runtime.t_totals_title"), &theme, color)
    );
    if t.sessions == 0 {
        println!("  —");
    } else {
        println!(
            "{}",
            totals_render::totals_line(
                t.sessions, t.total_cost, t.total_tokens, t.total_duration_secs,
                t.avg_duration_min, &sym, &theme, lang, color
            )
        );
    }

    println!(
        "{}",
        totals_render::section_title(tr(lang, "runtime.t_totals_daily_title"), &theme, color)
    );
    let daily = store.daily_totals()?;
    if daily.is_empty() {
        println!("  —");
    } else {
        for (day, cost, tokens) in daily {
            println!(
                "{}",
                totals_render::daily_line(&day, cost, tokens, &sym, &theme, lang, color)
            );
        }
    }

    println!(
        "{}",
        totals_render::section_title(
            tr(lang, "runtime.t_totals_windows_title"),
            &theme,
            color
        )
    );
    let wins = crate::core::windows::scan_windows(crate::core::state::now_secs());
    if wins.is_empty() {
        println!("  —");
    } else if all {
        for w in &wins {
            println!(
                "{}",
                totals_render::window_line(w, &sym, &theme, lang, color)
            );
        }
    } else {
        let (kept, fold) = totals_render::fold_ended(&wins);
        for w in kept {
            println!(
                "{}",
                totals_render::window_line(w, &sym, &theme, lang, color)
            );
        }
        if let Some(f) = fold {
            println!(
                "{}",
                totals_render::folded_line(&f, &sym, &theme, lang, color)
            );
        }
    }
    Ok(())
}

/// ㉑ 周报输出：空库全 —（不显示 0）；成本带 ≈（结账值可能为估算）。
fn print_weekly_report(
    store: &HistoryStore,
    symbol: &str,
    lang: crate::core::i18n::Language,
) -> Result<(), String> {
    let r = store.weekly_report()?;
    println!("{}", tr(lang, "runtime.history_weekly"));
    if r.sessions == 0 {
        println!("  —");
        return Ok(());
    }
    println!(
        "{}",
        tr(lang, "runtime.h_weekly_report_line")
            .replace("{sym}", symbol)
            .replace("{cost}", &format!("{:.2}", r.total_cost))
            .replace("{n}", &r.sessions.to_string())
            .replace("{tok}", &format_history_tokens(r.total_tokens))
            .replace("{dur}", &format_history_duration(r.longest_duration_secs))
            .replace("{sym2}", symbol)
            .replace("{top}", &format!("{:.2}", r.highest_cost_usd))
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
fn generate_completion(
    shell: &str,
    lang: crate::core::i18n::Language,
) -> Result<(), String> {
    // clap_complete 4.6 移除了 from_shell_name；Shell 的 FromStr 按
    // 名称解析（大小写不敏感），失败走统一错误路径 exit 1。
    let sh = shell
        .parse::<clap_complete::Shell>()
        .map_err(|_| tr(lang, "runtime.unsupported_shell").replace("{shell}", shell))?;
    let mut cmd = inject_help(Cli::command(), lang);
    clap_complete::generate(sh, &mut cmd, "claude-hud", &mut std::io::stdout());
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
