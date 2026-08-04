# Phase 3 — 配置契约（任务 ⑤⑥⑦）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复主题配置契约（ThemeRef 双形态 + 叠加链 + 失败可见 + import 落盘）、Mod 系统真相（校验/快照/场景/渲染灌入）、4 处 ANSI 上色修复 + 黑盒 ANSI 结构断言。

**Architecture:** 主题解析重构为「基底 + 逐键类型化叠加」四层链（mod preset → config 键 → config overrides → mod overrides），ThemeRef 用 untagged 两形态 + flatten 捕获显式键；Mod 系统补齐 use 校验/previous_mod/@scene/save 快照（新增 `compact_widgets` 字段）/渲染灌入；ANSI 统一「要着色的文本整体包进色码」。

**Tech Stack:** Rust + serde/toml/ratatui；Python 黑盒 harness。

**项目约束（务必遵守）：**
- 禁止运行 `cargo fmt`（代码库有意不遵循 rustfmt）
- cargo 不在 PATH：`export PATH="$HOME/.cargo/bin:$PATH"`
- 黑盒：`python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")"`
- **用户手动 git 提交**——计划中的 commit 命令由用户执行，agent 不得自动 git add/commit/push

---

## 文件结构

| 文件 | 职责 | 改动 |
|------|------|------|
| `src/core/theme.rs` | ThemeRef/ThemeTable/ResolvedTheme + apply_theme_keys + 11 色 default 函数 | 大改 |
| `src/core/config.rs` | theme 字段改 Option\<ThemeRef\> + AppConfig::resolve_theme + merge_theme | 大改 |
| `src/core/state.rs` | StateFile 加 previous_mod 字段 | 小改 |
| `src/main.rs` | load_theme→resolve_theme 调用点 + 失败警告 + mod 命令层（use 校验/previous/@scene/save/pick）+ theme import 落盘 | 大改 |
| `src/compact.rs` | resolve_compact_layout / resolve_compact_lines（纯函数 + 组装） | 中改 |
| `src/widgets/rate_limits.rs` | ANSI 整段包色 | 小改 |
| `src/widgets/session_stats.rs` | ANSI 整段包色 | 小改 |
| `src/widgets/token_attribution.rs` | ANSI 整段包色 | 小改 |
| `src/widgets/cost_display.rs` | ANSI 整段包色（含数字） | 小改 |
| `scripts/hudlib/assertions.py` | 新增 stdout_raw_regex 键 | 小改 |
| `scripts/hudlib/cases.py` | P3-01..15 用例（106 → 121） | 中改 |
| `fixtures/theme/nord_partial.toml` | theme import 测试夹具 | 新建 |
| `DESIGN.md` / `DEPLOY.md` / `COMPLETE.md` / `CHANGELOG.md` | 文档修正 | 小改 |

---

### Task 1: ThemeRef 双形态 + 11 色默认值（theme.rs + config.rs 类型层）

**Files:**
- Modify: `src/core/theme.rs`
- Modify: `src/core/config.rs:28`
- Test: `src/core/theme.rs`（tests 模块）

- [ ] **Step 1: 写失败测试（theme.rs tests 模块末尾追加）**

```rust
#[test]
fn theme_ref_string_preset_parses() {
    let tr: ThemeRef = toml::from_str("theme = \"dracula\"").and_then(|v: toml::Value| {
        Ok(match v.get("theme").unwrap() {
            toml::Value::String(s) => ThemeRef::Preset(s.clone()),
            _ => unreachable!(),
        })
    }).unwrap_or(ThemeRef::Preset("nord".into()));
    assert!(matches!(tr, ThemeRef::Preset(_)));
}

#[test]
fn theme_ref_table_parses_partial() {
    let cfg: toml::Value = toml::from_str(
        "[theme]\naccent = \"#ff0000\"\n",
    ).unwrap();
    // 通过 AppConfig 走 serde：ThemeRef 字段由 config.rs 持有，这里直接测枚举
    let tbl: ThemeTable = toml::from_str(
        "accent = \"#ff0000\"\n",
    ).unwrap();
    assert_eq!(tbl.preset, None);
    assert_eq!(tbl.overrides, None);
    assert!(tbl.colors.contains_key("accent"));
}

#[test]
fn theme_ref_table_parses_preset_and_overrides() {
    let tbl: ThemeTable = toml::from_str(
        "preset = \"dracula\"\n[overrides]\naccent = \"#123456\"\n",
    ).unwrap();
    assert_eq!(tbl.preset.as_deref(), Some("dracula"));
    assert!(tbl.overrides.is_some());
}

#[test]
fn theme_ref_empty_table_parses() {
    let tbl: ThemeTable = toml::from_str("").unwrap();
    assert_eq!(tbl.preset, None);
    assert!(tbl.colors.is_empty());
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test theme_ref 2>&1 | tail -8`
Expected: `error[E0425]: cannot find value 'ThemeRef'`（类型不存在）

- [ ] **Step 3: 实现 ThemeRef/ThemeTable + 11 色 default**

`src/core/theme.rs` 顶部（`use std::collections::HashMap;` 加入）：

