# 批次 C 剩余 — 历史消费 / 平台修复 / 占位收尾 / 宽度感知 / 仪表盘交互 / 安装备份 / 升级通路

> 来源：`TASKS.md` 任务 ⑨⑩⑪⑮⑯⑰⑱（2026-07-31 三轮 grill-me 拷打拍板）。本 spec 将拍板决策落实为文件级设计，全部决策沿用 TASKS.md，无新增开放问题。
> 前置：批次 A（①②⑧⑫⑬）、批次 B（③④⑭）、批次 C 前半（⑤⑥⑦ 已交付，77 单元 + 123 黑盒全绿）。

## 范围

| 任务 | 标题 | 主要落点 |
|------|------|----------|
| ⑨ | 历史库消费（数据孤岛） | state.rs / compact.rs / main.rs / serve.rs |
| ⑩ | Shell Widget Windows 分支 + 删死代码 | scripting.rs / probe/system.rs |
| ⑪ | 占位功能与死代码收尾 | main.rs / dashboard.rs / Cargo.toml |
| ⑮ | compact 零宽度感知 | compact.rs / ansi.rs / 3 个 widget / Cargo.toml |
| ⑯ | dashboard 交互（l / ? / 持久化） | dashboard.rs / main.rs |
| ⑰ | 安装占位符 / 时间戳备份 / 全局提示 | install.sh / install.ps1 / cc_config.rs / main.rs |
| ⑱ | 升级通路（update check） | main.rs / doctor.rs / install.sh |

**贯穿原则**（沿用 TASKS.md）：诚实降级（空数据显示 `—`）· 失败可见 · 不留死代码 · 网络不可达静默降级不报错 · 每任务一个 commit。

---

## 任务 ⑨：历史库消费

### 现状证据

- `history.rs` 建表 + 三个查询（weekly_stats / recent_sessions / daily_cost_trend）齐全，但读取路径为零；唯一写入在 dashboard.rs:128（q/Esc 退出时），render 主形态从不写。
- `SnapshotSegment`（state.rs:42-55）未存代理数，无法结账时还原 agent_count。
- `HistoryStore::open()` 失败在 dashboard.rs:72 被 `.ok()` 静默吞掉。

### 设计

**1. `SnapshotSegment` 增 `agent_count` 字段**（state.rs）

```rust
#[serde(default)]
pub agent_count: usize,
```

`from_session` 从 `data.subagent_status_line.as_ref().map(|s| s.agents.len()).unwrap_or(0)` 填充；`to_session` 忽略（注释注明该字段仅结账用）。

**2. render 会话切换结账**（compact.rs `run_pipeline`，在覆写 `state.snapshot` 之前）

```rust
// 纯函数（可单测）
pub fn should_checkout(prev_ts: u64, prev_path: Option<&str>, cur_path: Option<&str>) -> bool {
    prev_ts != 0
        && !prev_path.map(|p| p.is_empty()).unwrap_or(true)
        && prev_path != cur_path
}

// run_pipeline 中：
if should_checkout(state.snapshot.timestamp_secs,
                   state.snapshot.transcript_path.as_deref(),
                   data.transcript_path.as_deref())
{
    if let Ok(h) = HistoryStore::open() {
        let last = state.snapshot.to_session();
        let _ = h.record_session(&last, state.snapshot.agent_count, &config.active_mod);
    }
}
```

- 结账失败不中断渲染（`let _ =` + open 失败 eprintln 警告）。
- 边界（文档写明）：Claude Code 退出时最后一条会话可能不结账（render 不再被调用）；结账数据是上次快照（lines/subagent 字段为 0/None，state.rs 已有注释）。

**3. `claude-hud history` 子命令**（main.rs 新增 `History` 变体，~60 行）

输出三块（空库显示 `—`，不显示 0）：

```
Weekly stats:
  Cost: $1.23 | Sessions: 3 | Tokens: 45000 | Avg duration: 12.3m | Avg agents: 1.5
Recent sessions:
  #3  2026-08-01 12:00:00  $0.80  12m  5 agents  45k tok
  #2  2026-08-01 11:00:00  $0.43   8m  2 agents  12k tok
Daily cost (last 7 days):
  2026-07-28  $0.50
  2026-07-29  $1.20
```

