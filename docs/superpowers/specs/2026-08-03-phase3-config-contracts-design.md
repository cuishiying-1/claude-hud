# Phase 3 — 配置契约（任务 ⑤⑥⑦）设计

> 来源：TASKS.md 批次 C 前三个任务。前置：Phase 1/1.5（批次 A）+ Phase 2（批次 B）已完成。

## 1. 背景与范围

| 任务 | 主题 | 核心问题 |
|------|------|----------|
| ⑤ | 主题配置契约 | 文档教的三种写法全部或部分失效，坏 config 静默作废，overrides 形同虚设，import 不落盘 |
| ⑥ | Mod 系统真相 | save 是固定模板、use 不校验、@scene 别名坏、layout 不驱动渲染、pick 是占位 |
| ⑦ | ANSI 上色失效 | 4 处把空字符串包进色码，要着色的数字在色外；黑盒断言不查 ANSI 结构 |

**贯穿原则**（延续 TASKS.md）：诚实降级 · 失败可见 · 不留占位/死代码。

## 2. 拍板决策记录

| # | 问题 | 拍板 |
|---|------|------|
| D1 | `[theme] preset + overrides` 形态建模 | ThemeRef **两形态**：`Preset(String)` \| `Table{preset, overrides, colors(flatten)}`（演进说明见 §3.2——三形态 untagged 有顺序陷阱，flatten 方案更干净且满足"两形态"拍板） |
| D2 | active_mod 存在时 config 的 `[theme]` 是否叠加 | **总是叠加**：基底 → config.theme 颜色/overrides → mod.theme.overrides（最高） |
| D3 | `mod use @scene` 别名解析 | **固定别名表**（@daily→daily-dev、@night→night-coding、@agent→heavy-agent、@ssh→ssh-remote）+ 精确 scene/mod 名兜底 |
| D4 | mod save 的 compact 布局快照 | **ModPackage 加 `compact_widgets: Option<Vec<String>>`** 字段，save 写当前 widget 数组，渲染时优先 |

## 3. 任务 ⑤ — 主题配置契约

### 3.1 现状证据（src）

- `config.rs:28`：`theme: Option<Theme>` 只接受完整表 → `theme = "dracula"` 解析失败。
- `theme.rs:7-17`：11 个颜色 token 无 `#[serde(default)]` → 部分表解析失败。
- `main.rs:114`：`AppConfig::load().unwrap_or_default()` 静默吞错。
- `main.rs:177-191`：`load_theme` 只取 mod preset，`ModTheme.overrides` 从未应用。
- `main.rs:438-444`：`theme import` 只校验不落盘。
- doctor.rs:24-27 已有 "config.toml exists and parses" 检查项（Phase 1.5 落地），⑤ 方案 3 的 doctor 部分已存在，无需新增。

### 3.2 ThemeRef 设计

```rust
// theme.rs（或 config.rs）
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ThemeRef {
    /// theme = "dracula"
    Preset(String),
    /// [theme] ...（部分表/完整表/preset+overrides 统一走此形态）
    Table(ThemeTable),
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ThemeTable {
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub overrides: Option<HashMap<String, toml::Value>>,
    /// 显式提供的任意主题键（11 色 + 9 风格 token），flatten 保证"哪些键
    /// 被显式写出"可检测——这是叠加合并正确性的关键。
    #[serde(flatten)]
    pub colors: HashMap<String, toml::Value>,
}
```

**为何弃三形态**：untagged 枚举按声明顺序尝试，`Overlay{preset, overrides}` 若全 Option 字段会吞掉任何表（Full 不可达）；若 preset 必填则纯颜色部分表匹配不上。flatten 捕获任意键 + Option 元字段，两形态天然互斥（字符串 vs 表），无顺序陷阱。

**untagged 匹配验证**：
- `theme = "dracula"` → `Preset("dracula")`
- `[theme] preset="dracula" overrides={...}` → `Table`（preset+overrides）
- `[theme] accent="#fff"` → `Table`（colors={accent}）
- `[theme]` 空表 → `Table`（全 None，不报错）
- 完整 20 token 表 → `Table`（colors=全部）