```rust
/// 主题引用：字符串预设名或 [theme] 表（部分/完整/preset+overrides 统一走
/// Table 形态）。untagged 按声明顺序尝试，字符串与表天然互斥，无歧义。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ThemeRef {
    Preset(String),
    Table(ThemeTable),
}

/// [theme] 表：preset 引用 + overrides 微调 + flatten 捕获的显式主题键。
/// flatten 是叠加合并正确性的关键——「哪些键被显式写出」可检测。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ThemeTable {
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub overrides: Option<HashMap<String, toml::Value>>,
    #[serde(flatten)]
    pub colors: HashMap<String, toml::Value>,
}
```

`Theme` 结构 11 个颜色字段改为（示例，11 个全部改）：

```rust
    #[serde(default = "default_bg")]
    pub bg: String,
    #[serde(default = "default_fg")]
    pub fg: String,
    #[serde(default = "default_accent")]
    pub accent: String,
    #[serde(default = "default_success")]
    pub success: String,
    #[serde(default = "default_warning")]
    pub warning: String,
    #[serde(default = "default_danger")]
    pub danger: String,
    #[serde(default = "default_muted")]
    pub muted: String,
    #[serde(default = "default_border")]
    pub border: String,
    #[serde(default = "default_skill_color")]
    pub skill_color: String,
    #[serde(default = "default_mcp_color")]
    pub mcp_color: String,
    #[serde(default = "default_model_color")]
    pub model_color: String,
```

default 函数（放在 `default_bar_filled` 附近，值 = `Theme::default()` 的 nord 色）：

```rust
fn default_bg() -> String { "#2e3440".into() }
fn default_fg() -> String { "#d8dee9".into() }
fn default_accent() -> String { "#88c0d0".into() }
fn default_success() -> String { "#a3be8c".into() }
fn default_warning() -> String { "#ebcb8b".into() }
fn default_danger() -> String { "#bf616a".into() }
fn default_muted() -> String { "#5e81ac".into() }
fn default_border() -> String { "#434c5e".into() }
fn default_skill_color() -> String { "#b48ead".into() }
fn default_mcp_color() -> String { "#d08770".into() }
fn default_model_color() -> String { "#88c0d0".into() }
```

`src/core/config.rs:28`：`pub theme: Option<Theme>;` → `pub theme: Option<ThemeRef>;`，import 改为 `use super::theme::{Theme, ThemeRef};`。

- [ ] **Step 4: 跑测试确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -5`
Expected: `test result: ok. N passed`（原 57 + 新增 4 = 61；`theme_ref_string_preset_parses` 仅验证枚举变体存在）

- [ ] **Step 5: Commit（用户执行）**

```bash
git add src/core/theme.rs src/core/config.rs
git commit -m "feat: ThemeRef dual-form (preset string | table) + 11 color serde defaults"
```

---

### Task 2: 主题叠加链（apply_theme_keys + merge_theme + resolve_theme + 失败警告）

**Files:**
- Modify: `src/core/theme.rs`
- Modify: `src/core/config.rs`
- Modify: `src/main.rs:114,115,177-191,433`
- Test: `src/core/theme.rs`、`src/core/config.rs`

- [ ] **Step 1: 写失败测试**

`src/core/theme.rs` tests 追加：

```rust
#[test]
fn apply_theme_keys_color_numeric_and_enum() {
    let mut base = Theme::default();
    let keys: HashMap<String, toml::Value> = toml::from_str(
        "accent = \"#123456\"\nbar_width = 20\nicon_set = \"nerd\"\n",
    ).unwrap();
    apply_theme_keys(&mut base, &keys);
    assert_eq!(base.accent, "#123456");
    assert_eq!(base.bar_width, 20);
    assert!(matches!(base.icon_set, IconSet::Nerd));
    assert_eq!(base.bg, "#2e3440"); // 未提供的键不变
}

#[test]
fn apply_theme_keys_unknown_ignored() {
    let mut base = Theme::default();
    let keys: HashMap<String, toml::Value> =
        toml::from_str("future_key = \"x\"\n").unwrap();
    apply_theme_keys(&mut base, &keys);
    assert_eq!(base.accent, "#88c0d0");
}

#[test]
fn apply_theme_keys_enum_bad_value_keeps_base() {
    let mut base = Theme::default();
    let keys: HashMap<String, toml::Value> =
        toml::from_str("icon_set = \"bogus\"\n").unwrap();
    apply_theme_keys(&mut base, &keys);
    assert!(matches!(base.icon_set, IconSet::Auto));
}