- 空库：`total_sessions == 0` → 各数值位输出 `—`。
- `HistoryStore::open()` 失败 → `Err` 上报（非静默）。
- 费用符号用 `config.currency_symbol`。

**4. serve `/api/data` 追加 `weekly` 字段**（serve.rs `build_api_json`）

```json
"weekly": {"total_cost": 1.23, "total_sessions": 3, "total_tokens": 45000,
           "avg_duration_min": 12.3, "avg_agents_per_session": 1.5}
```

前端 HTML 加一张卡片（本周费用 + 会话数）；`HistoryStore::open()` 失败时 weekly 返回全 0 并标注 `"available": false`，前端显示 `—`。

### 验收标准

- [ ] 连续两次 render（不同 transcript_path，间隔 >0s）后 `history` 输出含 1 条记录；同 path 连续 render 不重复结账
- [ ] `history` 空库输出含 `—` 且 exit 0
- [ ] `/api/data` 含 `weekly` 字段；serve 无历史库时 `available:false`
- [ ] 单元测试：`should_checkout` 四态（ts=0 / prev 空 / 相同 path / 不同 path）
- [ ] 黑盒：P4 组新增用例（见测试策略）

---

## 任务 ⑩：Shell Widget Windows 分支 + 删死代码

### 现状证据

- `scripting.rs:77-91` `run_shell_command` 无条件 `sh -c`，原生 Windows 无 sh。
- `probe/system.rs` 的 `time_now()`/`memory_mb()` 零调用者（已 grep 确认）。

### 设计

**1. 平台分支**（scripting.rs `run_shell_command`）

```rust
#[cfg(windows)]
let output = Command::new("cmd").arg("/C").arg(command).output();
#[cfg(not(windows))]
let output = Command::new("sh").arg("-c").arg(command).output();
```

错误处理保持现有结构不变。

**2. 删除 `src/probe/system.rs` 整个文件**；`probe/mod.rs` 删除 `pub mod system;`。确认无其他引用后 `cargo build` 验证。

**3. DEPLOY.md** Shell 命令 Widget 章节加平台说明：Unix `sh -c` / Windows `cmd /C`；复杂命令建议写成 `.bat`/`.sh` 脚本再调用。

### 验收标准

- [ ] `cargo build` 通过（Windows 分支经 CI windows-x64 构建验证）
- [ ] 全项目无 `time_now`/`memory_mb`/`probe::system` 引用
- [ ] 现有 shell widget 黑盒用例（Unix 路径）不回归

---

## 任务 ⑪：占位功能与死代码收尾

### 现状证据

- `main.rs:679-689` completion 打印"用 clap_complete"的占位文本，照提示执行自我指涉。
- `dashboard.rs:132-134` `'1'..='9'` 空分支。
- `dashboard.rs:54` `last_agent_count` 恒 0 从未更新；`notify::agents_complete`（notify.rs:26-31）与 `notify::agent_stalled`（notify.rs:53-58）写好未接线。

### 设计

**1. completion 真实现**（Cargo.toml 加 `clap_complete = "4"`，main.rs `generate_completion`）

```rust
fn generate_completion(shell: &str) -> Result<(), String> {
    let sh = clap_complete::Shell::from_shell_name(shell)
        .ok_or_else(|| format!("unsupported shell: {}", shell))?;
    clap_complete::generate(sh, &mut Cli::command(), "claude-hud", &mut std::io::stdout());
    Ok(())
}
```

`Commands::Completion` 分支改为 `Err` 上报（main.rs:183-186 已有统一错误路径）。

**2. dashboard 空分支删除**（dashboard.rs:132-134）

**3. 通知接线**（dashboard.rs `run_loop`）

- `let mut last_agent_count: usize = 0;`
- 每 tick 计算活跃代理数：