**theme.rs 颜色默认值**：11 个颜色 token 各加 `#[serde(default = "default_xxx")]`，default 函数返回 `Theme::default()` 的 nord 色（bg #2e3440、fg #d8dee9、accent #88c0d0、success #a3be8c、warning #ebcb8b、danger #bf616a、muted #5e81ac、border #434c5e、skill_color #b48ead、mcp_color #d08770、model_color #88c0d0）。9 个风格 token 已有 default。

**config.rs 改动**：`theme: Option<Theme>` → `Option<ThemeRef>`（serde default 保留）。

### 3.3 load_theme 叠加链

```rust
/// 基底 preset 名 + 合并后的完整主题。save 快照需要知道基底名。
pub struct ResolvedTheme {
    pub preset: Option<String>,   // 基底 preset 名（default 时 None）
    pub theme: Theme,
}
```

合并顺序（自低到高）：
1. **基底**：active_mod 的 mod.theme.preset → `load_preset`；否则 config.theme 的 preset（Preset 形态或 Table.preset）→ `load_preset`；否则 `Theme::default()`（基底 preset 名记 None）。
2. **config.theme 显式颜色/风格键**：Table.colors 中与 Theme 20 字段同名的键，逐键类型化覆盖基底（11 色 = String；bar_width/padding/compact_lines/dashboard_grid = 整数；bar_filled/bar_empty = 字符串首字符；separator = String；icon_set/border_style = snake_case 枚举解析）。未知键忽略（不报错——容忍新键）。
3. **config.theme.overrides**：逐键覆盖（toml::Value → String，非 String 值忽略）。
4. **mod.theme.overrides**：同上，优先级最高。

> D2 行为变化：active_mod 存在时，config 的 `[theme]` 微调现在也生效（此前完全忽略）。

**load_theme 签名变化**：`load_theme(config) -> Theme` 改为 `resolve_theme(config) -> ResolvedTheme`，调用点适配：main.rs:115（主入口，`.theme` 取用）、main.rs:433（theme export 输出合并结果）、doctor.rs sample_render 无需变（走 config 参数）。`theme export` 输出 = 合并后的完整 Theme（现状即如此）。

### 3.4 失败不再静默

main.rs:114 改为：
```rust
let config = match AppConfig::load() {
    Ok(c) => c,
    Err(e) => {
        eprintln!("[claude-hud] warning: config.toml parse failed ({}); using defaults", e);
        AppConfig::default()
    }
};
```
doctor 检查项已存在（§3.1），仅确认 `[!!]` 文案引导到上述警告。

### 3.5 `theme import` 落盘

- `import <file>`：读文件 → `toml::from_str::<toml::Value>` 校验为表（顶层 `{theme: {...}}` 或散表体两种都接受）→ 读 config.toml 为 `toml::Value` → 插入/替换 `theme` 键 → 序列化写回（保留其他段）。
- 打印导入结果（主题名/键数）。
- 不提供 `--apply`/`--check`（拍板：直接落盘；`--check` 留作未来）。

### 3.6 DESIGN.md 修正

§ 三级配置深度（§290 附近）按正确形态重写：
- 级别 1：`theme = "dracula"`（字符串 → 基底替换）
- 级别 2：`[theme] preset = "dracula"` + `[theme.overrides]`（引用 + 微调）
- 级别 3：`[theme]` 部分/完整表（显式键逐键覆盖基底）
并说明叠加顺序（基底 → config 键 → config overrides → mod overrides）。

### 3.7 验收

- [ ] `theme = "dracula"` 字符串可用（渲染使用 dracula 色）
- [ ] `[theme] accent="#ff0000"` 部分表可用（仅 accent 变化，其余基底）
- [ ] 坏 config：stderr 有 `[claude-hud] warning` + 回退默认可渲染；doctor `[!!]`
- [ ] `[theme.overrides]` 与 `mod.theme.overrides` 都生效，mod 级覆盖 config 级
- [ ] `theme import` 后 config.toml 含 `[theme]` 段，其他段（active_mod 等）保留
- [ ] 单元测试覆盖：untagged 各形态解析、合并顺序、import 保留其他段