#[test]
fn apply_theme_keys_char_tokens() {
    let mut base = Theme::default();
    let keys: HashMap<String, toml::Value> =
        toml::from_str("bar_filled = \"■\"\n").unwrap();
    apply_theme_keys(&mut base, &keys);
    assert_eq!(base.bar_filled, '■');
}
```

`src/core/config.rs` tests 追加：

```rust
#[test]
fn merge_theme_layers_order() {
    // 基底 dracula → config 键层 accent → config overrides accent →
    // mod overrides accent：后者胜出
    let base = Theme::load_preset("dracula").unwrap();
    let config_keys: HashMap<String, toml::Value> =
        toml::from_str("accent = \"#111111\"\n").unwrap();
    let config_ov: HashMap<String, toml::Value> =
        toml::from_str("accent = \"#222222\"\n").unwrap();
    let mod_ov: HashMap<String, toml::Value> =
        toml::from_str("accent = \"#333333\"\n").unwrap();
    let mut merged = base;
    apply_theme_keys(&mut merged, &config_keys);
    apply_theme_keys(&mut merged, &config_ov);
    apply_theme_keys(&mut merged, &mod_ov);
    assert_eq!(merged.accent, "#333333");
    assert_eq!(merged.bg, "#282a36"); // dracula 底色保留
}

#[test]
fn resolve_theme_string_preset_without_mod() {
    let cfg: AppConfig = toml::from_str(
        "active_mod = \"\"\ntheme = \"dracula\"\n",
    ).unwrap();
    let r = cfg.resolve_theme();
    assert_eq!(r.preset.as_deref(), Some("dracula"));
    assert_eq!(r.theme.bg, "#282a36");
}

#[test]
fn resolve_theme_partial_table_overrides_default() {
    let cfg: AppConfig = toml::from_str(
        "active_mod = \"\"\n[theme]\naccent = \"#ff0000\"\n",
    ).unwrap();
    let r = cfg.resolve_theme();
    assert_eq!(r.theme.accent, "#ff0000");
    assert_eq!(r.theme.bg, "#2e3440"); // 其余为 default nord
}