```rust
let active = data.subagent_status_line.as_ref()
    .map(|s| s.agents.len()).unwrap_or(0);
if let Some(done) = agents_edge(last_agent_count, active) {
    crate::notify::agents_complete(done);
}
last_agent_count = active;
```

`agents_edge(prev, cur) -> Option<usize>` 纯函数：`prev > 0 && cur == 0` → `Some(prev)`，否则 `None`（放 dashboard.rs，可单测）。

- 卡顿通知（摘要数据可用时）：

```rust
if let Some(ref s) = summary {
    let threshold = config.widget_config("agent_overview")
        .get_u64("stall_threshold_sec", 30);
    let stalled = s.stalled_agents(threshold, state::now_secs());
    if stalled.is_empty() {
        notified_stalled.clear();
    } else {
        for agent in stalled {
            if notified_stalled.insert(agent.name.clone()) {
                crate::notify::agent_stalled(&agent.name,
                    agent.last_tool_call_secs.map(|t| now.saturating_sub(t)).unwrap_or(0));
            }
        }
    }
}
```

`notified_stalled: HashSet<String>` 进程内去重（同一代理只通知一次，代理不再卡顿即清除）。

### 验收标准

- [ ] `completion bash/zsh/fish` 输出真实补全脚本（含 `_claude_hud` 或 `complete -F`）；`completion nope` 报错 exit 1
- [ ] dashboard `1-9` 无副作用（分支已删）
- [ ] 单元测试：`agents_edge` 三态
- [ ] 代理结束/卡顿通知接线代码存在且可编译（运行时行为依赖真实 Transcript，黑盒不覆盖）

---

## 任务 ⑮：compact 零宽度感知

### 现状证据

- `compact.rs:180-208` 定宽渲染无截断；`COLUMNS` 是 statusLine 环境唯一可用宽度源（Claude Code 约束）。
- `ansi.rs` 有 `truncate`（按字符数 + "..."），无 ANSI 剥离函数。

### 设计

**1. 依赖**：Cargo.toml 加 `unicode-width = "0.2"`。

**2. ansi.rs 新增 `strip_ansi`**（扫描剔除 `\x1b[...m` 序列，纯手工状态机，不引正则依赖）：

```rust
/// Strip ANSI SGR escape sequences (e.g. \x1b[38;2;r;g;bm ... \x1b[0m).
pub fn strip_ansi(s: &str) -> String
```

**3. 宽度源**（compact.rs 纯函数）：

```rust
pub fn columns_env() -> u16 {
    std::env::var("COLUMNS").ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(80)
        .max(40)
}
```

**4. 组级截断**（compact.rs 纯函数，`render_with_data` 每行 join 后应用）：

```rust
/// 从行尾整组丢弃直至可见宽度 ≤ max_width（剥 ANSI 后测宽）。
pub fn fit_line(line: &str, sep: &str, max_width: usize) -> String {
    let groups: Vec<&str> = line.split(sep).collect();
    // 从尾部丢弃组，直到剥 ANSI 后 unicode 宽度 ≤ max_width；至少保留 1 组
}
```

宽度测量：`ansi::strip_ansi(line)` 后 `unicode_width::UnicodeWidthStr::width(s)`。

**5. 字段级截断**（`ansi::truncate(s, 24)`，现有实现，24 字符 + `...`）：

- model_display.rs:17 `data.model.display_name`
- git_status.rs:29 `s.branch`
- agent_detail.rs 紧凑输出中的 agent 名

### 验收标准

- [ ] 单元测试：`strip_ansi`（含嵌套/相邻序列）、`columns_env`（缺失/非法/正常/小值 clamp）、`fit_line`（超宽丢尾部组、恰好容纳、单组超宽保留、ANSI 不计宽、CJK 宽度正确）
- [ ] 黑盒：`env_extra={"COLUMNS": "30"}` 渲染输出可见宽度 ≤ 40（clamp 后）；`COLUMNS=200` 与无 COLUMNS 输出一致
- [ ] 正常宽终端（≥120 列）行为与现状一致（黑盒既有用例不回归）

---

## 任务 ⑯：dashboard 交互

