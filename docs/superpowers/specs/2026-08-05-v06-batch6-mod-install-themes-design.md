# Claude HUD v0.6 批次 VI — mod install 插件市场 + 主题预设扩充

> 来源：2026-08-05 brainstorming（批次 VI 为 v0.6 最后一个批次，执行序 III → I → V → II → IV → VI，全部完成后发布）。
> **已拍板（2026-08-05）**：⑰ mod install 仅 GitHub、整目录批量、raw 拉取 mods/、安装后联动 mod use 激活最后一个；⑱ Homebrew tap **砍除**（记录在案）；⑳ 新增 4 主题预设（gruvbox-dark / one-dark / github-dark / palenight，硬编码函数模式）。
> 贯穿原则：诚实降级 · 失败可见（stderr + exit 1）· 不留死代码 · 供应链风险显式警告 · 每批完成 `cargo test` + 黑盒套件 + COMPLETE.md 同步。

---

## 任务 ⑰：`mod install <user/repo>` 插件市场

### 现状（证据）

- 网络模式已有先例：`src/core/update.rs` — `ureq` + `User-Agent: claude-hud` + 10s timeout；404 → `NotPublished`，其他错误 → `Unavailable`；`UPDATE_REPO = "cuishiying-1/claude-hud"`。
- mod 落盘路径已有：`src/core/config.rs` `mods_dir()` = `~/.claude/plugins/claude-hud/mods`；`ModCommands::Import`（main.rs:571-587）已实现 read → `toml::from_str::<ModPackage>` → 写 `mods_dir/<mod_info.name>.toml`。
- mod use 联动已有：`ModCommands::Use` 经 `resolve_mod_target` 校验 → `StateFile.previous_mod` 记录 → `write_active_mod(config, &target)` → 输出 `mod_switched`（main.rs:464-494）。
- `ModPackage`（config.rs:163-177）：`mod_info`（name/version/description/scene）+ `layout` + `compact_widgets` + `animation` + `widgets: HashMap<String, toml::Value>`。
- **供应链风险实证**：`src/widgets/mod.rs:60-75` — widgets 表 `type = "rhai_script"`（script_path）/ `"shell_output"`（command）/ `"http_output"`（url）会注册 `ScriptWidget`，**激活即执行远程代码**。第三方 mod 安装必须显式警告。
- **Import 现状缺校验**：`mod_info.name` 直接拼进路径（main.rs:579），本地文件可接受；远程内容必须加文件名安全校验（path traversal 防护）。

### 方案要点（已拍板）

1. **CLI**：`ModCommands::Install { repo: String }`，用法 `claude-hud mod install <user/repo>`；仅 GitHub。
2. **网络**（复用 update.rs 的 ureq 模式）：
   - 列目录：`GET https://api.github.com/repos/{user}/{repo}/contents/mods`
   - 拉取：`GET https://raw.githubusercontent.com/{user}/{repo}/HEAD/mods/{name}`
   - 列表 404 → 错误 `no mods/ directory found in {repo}`；其他网络错误 → `mod install unavailable`（失败可见：stderr + exit 1）。
3. **两阶段批处理**（先全部拉取+校验，后统一落盘；警告先于任何写入）：
   - **Phase 1 列出**：解析 contents JSON → 仅 `type == "file"` 且以 `.toml` 结尾 → **按文件名升序排序**（确定性，黑盒/单测可断言）。
   - **Phase 2 拉取+校验**：逐文件 fetch → `toml::from_str::<ModPackage>` → 校验 `mod_info.name`。单个失败 → 记 `skipped(reason)`，继续其余（batch 语义）。
   - **Phase 3 供应链警告**：任一通过校验的 mod 的 `widgets` 表含 `type ∈ {rhai_script, shell_output, http_output}` → 打印警告行（i18n），不阻断。
   - **Phase 4 落盘**：写 `mods_dir/<name>.toml`（已存在 → `updated`；新文件 → `installed`）。
   - **Phase 5 报告+激活**：逐 mod 输出结果行 + 汇总行；≥1 成功 → 自动激活**字典序最大的成功 mod**（复用 `write_active_mod` + `StateFile.previous_mod` 路径，输出 `mod_switched` 同款文案，`(applies to all windows)` 后缀惯例一致）；全部失败 → exit 1 + 失败明细。