#[test]
fn resolve_theme_preset_and_overrides_table() {
    let cfg: AppConfig = toml::from_str(
        "active_mod = \"\"\n[theme]\npreset = \"dracula\"\n[theme.overrides]\naccent = \"#ff0000\"\n",
    ).unwrap();
    let r = cfg.resolve_theme();
    assert_eq!(r.preset.as_deref(), Some("dracula"));
    assert_eq!(r.theme.bg, "#282a36");
    assert_eq!(r.theme.accent, "#ff0000");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -8`
Expected: `error[E0425]: cannot find function 'apply_theme_keys'` 等

- [ ] **Step 3: 实现**

`src/core/theme.rs` 加（`impl Theme` 之后、`detect_nerd_font` 之前）：

```rust
/// 合并结果：基底 preset 名 + 完整主题。mod save 快照需要知道基底名。
#[derive(Debug, Clone)]
pub struct ResolvedTheme {
    pub preset: Option<String>,
    pub theme: Theme,
}

/// 将键表中与 Theme 20 字段同名的键类型化覆盖到 base；未知键忽略。
/// colors 与 overrides 共用（唯一的差别是调用层级）。
pub fn apply_theme_keys(base: &mut Theme, keys: &HashMap<String, toml::Value>) {
    for (k, v) in keys {
        match k.as_str() {
            "bg" => base.bg = v.as_str().unwrap_or(&base.bg).to_string(),
            "fg" => base.fg = v.as_str().unwrap_or(&base.fg).to_string(),
            "accent" => base.accent = v.as_str().unwrap_or(&base.accent).to_string(),
            "success" => base.success = v.as_str().unwrap_or(&base.success).to_string(),
            "warning" => base.warning = v.as_str().unwrap_or(&base.warning).to_string(),
            "danger" => base.danger = v.as_str().unwrap_or(&base.danger).to_string(),
            "muted" => base.muted = v.as_str().unwrap_or(&base.muted).to_string(),
            "border" => base.border = v.as_str().unwrap_or(&base.border).to_string(),
            "skill_color" => base.skill_color = v.as_str().unwrap_or(&base.skill_color).to_string(),
            "mcp_color" => base.mcp_color = v.as_str().unwrap_or(&base.mcp_color).to_string(),
            "model_color" => base.model_color = v.as_str().unwrap_or(&base.model_color).to_string(),
            "separator" => base.separator = v.as_str().unwrap_or(&base.separator).to_string(),
            "bar_filled" => {
                if let Some(c) = v.as_str().and_then(|s| s.chars().next()) {
                    base.bar_filled = c;
                }
            }
            "bar_empty" => {
                if let Some(c) = v.as_str().and_then(|s| s.chars().next()) {
                    base.bar_empty = c;
                }
            }
            "bar_width" => {
                if let Some(i) = v.as_integer() {
                    base.bar_width = i as u16;
                }
            }
            "padding" => {
                if let Some(i) = v.as_integer() {
                    base.padding = i as u16;
                }
            }
            "compact_lines" => {
                if let Some(i) = v.as_integer() {
                    base.compact_lines = i as u8;
                }
            }
            "dashboard_grid" => {
                if let Some(i) = v.as_integer() {
                    base.dashboard_grid = i as u8;
                }
            }
            "icon_set" => {
                if let Some(s) = v.as_str() {
                    base.icon_set = match s {
                        "auto" => IconSet::Auto,
                        "nerd" => IconSet::Nerd,
                        "ascii" => IconSet::Ascii,
                        "minimal" => IconSet::Minimal,
                        _ => base.icon_set,
                    };
                }
            }
            "border_style" => {
                if let Some(s) = v.as_str() {
                    base.border_style = match s {
                        "single" => BorderStyle::Single,
                        "double" => BorderStyle::Double,
                        "rounded" => BorderStyle::Rounded,
                        "thick" => BorderStyle::Thick,
                        "hidden" => BorderStyle::Hidden,
                        _ => base.border_style,
                    };
                }
            }
            _ => {}
        }
    }
}
```

`src/core/config.rs` 加（`impl AppConfig` 内，`widget_config` 之后）：

```rust
    /// 主题叠加链：基底(mod preset 或 config preset 或 default) →
    /// config.theme 显式键 → config.theme.overrides → mod.theme.overrides。
    pub fn resolve_theme(&self) -> ResolvedTheme {
        let mut preset_name: Option<String> = None;
        let mut base = Theme::default();
        if !self.active_mod.is_empty() {
            if let Ok(pkg) = Self::load_mod(&self.active_mod) {
                if let Some(mt) = &pkg.theme {
                    if let Some(t) = Theme::load_preset(&mt.preset) {
                        base = t;
                        preset_name = Some(mt.preset.clone());
                    }
                }
            }
        }
        if let Some(tr) = &self.theme {
            match tr {
                ThemeRef::Preset(p) => {
                    if preset_name.is_none() {
                        if let Some(t) = Theme::load_preset(p) {
                            base = t;
                            preset_name = Some(p.clone());
                        }
                    }
                }
                ThemeRef::Table(tbl) => {
                    if preset_name.is_none() {
                        if let Some(p) = &tbl.preset {
                            if let Some(t) = Theme::load_preset(p) {
                                base = t;
                                preset_name = Some(p.clone());
                            }
                        }
                    }
                    apply_theme_keys(&mut base, &tbl.colors);
                    if let Some(ov) = &tbl.overrides {
                        apply_theme_keys(&mut base, ov);
                    }
                }
            }
        }
        if !self.active_mod.is_empty() {
            if let Ok(pkg) = Self::load_mod(&self.active_mod) {
                if let Some(mt) = &pkg.theme {
                    if let Some(ov) = &mt.overrides {
                        apply_theme_keys(&mut base, ov);
                    }
                }
            }
        }
        ResolvedTheme { preset: preset_name, theme: base }
    }
```

import 补：`use super::theme::{ResolvedTheme, Theme, ThemeRef};` 和 `use super::theme::apply_theme_keys;`

`src/main.rs`：

```rust
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
```

删除 `fn load_theme`（main.rs:177-191），`theme export` 分支（main.rs:433）改为：

```rust
        ThemeCommands::Export => {
            let theme = config.resolve_theme().theme;
            let toml_str =
                toml::to_string_pretty(&theme).map_err(|e| format!("serialize: {}", e))?;
            print!("{}", toml_str);
        }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -5`
Expected: `test result: ok. 69 passed`（61 + 新增 8）

- [ ] **Step 5: Commit（用户执行）**

```bash
git add src/core/theme.rs src/core/config.rs src/main.rs
git commit -m "feat: theme merge chain — base + config keys + overrides layers + config failure warning"
```

---

### Task 3: theme import 落盘 + DESIGN.md 修正

**Files:**
- Modify: `src/main.rs:438-444`
- Create: `fixtures/theme/nord_partial.toml`
- Modify: `scripts/hudlib/cases.py`（P3-06）
- Modify: `DESIGN.md`（§ 三级配置深度）

- [ ] **Step 1: 写失败黑盒用例（cases.py 追加）**

`fixtures/theme/nord_partial.toml` 新建：

```toml
accent = "#ff00ff"
bar_width = 20
```

cases.py（P3 列表，`render_case` 前加 `import_case`/`assert_file` 辅助或复用现有 spec 键——**用现有 keys**，追加到 P2 之后）：

```python
    render_case("P3-06", "theme import 落盘保留其他段", "P3",
                {"exit": 0, "stdout_contains": ["imported"]},
                stdin=j(full_dict()), config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "compact_layout = [\"model_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                pre_cmds=[["theme", "import",
                           os.path.join(FIXTURES, "theme", "nord_partial.toml")]],
                note="任务⑤：import 写入 config.toml [theme] 段，active_mod 段保留")
```

（在 cases.py 中 `import_case` 不存在——检查现有 pre_cmds 机制：P2 用例用 `pre_cmds=[["mod", "use", "noir-tabbed"]]`，说明 harness 支持前置命令 + config 后写。P3-06 需在 import 后**读回 config.toml 断言**——新增 spec 键 `file_contains` 或复用现有？先确认 cases.py 是否已有文件断言键，没有则 Task 3 一并加 `config_file_contains` 断言键。）

- [ ] **Step 2: 跑黑盒确认失败**

Run: `python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")" --case P3-06 2>&1 | tail -5`
Expected: `FAIL`（import 不落盘——现状只打印提示）

- [ ] **Step 3: 实现 `theme import` 落盘**

`src/main.rs` handle_theme Import 分支重写：

```rust
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
                .map_err(|_| "config.toml is not a table".to_string())?
                .insert("theme".into(), table);
            let out = toml::to_string_pretty(&root)
                .map_err(|e| format!("serialize config: {}", e))?;
            std::fs::write(&config_path, out)
                .map_err(|e| format!("write config: {}", e))?;
            println!("Theme imported to config.toml [theme] section");
        }