### 现状证据

- `dashboard.rs` 只有 q/Esc 退出 + `'1'..='9'` 空分支；布局启动时从 config 定死。
- 布局实现三套：`build_grid_2x2` / `build_sidebar` / `build_single_panel`（`tabbed`/`focus` 均映射单面板）。

### 设计

**1. 布局状态**（dashboard.rs `run_loop`）：

```rust
let mut layout_name = config.dashboard.default_layout.clone();
let mut show_help = false;
```

`draw_dashboard` 增参数 `layout_name: &str` 与 `show_help: bool`；布局匹配改为：

```rust
match layout_name.as_str() {
    "sidebar" => build_sidebar(area),
    "focus" => build_single_panel(area),   // tabbed 视为单面板，但 l 循环不含 tabbed
    _ => build_grid_2x2(area),
}
```

**2. 按键**：

```rust
KeyCode::Char('l') => {
    layout_name = next_layout(&layout_name);
    persist_layout(config, &layout_name);          // best-effort
    println!?  // 不 print —— TUI 内改画底部提示
}
KeyCode::Char('?') => show_help = !show_help;
```

纯函数 `next_layout(cur: &str) -> String`：`grid-2x2` → `sidebar` → `focus` → `grid-2x2`；未知值 → `grid-2x2`。

**3. 布局持久化**（dashboard.rs 辅助函数）：

```rust
/// 读-改-写 config.toml 的 dashboard.default_layout；失败 eprintln 警告不中断。
/// TOML 往返会丢失注释（拍板取舍，doctor 与文档提示）。
fn persist_layout(config: &AppConfig, layout: &str)
```

实现复用 theme import 的 toml::Value 编辑模式（main.rs:638-649），`fs::write` 落盘。

**4. 底部提示 + 帮助视图**：

- 每帧底部渲染一条 footer：`Layout: <name> · Mod: <active_mod> · l=cycle ?=help q=quit`（满足"切换时底部提示当前布局名"且常驻可发现）。
- `show_help` 时在底部上方区域渲染帮助面板：全部按键 + 当前 mod 名 + `(applies to all windows)` 说明（布局切换写回 config，属全局生效）。

### 验收标准

- [ ] `l` 循环三布局，footer 显示当前布局名
- [ ] 切换后 config.toml `dashboard.default_layout` 更新；重启 dashboard 沿用
- [ ] `?` 显示帮助，再按隐藏
- [ ] 单元测试：`next_layout` 四态
- [ ] 黑盒：dashboard 交互无法黑盒（TTY），通过 `config_file_contains` 验证持久化函数的落盘（若可单独触发）或单元覆盖；dashboard 既有用例（timeout 注入 q 退出）不回归

---

## 任务 ⑰：安装占位符 / 时间戳备份 / 全局提示

### 现状证据

- `install.sh:5` / `install.ps1` REPO 默认占位符 `user/claude-hud`，无检测。
- `main.rs:228-233` setup 无条件把原 settings.json 写 `settings.json.bak`（固定名，每次覆盖旧备份）。
- `run_uninstall`（main.rs:251-278）不提示备份位置；config 目录删除不影响 `~/.claude/settings.json.hud.bak-*`（备份在 ~/.claude 下，天然保留）。
- 写配置命令（mod use 等）无全局生效提示。

### 设计

**1. 安装脚本占位符检测**（install.sh / install.ps1，仅网络路径内）：

```bash
# 网络分支内、curl 之前：
if [ "$REPO" = "user/claude-hud" ]; then
  echo "error: Claude HUD 尚未发布，请使用源码构建（cargo build --release）" >&2
  exit 1
fi
```

`HUD_LOCAL_BIN` 本地开发路径不受影响。README 安装段加 not-yet-released 标注（真实仓库创建后移除）。

**2. cc_config.rs 新增 `has_status_line`**：

```rust
/// True when the settings JSON contains a statusLine key (any shape).
pub fn has_status_line(existing: &str) -> bool
```

（解析失败返回 false。单测覆盖：有/无/非法 JSON。）