## 4. 任务 ⑥ — Mod 系统

### 4.1 六项修复

**① mod use 校验**：`use <name>` 前 `AppConfig::load_mod(name)`，失败 → `Err("mod 'x' not found")`（exit 1），不写 config。@ 开头走场景解析（③）。

**② mod use -（previous_mod）**：`StateFile` 加 `#[serde(default)] pub previous_mod: Option<String>`。use 成功后写 state.json（read-modify-write，`StateFile::update`）：`previous_mod = 旧 active_mod`。`use -`：读 previous_mod，有 → 对调（active_mod ↔ previous_mod，支持往返 toggle），无 → `Err("no previous mod recorded")`。
> render 进程全量写 state.json（compact.rs:33 read → 改 → write）为 read-modify-write，previous_mod 不丢失；dashboard/serve 同理。

**③ mod use @scene**：
```rust
fn scene_alias(alias: &str) -> Option<&'static str> {
    match alias {
        "daily" => Some("daily-dev"),
        "night" => Some("night-coding"),
        "agent" => Some("heavy-agent"),
        "ssh" => Some("ssh-remote"),
        _ => None,
    }
}
```
解析顺序：① 别名表命中 → scene 名；② 直接 load_mod(名字)（@glacier-workstation 等）；③ 遍历内置 6 mod + 用户 mods 目录，`mod_info.scene == scene 名` 首命中。全部失败 → `Err("scene '@x' not found")`。内置 scene 值（有重复 daily-dev ×2，按内置 match 顺序首者 = glacier-workstation）。

**④ mod save 真实快照**：
```rust
ModPackage {
    mod_info: { name, version: "1.0.0", description: "", scene: "" },
    layout: Some(ModLayout {
        compact: 当前 widget 数组匹配 minimal/activity 内置数组 → 对应 ID，否则 "custom"（元数据，渲染由 compact_widgets 驱动）,
        dashboard: config.dashboard.default_layout,
        compact_lines: runtime_overrides.compact_lines.unwrap_or(theme.compact_lines),
    }),
    compact_widgets: Some(config.compact_layout.clone()),   // 新增字段
    theme: Some(ModTheme {
        preset: resolve_theme 的基底 preset 名（无则 "nord"）,
        overrides: 合并后主题与 load_preset(preset) 的 20 字段差异（逐键 toml::Value），无差异则 None,
    }),
    animation: Some(ModAnimation { enabled: true, effects: vec![] }),
    widgets: config.widgets.clone(),
}
```
`mod export my-custom` 内容包含当前 widgets 配置 + compact_widgets（验收）。

**⑤ layout 灌入渲染（compact.rs）**：
- widget 数组解析顺序：`mod.compact_widgets`（Some）→ `mod.layout.compact` 映射（minimal = `["model_display", "context_bar", "cost_display", "git_status"]`；activity = `["model_display", "context_bar", "agent_overview", "git_status", "skills_mcp", "cost_display", "rate_limits"]`）→ `config.compact_layout`。
- 布局 ID 其余值（agent-centric/full/contextual/kpi）且无 compact_widgets → `Err("compact layout 'x' not implemented")`（render 报错路径，走 hud_err_marker）。
- compact_lines 优先级：`runtime_overrides.compact_lines` → `mod.layout.compact_lines` → `theme.compact_lines`（现状是 runtime_overrides → theme，插入 mod 层）。
- active_mod 加载失败（mod 被删）→ 静默回退 config 路径（现状行为）。

**⑥ mod pick**：最简序号选择器：列出内置 + 用户 mods（`1. glacier-workstation [active]` 格式）→ stdin 读一行 → 序号解析 → 走 use 流程（含校验/previous_mod）。非法输入 → `Err` 提示重试说明。不引 TUI 依赖。

**文档修正**：DEPLOY.md 的 `mod save` 描述（真实快照 + compact_widgets）、`mod use -`/`@scene`/`pick` 状态同步更新。

### 4.2 验收