```

- [ ] **Step 4: 黑盒确认通过**

Run: `python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")" --case P3-06 2>&1 | tail -3`
Expected: `PASS`

- [ ] **Step 5: DESIGN.md 修正（§ 三级配置深度，约 290 行附近）**

将「字符串 / 字符串+overrides / custom 全表」三种写法替换为：

```markdown
### 主题引用（三级配置深度）

| 级别 | 写法 | 语义 |
|------|------|------|
| 1 | `theme = "dracula"` | 字符串预设名，替换基底 |
| 2 | `[theme] preset = "dracula"` + `[theme.overrides]` | 预设 + 微调 |
| 3 | `[theme] accent = "#ff0000"`（部分/完整表） | 显式键逐键覆盖基底 |

叠加顺序（自低到高）：基底（active_mod 的 mod preset，否则 config preset，否则默认）
→ config `[theme]` 显式键 → config `[theme.overrides]` → mod `[mod.theme.overrides]`。
config 的 `theme = "..."` 字符串在 active_mod 存在时不参与叠加（基底已由 mod 决定）。
坏 config 不再静默：stderr 警告 + doctor `[!!]`。
```

- [ ] **Step 6: Commit（用户执行）**

```bash
git add src/main.rs fixtures/theme/nord_partial.toml scripts/hudlib/cases.py DESIGN.md
git commit -m "feat: theme import persists [theme] section; DESIGN.md 3-level theme reference corrected"
```

---

### Task 4: Mod 命令层（use 校验 + previous_mod + @scene + save 快照 + pick）

**Files:**
- Modify: `src/core/state.rs`（previous_mod 字段）
- Modify: `src/main.rs`（handle_mod 重构 + config_to_mod 重写 + scene_alias/find_mod_by_scene/diff_theme）
- Test: `src/main.rs`（scene_alias）、`src/core/state.rs`（previous_mod 往返）

- [ ] **Step 1: 写失败测试**

`src/core/state.rs` tests 追加：

```rust
#[test]
fn previous_mod_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let mut st = StateFile::default();
    st.previous_mod = Some("noir-tabbed".into());
    st.write(&path).unwrap();
    let back = StateFile::read(&path);
    assert_eq!(back.previous_mod.as_deref(), Some("noir-tabbed"));
}
```

（tempfile 已是 dev-dependency 吗？检查 Cargo.toml——若没有，用 `std::env::temp_dir()` 拼唯一路径，写完清理。**改用临时路径方式**，避免新依赖。）

`src/main.rs` tests 模块（文件底部若没有则新建）：

```rust
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
```

- [ ] **Step 2: 跑测试确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test previous_mod scene_alias 2>&1 | tail -6`
Expected: `error[E0603]: field 'previous_mod' of struct 'StateFile' is private`（不存在）等

- [ ] **Step 3: 实现**

`src/core/state.rs` StateFile 结构追加（last_error 之后）：

```rust
    /// mod use 的历史切换记录（`mod use -` 往返 toggle）。
    #[serde(default)]
    pub previous_mod: Option<String>,
```

`src/main.rs`：

```rust
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
    const BUILTINS: [&str; 6] = [
        "glacier-workstation",
        "obsidian-command",
        "ember-night",
        "matrix-surveillance",
        "noir-precision",
        "noir-tabbed",
    ];
    for name in BUILTINS {
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
```

`handle_mod` Use 分支重写：

```rust
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
                println!("Switched back to mod '{}' ✓", prev);
                return Ok(());
            }
            let target = resolve_mod_target(&name)?;
            let state_path = AppConfig::state_path()?;
            let mut st = StateFile::read(&state_path);
            st.previous_mod = Some(config.active_mod.clone());
            st.write(&state_path)
                .map_err(|e| format!("write state: {}", e))?;
            write_active_mod(config, &target)?;
            println!("Switched to mod '{}' ✓", target);
        }
```

`handle_mod` 顶部 import 补：`use crate::core::state::StateFile;`

`config_to_mod` 重写（main.rs:404-428）：

```rust
const MINIMAL_WIDGETS: [&str; 4] =
    ["model_display", "context_bar", "cost_display", "git_status"];
const ACTIVITY_WIDGETS: [&str; 7] = [
    "model_display", "context_bar", "agent_overview",
    "git_status", "skills_mcp", "cost_display", "rate_limits",
];

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
    use std::collections::HashMap;
    let resolved = config.resolve_theme();
    let base = resolved
        .preset
        .as_deref()
        .and_then(Theme::load_preset)
        .unwrap_or_default();
    let mut overrides: HashMap<String, toml::Value> = HashMap::new();
    diff_theme(&base, &resolved.theme, &mut overrides);
    let layout_id = match config.compact_layout.as_slice() {
        MINIMAL_WIDGETS => "minimal".to_string(),
        ACTIVITY_WIDGETS => "activity".to_string(),
        _ => "custom".to_string(),
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
```

`src/core/config.rs` ModPackage 加字段：