**3. setup_cc_settings 备份策略变更**（main.rs:217-249 重写）：

- 文件为空/不存在 → 无备份。
- 合法 JSON 且 `has_status_line` → 写 `settings.json.hud.bak-<epoch>`（SystemTime epoch 秒，与 state.rs `now_secs` 同源），打印 `replacing existing statusLine (backup at <path>)`。
- 合法 JSON 无 statusLine → 无备份（不产生文件）。
- 非法 JSON → 原样备份到 `settings.json.hud.bak-<epoch>` + 现有警告文案（"original saved to ...; rebuilding with minimal config"），继续重建。
- 替换现有固定名 `json.bak` 逻辑；`.hud.bak-*` 从不在 setup/uninstall 中删除。

**4. uninstall 提示**（main.rs `run_uninstall` 结尾追加）：

```
Your original settings backup (if any) is at ~/.claude/settings.json.hud.bak-* — copy it back over ~/.claude/settings.json to restore.
```

**5. 全局生效提示**：`mod use` / `mod reset` / `mod save` / `mod delete` / `mod import` / `theme import` / ⑯ 布局切换 的输出追加 `(applies to all windows)`。

**6. DEPLOY.md** 配置章节开头声明："配置全局生效于所有会话窗口；数据层面（session/git）各窗口独立"。

### 验收标准

- [ ] 占位符 REPO：install.sh 报"尚未发布"exit 1；`HUD_REPO=真实仓库` 走正常路径；`HUD_LOCAL_BIN` 本地安装不受影响（本地开发回归）
- [ ] 单元测试：`has_status_line` 三态
- [ ] 黑盒：setup 输出含 `replacing existing statusLine (backup at`（真实环境 statusLine 已存在时）；mod use / theme import 输出含 `(applies to all windows)`；uninstall 不跑（会删真实配置目录，靠单元+审查）
- [ ] 连续两次 setup 产生两个不同时间戳备份（手动验证项，文档写明）

---

## 任务 ⑱：升级通路

### 现状证据

- `install.sh:27-35` 幂等升级逻辑存在（version.txt 对比）但输出不可区分"已最新"与"首次安装"；无 `update` 命令；doctor 无 update 项。

### 设计

**1. `claude-hud update check` 子命令**（main.rs 新增 `Update { Check }` 变体）

常量 `const UPDATE_REPO: &str = "user/claude-hud";`（与 install.sh 同源默认；发布后同步替换）。

```rust
pub enum UpdateStatus { UpToDate(String), Available(String), NotPublished, Unavailable }

pub fn check_update() -> UpdateStatus {
    if UPDATE_REPO == "user/claude-hud" {
        return UpdateStatus::NotPublished;   // 占位符短路，零网络
    }
    let url = format!("https://api.github.com/repos/{}/releases/latest", UPDATE_REPO);
    let resp = ureq::get(&url)
        .set("User-Agent", "claude-hud")
        .timeout(std::time::Duration::from_secs(10))
        .call();
    // 404 → NotPublished；其他网络错误 → Unavailable
    // 成功 → 解析 tag_name，剥 v 前缀 → cmp_versions(CARGO_PKG_VERSION, latest)
}
```

`cmp_versions(a, b)` 纯函数：按 `.` 分段逐段数字比较（a>b → Greater 等），段数不同时缺段视为 0；单测覆盖。

输出（exit 0 恒定）：

| 状态 | 输出 |
|------|------|
| UpToDate | `✓ up to date (vX.Y.Z)` |
| Available | `↗ update available: vX.Y.Z — re-run the install script to upgrade` |
| NotPublished | `not published yet` |
| Unavailable | `update check unavailable` |

**2. doctor 集成**（doctor.rs，信息项不算 failure）：

```
[ok] update: up to date (v0.2.0)          ← UpToDate
[ok] update: update available vX.Y.Z — re-run the install script   ← Available
[..] update: not published yet             ← NotPublished / Unavailable
```

**3. install.sh 输出诚实化**（替换 "already installed — nothing to do"）：