4. **文件名安全校验**（`mod_info.name`）：非空、≤ 64 字符、仅 `[A-Za-z0-9._-]`、不含 `/`、`\`、`..`；与内置出厂 mod 名冲突 → `skipped("conflicts with built-in mod")`（内置优先，避免用户 mod 永远不生效的困惑）。校验失败的 mod 不落盘。
5. **可测试性**：核心逻辑拆纯函数 + 注入 fetch（单测用 mock，不依赖网络）：
   - `parse_repo_arg(s) -> Result<(String, String), String>` — 恰好一个 `/`、无协议前缀、无空白。
   - `filter_mod_entries(json) -> Vec<String>` — 过滤 + 升序排序。
   - `validate_mod_name(name) -> Result<(), String>` — 安全字符集 + 长度 + 内置冲突。
   - `contains_script_widget(&ModPackage) -> bool` — widgets 表 type 检测。
   - `install_mods(fetch: &impl Fn(&str) -> Result<String, String>, repo: &str, mods_dir: &Path) -> InstallReport` — 报告含 installed/updated/skipped 列表、是否含 script、激活名。
   - CLI 层以真实 ureq fetch 包装。
6. **离线降级黑盒**（确定性、零网络依赖）：仅覆盖网络前即可确定的校验失败路径（坏 repo 格式 → exit 1 + stderr）；网络路径由单测 mock 覆盖，黑盒不碰真实 GitHub（CI 不可依赖仓库内容与网络）。

### 涉及

- `src/main.rs`（`ModCommands::Install` 分支 + clap help）
- 新增 `src/core/mod_install.rs`（纯函数 + 报告结构 + 单测；内部复用 update.rs 的 ureq 调用形状：User-Agent `claude-hud` + 10s timeout，不抽取共享封装）
- `locales/en.toml` + `locales/zh.toml`（新增 keys：`cli.mod_install`、`runtime.mod_install_summary`、`runtime.mod_install_skipped`、`runtime.mod_install_script_warning`、`runtime.mod_install_no_mods_dir`、`runtime.mod_install_no_mods`、`runtime.mod_install_unavailable`、`runtime.mod_install_bad_repo`；激活行复用 `mod_switched`）
- 文档：CHANGELOG（[Unreleased] 批次 VI 条目）、DEPLOY.md（mod install 用法 + 供应链警告）、COMPLETE.md（mod 命令表 + ✅ 段落）

### 验收标准

- [ ] `mod install user/repo` 无网络可用时对坏参数（无斜杠 / 协议前缀 / 含空白）→ stderr 明确错误 + exit 1，不 panic（黑盒 3 例）
- [ ] 单测：`parse_repo_arg` 四态；`filter_mod_entries` 过滤+排序+空表；`validate_mod_name` 六态（合法/斜杠/`..`/超长/空/内置冲突）；`contains_script_widget` 三 type 命中 + 普通 widget 不命中
- [ ] 单测：`install_mods` mock fetch — 全成功（installed/updated 混合）、部分失败跳过、全部失败 exit 1 语义、列表 404、空目录
- [ ] 覆盖语义：重跑同 repo → 同名文件 `updated` 且报告计数正确
- [ ] 供应链警告：含 shell_output 的 mod 触发警告行且安装不阻断；无 script widget 不触发
- [ ] 激活联动：多 mod 安装后激活字典序最大者；输出含 `mod_switched` 文案与 `(applies to all windows)`
- [ ] i18n zh/en 两套 key 全量接入；`cargo test` + 黑盒套件全绿（黑盒 191 + 3 = 194）

---

## 任务 ⑱：Homebrew tap — ❌ 已砍（记录在案）

- **2026-08-05 拍板**：用户了解 Homebrew tap 用途后砍除。
- 原因：自用定位（主要平台 Windows）、release 稳定 ≥ 2 版本的前置、tap 仓库 + formula 的维护成本。
- **防止重复讨论**：后续如需再立项，须先重新评估前置条件与维护者负担。

---

## 任务 ⑳：更多主题预设（4 个）

### 现状（证据）

- `src/core/theme.rs`：`preset_names()` 返回 6 个（dracula/nord/tokyo-night/catppuccin/monochrome/solarized-dark）；`load_preset` 分发到硬编码 fn（`dracula()`/`nord()`/...），每 fn 设 11 个颜色 token（bg/fg/accent/success/warning/danger/muted/border/skill_color/mcp_color/model_color）+ `..Default::default()`（theme.rs:117-212）。
- 主题预设总数 6 在 COMPLETE.md / DESIGN.md 有文案（"6 built-in Theme presets"），需同步 6 → 10。
- 黑盒无主题列表命令面（`theme` 子命令仅 Export/Import），主题变更走单元测试即可，黑盒 0 新增（诚实标注）。

### 方案要点（已拍板）

1. 新增 4 预设，沿用硬编码函数模式（与现有 6 个完全同构）：
   - **gruvbox-dark**（gruvbox dark medium）：bg `#282828` / fg `#ebdbb2` / accent `#fabd2f` / success `#b8bb26` / warning `#d79921` / danger `#fb4934` / muted `#928374` / border `#3c3836` / skill_color `#d3869b` / mcp_color `#8ec07c` / model_color `#83a598`
   - **one-dark**（Atom One Dark）：bg `#282c34` / fg `#abb2bf` / accent `#61afef` / success `#98c379` / warning `#e5c07b` / danger `#e06c75` / muted `#5c6370` / border `#3e4451` / skill_color `#c678dd` / mcp_color `#56b6c2` / model_color `#61afef`
   - **github-dark**：bg `#0d1117` / fg `#c9d1d9` / accent `#58a6ff` / success `#3fb950` / warning `#d29922` / danger `#f85149` / muted `#8b949e` / border `#21262d` / skill_color `#bc8cff` / mcp_color `#39c5cf` / model_color `#58a6ff`
   - **palenight**（Material Palenight）：bg `#292d3e` / fg `#a6accd` / accent `#82aaff` / success `#c3e88d` / warning `#ffcb6b` / danger `#f07178` / muted `#676e95` / border `#32374d` / skill_color `#c792ea` / mcp_color `#89ddff` / model_color `#82aaff`
