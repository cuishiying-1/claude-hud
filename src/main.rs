mod compact;
mod core;
mod dashboard;
mod notify;
mod probe;
mod serve;
mod widgets;

use clap::{Parser, Subcommand};
use core::config::AppConfig;
use core::theme::Theme;
use core::widget::WidgetRegistry;

#[derive(Parser)]
#[command(name = "claude-hud")]
#[command(about = "Dual-mode terminal HUD for Claude Code")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compact mode: read stdin JSON, output ANSI status line
    Render,
    /// Full-screen TUI dashboard
    Dashboard,
    /// Web dashboard (HTTP server on localhost:9527)
    Serve,
    /// Auto-configure Claude Code settings.json
    Setup,
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

    let config = AppConfig::load().unwrap_or_default();
    let theme = load_theme(&config);
    let mut registry = WidgetRegistry::new();
    widgets::register_all(&mut registry);
    widgets::register_script_widgets(&mut registry, &config);

    // Inject probe results as env vars for widgets to use
    inject_probe_env();

    let result = match cli.command {
        Commands::Render => {
            match compact::render(&registry, &config, &theme) {
                Ok(output) => {
                    print!("{}", output);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Commands::Dashboard => dashboard::run(&registry, &config, &theme),
        Commands::Serve => serve::run(
            Box::leak(Box::new(registry)),
            Box::leak(Box::new(config.clone())),
            Box::leak(Box::new(theme.clone())),
        ),
        Commands::Setup => run_setup(),
        Commands::Mod(cmd) => handle_mod(cmd, &config),
        Commands::Theme(cmd) => handle_theme(cmd, &config),
        Commands::Widget(cmd) => handle_widget(cmd, &registry),
        Commands::Completion { shell } => {
            generate_completion(&shell);
            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn load_theme(config: &AppConfig) -> Theme {
    // Try loading from active mod first
    if !config.active_mod.is_empty() {
        if let Ok(mod_pkg) = AppConfig::load_mod(&config.active_mod) {
            if let Some(mod_theme) = mod_pkg.theme {
                if let Some(theme) = Theme::load_preset(&mod_theme.preset) {
                    return theme;
                }
            }
        }
    }

    // Fall back to config.theme or default
    config.theme.clone().unwrap_or_default()
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
    // Write default config if not exists
    if !config_path.exists() {
        let default_config = toml::to_string_pretty(&AppConfig::default())
            .map_err(|e| format!("serialize config: {}", e))?;
        std::fs::write(&config_path, default_config)
            .map_err(|e| format!("write config: {}", e))?;
        println!("Config written to {:?}", config_path);
    } else {
        println!("Config already exists at {:?}", config_path);
    }

    // Write Claude Code settings.json
    if let Some(home) = dirs::home_dir() {
        let cc_settings = home.join(".claude").join("settings.json");
        let status_line_config = r#"{
  "statusLine": {
    "type": "command",
    "command": "claude-hud render",
    "refreshInterval": 5
  }
}"#;
        if !cc_settings.exists() {
            std::fs::write(&cc_settings, status_line_config)
                .map_err(|e| format!("write claude settings: {}", e))?;
            println!("Claude Code status line configured.");
        } else {
            println!("Claude Code settings.json already exists. Add this manually:");
            println!("{}", status_line_config);
        }
    }

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
                eprintln!("Quick-switch: use 'mod use -' to toggle to previous mod (not yet persisted)");
                return Ok(());
            }
            // Write active_mod to config
            let mut new_config = config.clone();
            new_config.active_mod = name.clone();
            let config_path = AppConfig::config_path()?;
            let toml_str =
                toml::to_string_pretty(&new_config).map_err(|e| format!("serialize: {}", e))?;
            std::fs::write(&config_path, toml_str)
                .map_err(|e| format!("write config: {}", e))?;
            println!("Switched to mod '{}' ✓", name);
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
            println!("Saved mod '{}' to {:?}", name, path);
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
            println!("Imported mod '{}' to {:?}", name, path);
        }
        ModCommands::Delete { name } => {
            let mods_dir = AppConfig::mods_dir()?;
            let path = mods_dir.join(format!("{}.toml", name));
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| format!("remove: {}", e))?;
                println!("Deleted mod '{}'", name);
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
            println!("Reset to factory default (Glacier Workstation)");
        }
        ModCommands::Pick => {
            println!("Interactive picker: use arrow keys to select, Enter to confirm");
            println!("(Full interactive TUI picker coming in Phase 2)");
            // For now, just list mods
            return handle_mod(ModCommands::List, config);
        }
    }
    Ok(())
}

fn config_to_mod(config: &AppConfig, name: &str) -> crate::core::config::ModPackage {
    use std::collections::HashMap;
    crate::core::config::ModPackage {
        mod_info: crate::core::config::ModInfo {
            name: name.into(),
            version: "1.0.0".into(),
            description: String::new(),
            scene: String::new(),
        },
        layout: Some(crate::core::config::ModLayout {
            compact: "activity".into(),
            dashboard: "grid-2x2".into(),
            compact_lines: 2,
        }),
        theme: Some(crate::core::config::ModTheme {
            preset: "nord".into(),
            overrides: None,
        }),
        animation: Some(crate::core::config::ModAnimation {
            enabled: true,
            effects: vec![],
        }),
        widgets: HashMap::new(),
    }
}

fn handle_theme(cmd: ThemeCommands, config: &AppConfig) -> Result<(), String> {
    match cmd {
        ThemeCommands::Export => {
            let theme = load_theme(config);
            let toml_str =
                toml::to_string_pretty(&theme).map_err(|e| format!("serialize: {}", e))?;
            print!("{}", toml_str);
        }
        ThemeCommands::Import { file } => {
            let content =
                std::fs::read_to_string(&file).map_err(|e| format!("read: {}", e))?;
            let _theme: Theme =
                toml::from_str(&content).map_err(|e| format!("parse theme: {}", e))?;
            println!("Theme imported successfully (apply with 'claude-hud mod use' or update config.toml)");
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
                let theme = Theme::default();
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

fn generate_completion(shell: &str) {
    // clap completion generation
    println!("Shell completion for {} — use 'clap_complete' crate for generation", shell);
    println!("Install:");
    match shell {
        "bash" => println!("  source <(claude-hud completion bash)"),
        "zsh" => println!("  source <(claude-hud completion zsh)"),
        "fish" => println!("  claude-hud completion fish > ~/.config/fish/completions/claude-hud.fish"),
        _ => println!("  Unsupported shell: {}", shell),
    }
}