```bash
if [ -f "$INSTALL_DIR/version.txt" ]; then
  OLD="$(cat "$INSTALL_DIR/version.txt")"
  if [ "$OLD" = "$LATEST" ]; then
    echo "claude-hud v${LATEST} is up to date"
  else
    echo "upgrading v${OLD} → v${LATEST}"
  fi
else
  echo "installing claude-hud v${LATEST}"
fi
```

install.ps1 同口径。

**4. README 增加 Upgrade 一节**："重新运行安装命令即升级（自动检测新版本），config.toml 与数据保留在 ~/.claude/plugins/claude-hud/"。DEPLOY.md 发布章节写明版本口径：bump Cargo.toml → tag `vX.Y.Z` → CI 出 artifacts → install 脚本 / update check / CI 三方同一口径。

**明确不做**（TASKS.md 拍板）：`upgrade` 自替换、自动后台下载、render 进程内自动检查。

### 验收标准

- [ ] 占位符阶段：`update check` 输出 `not published yet` exit 0（黑盒离线可测，零网络）
- [ ] 单元测试：`cmp_versions`（相等/高版本/低版本/段数不同/前缀 v）
- [ ] doctor 输出含 `[..] update:` 且 exit 0（黑盒既有 doctor 用例不回归）
- [ ] install.sh 三态输出文案存在（脚本审查，不跑网络）

---

## 测试策略

### 单元测试（新增 ~15 个）

| 模块 | 用例 |
|------|------|
| compact.rs | `should_checkout` 四态；`columns_env` 四态；`fit_line` 五态（超宽/恰好/单组/CJK/ANSI 不计宽） |
| ansi.rs | `strip_ansi`（无码/单段/嵌套/多段） |
| cc_config.rs | `has_status_line` 三态 |
| dashboard.rs | `next_layout` 四态；`agents_edge` 三态 |
| main.rs（或新 update.rs） | `cmp_versions` 五态 |

建议将 update 逻辑抽到 `src/core/update.rs`（main.rs 已 700 行），doctor/CLI 复用。

### 黑盒用例（P4 组，新增 ~8 个，123 → ~131）

| ID | 内容 |
|----|------|
| P4-01 | 两次 render（不同 transcript_path）→ `history` 输出含 `Weekly stats` 与 ≥1 条 Recent session |
| P4-02 | `history` 空库输出含 `—`，exit 0 |
| P4-03 | `update check` 输出 `not published yet`，exit 0（离线） |
| P4-04 | `completion bash` stdout 含 `_claude_hud` 或 `complete -F`；`completion nope` exit 1 |
| P4-05 | `mod use <existing>` 输出含 `(applies to all windows)` |
| P4-06 | `theme import <fixture>` 输出含 `(applies to all windows)`（复用 P3-06 流程） |
| P4-07 | `COLUMNS=30` render：剥离 ANSI 后宽度 ≤ 40 |
| P4-08 | `COLUMNS=200` render 与无 COLUMNS 输出一致 |
| P4-09 | doctor 输出含 `update:` 行且 exit 0（doctor 既有用例同步断言） |

setup 备份行为以单元 + 审查为主（黑盒跑 setup 会在真实 ~/.claude 留下 `.hud.bak-*`，D5-15 已存在 setup 用例，若输出断言不破坏则并入）。

### 文档回写（批次完成后）

- COMPLETE.md §20 ✅/🟡：⑨（history 命令 + weekly + 结账）、⑪（completion 真实现）、⑮（宽度感知）、⑯（l/?/持久化）、⑰（时间戳备份/全局提示）、⑱（update check）状态更新；§21 路线图加批次行。
- DEPLOY.md：history 命令、宽度说明、Shell Widget 平台说明、全局生效声明、发布版本口径。
- CHANGELOG.md 追加 [0.3.0] 段。

## 实施顺序

⑨ → ⑩ → ⑪ → ⑮ → ⑯ → ⑰ → ⑱（每任务一 commit，`feat:`/`fix:` 前缀；⑱ 依赖 ⑰ 的 README/DEPLOY 结构调整顺序不影响实现独立性）。