2. `preset_names()` 6 → 10；`load_preset` 增加 4 个 match 分支。
3. 单元测试：`preset_names` 长度 10 且含 4 新名；4 新预设 `load_preset` 均返回 `Some` 且 11 色齐全、与 `Theme::default()` 不同（证明非占位）；既有 6 预设回归不变。
4. 文档同步：COMPLETE.md / DESIGN.md 中主题预设计数 6 → 10（实施时 grep 全仓库 "6 built-in" / "preset" 计数文案核对）。

### 涉及

- `src/core/theme.rs`（4 个 fn + preset_names + load_preset + 单测）
- 文档：CHANGELOG（[Unreleased] 批次 VI 条目）、COMPLETE.md / DESIGN.md 计数

### 验收标准

- [ ] `preset_names()` 含 4 新名且总数 10
- [ ] 4 新预设 `load_preset` 成功、11 色 token 与 spec 配色一致、非 Default 占位
- [ ] 既有 6 预设回归不变；`cargo test` 全绿（单测 209 → ~227）
- [ ] 黑盒 0 新增（无主题列表命令面，诚实标注）；全量黑盒 194 全绿

---

## 砍除/否决项（本批次追加，记录在案）

- **⑱ Homebrew tap**（2026-08-05）：自用定位 + 前置未满足 + 维护成本，见上文。
- **mod install 多仓库源 / GitCode 支持**（2026-08-05）：仅 GitHub 拍板，GitCode 后续按需加。
- **mod install 交互式选择器**（2026-08-05）：批量安装 + 自动激活最后一个已覆盖自用场景，YAGNI。

## 关联约束

- ⑰ 与 ⑳ 相互独立，可并行实施；共享文档收尾（CHANGELOG / COMPLETE）。
- ⑰ 网络层复用 update.rs 的 ureq 模式；fetch 封装如抽取为共享函数，须保持 update 行为不变（回归单测已有）。
- ⑰ 两阶段批处理保证「警告先于写入」「校验失败零落盘」；写盘失败按报告明细可见。
- 黑盒新增仅限网络前校验路径（确定性）；网络/仓库内容路径全部由单测 mock 覆盖，CI 零网络依赖。

## 后续流程

1. 用户审阅本 spec。
2. 批准后走 writing-plans 生成实施计划（按 brainstorming → spec → plan 标准流程）。
3. 实施完成后批次 VI 全绿（单测 + 黑盒 194）→ v0.6 六批次全部完成 → 走发布流程（bump Cargo.toml 版本 → tag → release.yml 自动构建）。