```rust
    /// 保存时的 compact widget 数组快照（布局 ID 之外的完整保留）。
    #[serde(default)]
    pub compact_widgets: Option<Vec<String>>,
```

`mod pick` 分支重写：

```rust
        ModCommands::Pick => {
            let mut items: Vec<String> = Vec::new();
            for name in Theme::preset_names() {
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
            println!("Switched to mod '{}' ✓", target);
        }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -5`
Expected: `test result: ok. 72 passed`（69 + previous_mod 1 + scene_alias 1 + 编译期新字段）

- [ ] **Step 5: Commit（用户执行）**

```bash
git add src/core/state.rs src/core/config.rs src/main.rs
git commit -m "feat: mod use validation + previous_mod toggle + @scene aliases + save snapshot + pick"
```

---

### Task 5: Mod 渲染灌入（compact.rs 布局与行数）

**Files:**
- Modify: `src/compact.rs`
- Test: `src/compact.rs`

- [ ] **Step 1: 写失败测试（compact.rs tests 追加）**

```rust
    #[test]
    fn layout_from_mod_widgets_win() {
        let widgets = vec!["model_display".to_string(), "cost_display".to_string()];
        let got = layout_from_mod(Some(&widgets), "minimal").unwrap();
        assert_eq!(got, widgets);
    }

    #[test]
    fn layout_from_mod_minimal_maps() {
        let got = layout_from_mod(None, "minimal").unwrap();
        assert_eq!(got, vec!["model_display", "context_bar", "cost_display", "git_status"]);
    }

    #[test]
    fn layout_from_mod_activity_maps() {
        let got = layout_from_mod(None, "activity").unwrap();
        assert_eq!(got.len(), 7);
        assert_eq!(got[0], "model_display");
        assert_eq!(got[6], "rate_limits");
    }

    #[test]
    fn layout_from_mod_unknown_errors() {
        let err = layout_from_mod(None, "agent-centric").unwrap_err();
        assert!(err.contains("not implemented"));
    }

    #[test]
    fn lines_from_layers_priority() {
        assert_eq!(lines_from_layers(Some(3), Some(2), 1), 3);
        assert_eq!(lines_from_layers(None, Some(2), 1), 2);
        assert_eq!(lines_from_layers(None, None, 1), 1);
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test layout_from_mod lines_from_layers 2>&1 | tail -6`
Expected: `error[E0425]: cannot find function 'layout_from_mod'`

- [ ] **Step 3: 实现**

`src/compact.rs` 顶部常量 + 纯函数：

```rust
pub const MINIMAL_WIDGETS: [&str; 4] =
    ["model_display", "context_bar", "cost_display", "git_status"];
pub const ACTIVITY_WIDGETS: [&str; 7] = [
    "model_display", "context_bar", "agent_overview",
    "git_status", "skills_mcp", "cost_display", "rate_limits",
];

/// 布局解析：compact_widgets 快照 > 布局 ID 映射（minimal/activity）> 其他。
/// 未知布局 ID 返回 Err（render 报错路径，hud_err_marker 上屏）。
pub fn layout_from_mod(
    compact_widgets: Option<&Vec<String>>,
    layout_compact: &str,
) -> Result<Vec<String>, String> {
    if let Some(widgets) = compact_widgets {
        return Ok(widgets.clone());
    }
    let ids: &[&str] = match layout_compact {
        "minimal" => &MINIMAL_WIDGETS,
        "activity" => &ACTIVITY_WIDGETS,
        other => return Err(format!("compact layout '{}' not implemented", other)),
    };
    Ok(ids.iter().map(|s| s.to_string()).collect())
}

/// 行数三层优先级：runtime_overrides > mod.layout > theme。
pub fn lines_from_layers(runtime: Option<u8>, mod_lines: Option<u8>, theme: u8) -> u8 {
    runtime.or(mod_lines).unwrap_or(theme)
}

/// 当前生效的 compact widget 数组（mod 灌入优先，fallback config）。
pub fn resolve_compact_layout(config: &AppConfig) -> Result<Vec<String>, String> {
    if !config.active_mod.is_empty() {
        if let Ok(pkg) = AppConfig::load_mod(&config.active_mod) {
            return layout_from_mod(pkg.compact_widgets.as_ref(), pkg.layout.as_ref().map(|l| l.compact.as_str()).unwrap_or(""));
        }
    }
    Ok(config.compact_layout.clone())
}

/// 当前生效的 mod compact_lines（无 mod 或加载失败 → None）。
pub fn mod_compact_lines(config: &AppConfig) -> Option<u8> {
    if config.active_mod.is_empty() {
        return None;
    }
    AppConfig::load_mod(&config.active_mod)
        .ok()
        .and_then(|pkg| pkg.layout)
        .map(|l| l.compact_lines)
}
```

`render_with_data` 修改（compact.rs:103-112）：

```rust
    let layout = resolve_compact_layout(config)?;
    if layout.is_empty() {
        return Ok(String::new());
    }

    let lines = lines_from_layers(
        config.runtime_overrides.as_ref().and_then(|o| o.compact_lines),
        mod_compact_lines(config),
        theme.compact_lines,
    ) as usize;
```