- [ ] `mod use nonexistent` 报错退出（exit 非 0），config.toml 未被写入
- [ ] `mod save my-custom` 后 `mod export my-custom` 含当前 widgets 配置 + compact_widgets
- [ ] `use A` → `use B` → `use -` 回到 A；`use -` 再 `use -` 回到 B
- [ ] `use @daily` 生效（切到 glacier-workstation）；`use @unknown` 报错
- [ ] `mod use obsidian-command`（agent-centric + compact_lines=3 + 无 compact_widgets）→ 渲染报 "not implemented"
- [ ] 自定义数组 mod（save 生成）→ use 后渲染与该数组一致
- [ ] `mod pick` 可通过序号切换
- [ ] 黑盒用例覆盖上述场景（config 备份/恢复防污染）

## 5. 任务 ⑦ — ANSI 上色修复

### 5.1 四处修复（统一模式：要着色的文本整体包进 ansi_fg，含数字）

| widget | 现状（空 wrap） | 修复后 |
|--------|----------------|--------|
| rate_limits.rs:23-25 | `5h:{empty} {fh:.0}%{reset}` | `5h:{ansi_fg("{fh:.0}%", fc)} 7d:{ansi_fg("{sd:.0}%", sc)}` |
| session_stats.rs:43-50 | 符号空 wrap、数字在外 | `{ansi_fg("⏱{dur}", muted)} {ansi_fg("{n}·", accent)}{ansi_fg("{tps}tok/s", fg)}` 等，数字+单位整体包 |
| token_attribution.rs:36-37 | `top:` 空 wrap、pct 在外 | `{ansi_fg("top:{name} {pct:.0}%", accent)}`（整体） |
| cost_display.rs:23-28 | 符号在色内、数字在外 | `{ansi_fg("{prefix}{symbol}{cost:.2}", color)}`（整体） |

### 5.2 黑盒断言扩展（assertions.py）

新增 spec 键 `stdout_raw_regex`：在**原始** stdout（含 ANSI）上 `re.search`（与现有 stdout_regex 的剥离后文本区分）。用例断言色内文本非空：
```
# rate_limits 超阈值数字为红（danger 色码后紧跟非空文本）
stdout_raw_regex: r"\x1b\[38;2;\d+;\d+;\d+m[^\x1b]+"
# 并叠加 stdout_contains（剥离后）验证数字文本出现
```

新用例（D 系列追加）：P3-0x —— rate_limits 超 90% 红、session_stats 三色（fg/accent/muted 色码各至少一次且色内非空）、token_attribution 色内含 "top:"、cost_display 色内含 "$"。

### 5.3 验收

- [ ] rate_limits 超阈值时数字在色内（原始 stdout 正则验证）
- [ ] session_stats / token_attribution / cost_display 三色生效（色内文本非空）
- [ ] 新断言用例通过；既有 106 例全量回归（任务 3 的 ANSI 剥离逻辑不受影响——剥离只作用于 stdout_contains 等旧键）

## 6. 测试策略

- **单元**：theme.rs（ThemeRef 各形态解析 + 11 色 default）、config.rs（ThemeRef 字段、partial 解析）、main.rs/新模块（合并顺序、scene 别名、layout 映射）、compact.rs（compact_lines 优先级）。
- **黑盒**：P3-0x 系列约 10 例（theme 字符串/部分表/overrides 叠加、坏 config 警告 + doctor [!!]、import 落盘保留段、mod use 校验/previous_mod/@scene/save/pick、ANSI 结构）。计数从 106 增至 ~116。
- 全量：`cargo test` + 黑盒 + doctor 三件套（沿用 Phase 2 收尾流程）。

## 7. 文档更新清单

| 文件 | 改动 |
|------|------|
| DESIGN.md | 三级配置深度正确形态 + 叠加顺序（§3.6） |
| DEPLOY.md | mod save 描述、use -/@scene/pick 状态 |
| COMPLETE.md | §20 🟡 表移除「Mod overrides / mod pick / theme import / 动画占位」相关行，✅ 段追加 Phase 3 要点；§21 roadmap 加行 |
| CHANGELOG.md | 追加 Phase 3 条目 |