- [ ] **Step 4: 跑测试确认通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -5`
Expected: `test result: ok. 77 passed`（72 + 5）

- [ ] **Step 5: 黑盒用例（cases.py 追加 P3-10/P3-11）**

```python
    render_case("P3-10", "mod save→use 自定义数组渲染一致", "P3",
                {"exit": 0, "stdout_contains": ["model_display 布局"]},
                stdin=j(full_dict()), config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"model_display\", \"cost_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                pre_cmds=[["mod", "save", "my-custom"],
                          ["mod", "use", "my-custom"]],
                note="任务⑥：save 快照 compact_widgets，use 后按数组渲染"),
    render_case("P3-11", "未实现布局 ID 明确报错", "P3",
                {"exit": -1, "stdout_contains": ["not implemented"]},
                stdin=j(full_dict()), config=(
                    "active_mod = \"obsidian-command\"\n"
                    "preset = \"full\"\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="任务⑥：agent-centric 无 compact_widgets → 渲染报错"),
```

（P3-10 断言文案待定——save 后 use my-custom 的渲染结果 = [model_display, cost_display] 数组的输出，断言具体子串如 model 名 + "$"。执行时按实际输出微调断言。）

- [ ] **Step 6: 黑盒确认通过**

Run: `python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")" --case P3-10 --case P3-11 2>&1 | tail -5`
Expected: 两例 PASS

- [ ] **Step 7: Commit（用户执行）**

```bash
git add src/compact.rs scripts/hudlib/cases.py
git commit -m "feat: mod layout drives compact render — compact_widgets + minimal/activity maps + lines priority"
```

---

### Task 6: ANSI 四处修复 + stdout_raw_regex 断言

**Files:**
- Modify: `src/widgets/rate_limits.rs:23-25`
- Modify: `src/widgets/session_stats.rs:42-52`
- Modify: `src/widgets/token_attribution.rs:35-39`
- Modify: `src/widgets/cost_display.rs:23-28`
- Modify: `scripts/hudlib/assertions.py`
- Modify: `scripts/hudlib/cases.py`（P3-13..15）

- [ ] **Step 1: 写失败黑盒用例 + 断言键**

`scripts/hudlib/assertions.py` check() 加（stdout_not_contains 之后）：

```python
    if "stdout_raw_regex" in spec:
        if not re.search(spec["stdout_raw_regex"], result.stdout):
            fails.append(f"stdout raw regex no match: {spec['stdout_raw_regex']!r}")
```

cases.py 追加：

```python
    render_case("P3-13", "rate_limits 超阈值数字在色内", "P3",
                {"exit": 0, "stdout_contains": ["92%"],
                 "stdout_raw_regex": r"\x1b\[38;2;[0-9;]+m[^\x1b]*[0-9]+%[^\x1b]*"},
                stdin=j(full_dict()), config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "compact_layout = [\"rate_limits\"]\n"
                    "rate_limit_warn = 90\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="任务⑦：92% 超过 warn=90，数字整体在 danger 色内"),
    render_case("P3-14", "session_stats 三色生效", "P3",
                {"exit": 0, "stdout_contains": ["tok/s"],
                 "stdout_raw_regex": r"(\x1b\[38;2;[0-9;]+m[^\x1b]+){3,}"},
                stdin_file="json/full.json", config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "compact_layout = [\"session_stats\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="任务⑦：≥3 组色码且每组色内非空"),
    render_case("P3-15", "cost_display 符号+数字整体在色内", "P3",
                {"exit": 0,
                 "stdout_raw_regex": r"\x1b\[38;2;[0-9;]+m\$[0-9.]+[^\x1b]*"},
                stdin_file="json/full.json", config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "compact_layout = [\"cost_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="任务⑦：$0.03 数字在色码内"),
```

（P3-13 的 rate_limit_warn 键：现有 WidgetConfig 键名是 `rate_limit_warn`（rate_limits.rs:20 `config.get_f64("rate_limit_warn", 90.0)`）✓；full.json 的 rate_limits 值需 ≥90 才触发——检查 full.json 的 five_hour 值，不足则在用例 stdin 里覆写或用 j(full_dict()) 修改。执行时确认。）

- [ ] **Step 2: 跑黑盒确认失败**

Run: `python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")" --case P3-13 --case P3-14 --case P3-15 2>&1 | tail -6`
Expected: 三例 FAIL（空 wrap：色内无数字文本）

- [ ] **Step 3: 四处修复**

`rate_limits.rs:23-25`：

```rust
        format!("5h:{} 7d:{}",
            ansi::ansi_fg(&format!("{:.0}%", fh), fc),
            ansi::ansi_fg(&format!("{:.0}%", sd), sc))
```

`session_stats.rs:42-52`：

```rust
        format!("{} {} {}",
            ansi::ansi_fg(&format!("⏱{}", dur_str), &theme.fg),
            ansi::ansi_fg(&format!("{}tok/s", tok_per_sec), &theme.accent),
            ansi::ansi_fg(&format!("{}calls", total_tool_calls), &theme.muted))
```

`token_attribution.rs:35-39`：

```rust
                    return format!("{}",
                        ansi::ansi_fg(&format!("top:{} {:.0}%", top_agent.name, pct), &theme.accent));
```

`cost_display.rs:23-28`：

```rust
        format!("{}",
            ansi::ansi_fg(&format!("{}{}{:.2}", prefix, symbol, cost), color))
```

- [ ] **Step 4: 黑盒确认通过 + 全量回归**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1 | tail -2 && python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")" 2>&1 | tail -3`
Expected: 黑盒 121/121（106 旧例 + P3-01..15，P3-01..05 在 Task 2/3 期间已加入）

- [ ] **Step 5: Commit（用户执行）**

```bash
git add src/widgets/rate_limits.rs src/widgets/session_stats.rs \
        src/widgets/token_attribution.rs src/widgets/cost_display.rs \
        scripts/hudlib/assertions.py scripts/hudlib/cases.py
git commit -m "fix: wrap whole text in ansi_fg (4 widgets) + stdout_raw_regex assertions"
```

---

### Task 7: 全量验证 + 文档回写

**Files:**
- Modify: `DEPLOY.md`（mod save/use -/@scene/pick 描述）
- Modify: `COMPLETE.md`（§20/§21）
- Modify: `CHANGELOG.md`

- [ ] **Step 1: 全量验证**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test && python scripts/test_hud.py --exe "$(cygpath -w "$PWD/target/debug/claude-hud.exe")" && ./target/debug/claude-hud.exe doctor`
Expected: cargo test 全绿（~77）；黑盒 121/121；doctor 全过（contract probe 不变）

- [ ] **Step 2: DEPLOY.md 修正**

- `mod save`：从「生成固定模板」改为「当前配置真实快照（theme 合并结果 + compact_widgets + widgets 段）」描述
- `mod use -` / `mod use @scene` / `mod pick`：删除占位文案，按实现描述

- [ ] **Step 3: COMPLETE.md 更新**

- §20 ✅ 段追加：`· 配置契约（ThemeRef 双形态 + 四层叠加 + 失败警告 + import 落盘）· Mod 真相（use 校验 + previous_mod + @scene + save 快照 + 渲染灌入 + pick）· ANSI 整段上色（4 widget + 黑盒 ANSI 结构断言）`
- §20 🟡 表删除/更新：`Mod overrides` 行、`mod pick / mod use -` 占位行、`theme import` 行
- §21 roadmap 加行：`| Phase 3 配置契约 | ThemeRef 双形态 + Mod 系统真相 + ANSI 修复 + 黑盒用例 121 例 | ✅ |`
- 页脚时间戳更新

- [ ] **Step 4: CHANGELOG.md 追加**

```markdown
## [0.2.0] - 2026-08-03 (Phase 3)
### Added
- theme 支持字符串预设 / [theme] 表 / preset+overrides 三种引用形态
- mod use 校验、mod use - 往返切换、@scene 场景别名、mod pick 序号选择器
- mod save 真实配置快照（compact_widgets 字段）
- theme import 落盘 config.toml [theme] 段
### Fixed
- 4 个 widget ANSI 空字符串上色（数字/符号整体入色）
- 坏 config 不再静默（stderr 警告 + doctor [!!]）
```

- [ ] **Step 5: Commit（用户执行）**

```bash
git add DEPLOY.md COMPLETE.md CHANGELOG.md
git commit -m "docs: Phase 3 status — theme contract, mod truth, ANSI fixes"
```

---

## 自检（spec 覆盖对照）

| spec 需求 | 落点 |
|-----------|------|
| ThemeRef untagged 两形态 + 11 色 default | Task 1 |
| 叠加链顺序（基底 → config 键 → config overrides → mod overrides）| Task 2（merge_theme/resolve_theme）|
| 失败警告 + doctor 检查（后者 Phase 1.5 已有）| Task 2 Step 3（main.rs eprintln）|
| theme import 落盘 + 保留其他段 | Task 3 |
| DESIGN.md 三级深度正确形态 | Task 3 Step 5 |
| mod use 校验（不污染 config）| Task 4（resolve_mod_target）|
| previous_mod 往返 toggle（state.json）| Task 4（StateFile.previous_mod）|
| @scene 固定别名表 + 兜底 | Task 4（scene_alias + find_mod_by_scene）|
| save 真实快照 + compact_widgets | Task 4（config_to_mod + ModPackage 字段）|
| layout 灌入渲染 + compact_lines 三层 | Task 5 |
| mod pick 序号选择器 | Task 4 |
| 4 处 ANSI 整段包色 | Task 6 |
| stdout_raw_regex 断言 + 色内非空 | Task 6 |
| DEPLOY/COMPLETE/CHANGELOG 回写 | Task 7 |

**已知待执行时确认点**：
1. P3-13 的 full.json five_hour 值是否 ≥ 90（不足则用例 stdin 覆写）
2. P3-10 的断言子串按实际渲染输出微调
3. Task 3 Step 1 的 cases.py 文件断言键（P3-06 需断言 config.toml 内容——若 harness 无文件断言键，用 `pre_cmds` + 新增 `file_contains` spec 键或在 runner 层实现）
