# Claude HUD 黑盒测试方案设计

日期：2026-07-31
状态：已获用户认可（2026-07-31）

## 1. 背景与目标

claude-hud 是 Claude Code 的终端 HUD（statusline + TUI 仪表盘）。2026-07-31 修复了
statusLine stdin JSON 中 `used_percentage`/`current_usage` 为 `null` 时解析失败、
状态栏空白的 bug。

本测试方案的目标：**在不修改任何源码的前提下**，对 claude-hud 的全部命令建立
黑盒测试套件，覆盖：

- 所有命令（render / dashboard / serve / setup / mod / theme / widget / completion）
- stdin JSON 的全部形态（含已修复的 null 场景，作为头号回归用例）
- UI 组合（compact 布局排列、行数、分隔符、主题、widget 配置键）
- 配置与 mod 生命周期

每次运行产出 markdown 测试报告。

## 2. 硬性约束

| 约束 | 说明 |
|---|---|
| 不碰源码 | 不改 `src/` 任何文件，不改 `Cargo.toml`，不新增依赖 |
| 配置隔离 | 测试会临时改写 `~/.claude/plugins/claude-hud/`，必须走备份-恢复协议（见 §4） |
| 环境依赖 | skills/mcp 计数由 exe 探测真实文件系统，黑盒无法注入 → 形状断言 |
| dashboard TUI | 依赖真实 TTY，无法自动化 → 非 TTY 失败用例自动化 + 手工清单 |

## 3. 工具链与目录结构

- **harness**：`scripts/test_hud.py`（python3，仅标准库：subprocess / json / tempfile /
  http.client / urllib），驱动 `~/.cargo/bin/claude-hud.exe`
- **fixtures**（入仓，可 review 可演进）：
  - `fixtures/json/*.json` — stdin 样本（D1 用例语料）
  - `fixtures/transcript/*.jsonl` — 假 transcript（D8 用例语料）
  - `fixtures/config/*.toml` — 各用例组的配置模板
  - `fixtures/mods/*.toml` — 测试 mod（D4 生命周期用例）
- **运行期产物**（不入仓）：临时工作目录、`reports/test-report-YYYYMMDD.md`

## 4. 隔离协议（备份-恢复）

1. 套件启动时备份 `~/.claude/plugins/claude-hud/` 整体到临时目录（含 config.toml、
   mods/ 子目录）
2. 每个用例组执行前写入测试配置；执行后（`finally`）恢复
3. 恢复失败的判定：备份文件与恢复后文件字节不一致 → 报告置红并输出醒目告警，
   绝不静默继续
4. `setup` 用例单独预案：备份 `~/.claude/settings.json`，测试后恢复
5. 测试期间用户不得手动操作 claude-hud 配置（报告头部明确提示）

## 5. 用例矩阵

用例 ID 规则：`D{维度}-{两位序号}`。每例记录：输入、期望、断言方式、复现命令。

### D1 — stdin JSON schema（22 例，回归优先级最高）

D1 开头 3 例为已修复 bug 的回归用例（当时 RED→GREEN 的 fixture）。

| ID | 输入 | 期望 |
|---|---|---|
| D1-01 | 全字段 JSON，`used_percentage: null` | exit 0，输出状态栏，ctx 0% |
| D1-02 | 全字段 JSON，`current_usage: null` | exit 0，输出状态栏，ctx 正常百分比 |
| D1-03 | 全字段 JSON，`used_percentage` + `current_usage` 均 null | exit 0，输出状态栏，ctx 0% |
| D1-04 | 全字段 JSON（数字全有） | exit 0，输出完整状态栏（模型/ctx/成本） |
| D1-05 | 缺可选字段（rate_limits/transcript_path/total_output_tokens） | exit 0，正常渲染 |
| D1-06 | 缺必需字段 `model` | exit 1，stderr 含 error |
| D1-07 | 缺必需字段 `context_window` | exit 1，stderr 含 error |
| D1-08 | 缺必需字段 `cost` | exit 1，stderr 含 error |
| D1-09 | 空对象 `{}` | exit 1，stderr 含 error |
| D1-10 | 空 stdin（0 字节） | exit 1，stderr 含 `parse stdin JSON` |
| D1-11 | 垃圾输入（非 JSON 文本） | exit 1，stderr 含 error |
| D1-12 | 类型错误：`used_percentage` 为字符串 | exit 1，stderr 含 error |
| D1-13 | 类型错误：`total_cost_usd` 为字符串 | exit 1，stderr 含 error |
| D1-14 | 极端值：`used_percentage: -5` | exit 0，bar 0 填充（不 panic） |
| D1-15 | 极端值：`used_percentage: 150` | exit 0，bar 满填充（不 panic） |
| D1-16 | 极端值：超大 token 数 / 超大成本 | exit 0，不 panic，输出合理 |
| D1-17 | Unicode 模型名（中文/emoji） | exit 0，原样输出 |
| D1-18 | 多余字段（workspace/version/session_id/exceeds_200k_tokens/cwd） | exit 0，正常渲染（未知字段忽略） |
| D1-19 | `transcript_path` 指向不存在文件 | exit 0，不 panic，输出正常 |
| D1-20 | `transcript_path` 指向损坏文件 | exit 0，不 panic（解析失败被吞或降级） |
| D1-21 | `rate_limits` 各桶 `used_percentage` 数字/缺失 | exit 0，正常渲染 |
| D1-22 | `subagent_status_line` 带 agent 列表 | exit 0，agent 相关 widget 输出 |

### D2 — 布局组合（UI 组合核心，12 例）

| ID | 输入 | 期望 |
|---|---|---|
| D2-01 | 默认 6-widget 布局，2 行 | 两行输出，行 1 = 前 3 widget，行 2 = 后 3 widget，分隔符 ` │ ` |
| D2-02 | 空 `compact_layout` | exit 0，空输出 |
| D2-03 | 单 widget 布局 | 一行输出 |
| D2-04 | 含未知 widget id | 未知 id 被跳过，其余正常 |
| D2-05 | 全部 13 个 widget | 全部渲染，2 行（7+6 取整） |
| D2-06 | 布局顺序重排（cost 在前） | 输出顺序与布局一致 |
| D2-07 | `compact_lines: 1` | 全部 widget 一行，分隔符拼接 |
| D2-08 | `compact_lines: 3`（6 widget） | 3 行，每行 2 个 |
| D2-09 | 奇数个 widget（5 个）+ 2 行 | 行 1 = 3 个，行 2 = 2 个（向上取整） |
| D2-10 | 分隔符变体（`|`、空串） | 按配置拼接 |
| D2-11 | 某 widget 输出为空（如无 agent 数据） | 该位被过滤，不产生空段 |
| D2-12 | 所有 widget 均空输出 | 空输出，exit 0 |

### D3 — widget 配置键（10 例）

| ID | 覆盖点 | 期望 |
|---|---|---|
| D3-01 | context_bar `bar_width: 5` | bar 宽度 5 |
| D3-02 | context_bar `warn_threshold` / `critical_threshold` | 阈值生效，颜色切换（ANSI 码变化） |
| D3-03 | context_bar 缺省配置 | 用 theme.bar_width 默认 |
| D3-04 | cost_display `currency_symbol: "$"` | 符号变为 $ |
| D3-05 | cost_display `warn_threshold_usd: 0.01` | 高成本变色 |
| D3-06 | icon_set: ascii（config 主题） | `[` `]`、`[SK]`、`[MC]` 样式 |
| D3-07 | icon_set: minimal | `◇` `◆` 样式 |
| D3-08 | icon_set: nerd（默认） | `▸` `🧩` `🔌` 样式 |
| D3-09 | widgets 表中非表值 / 未知 widget 配置 | 忽略，不崩溃 |
| D3-10 | 配置非法（坏 TOML） | exe 回退默认配置或报错退出，不 panic |

### D4 — 主题与 mod 生命周期（11 例）

| ID | 覆盖点 | 期望 |
|---|---|---|
| D4-01 | 默认主题（无 config） | 输出默认配色 ANSI |
| D4-02 | 6 个内置 preset 逐一 `mod use` 切换 | 每次切换后 render 正常、ANSI 颜色有差异 |
| D4-03 | 主题颜色覆盖（theme 覆盖表） | 覆盖色生效 |
| D4-04 | bar 字符覆盖（bar_filled/bar_empty） | 字符生效 |
| D4-05 | `theme export` | exit 0，输出合法 TOML |
| D4-06 | `theme import` 合法文件 | exit 0 |
| D4-07 | `theme import` 非法文件 | exit 1，stderr 含 error |
| D4-08 | `mod save` → `mod list` → `mod current` | 生命周期状态一致 |
| D4-09 | `mod use` 不存在 mod | exit 1，stderr 含 error |
| D4-10 | `mod export` / `mod import` 往返 | 导出再导入内容一致 |
| D4-11 | `mod delete` → 再 `mod use` 该 mod | 已删除，报错 |

### D5 — CLI 子命令（15 例）

| ID | 覆盖点 | 期望 |
|---|---|---|
| D5-01 | `--help` | exit 0，输出用法 |
| D5-02 | 无参数 | exit 2（clap 报缺子命令） |
| D5-03 | 未知子命令 | exit 2 |
| D5-04 | `widget list` | exit 0，列出 13 个内置 widget |
| D5-05 | `widget test model_display` | exit 0，输出 widget 渲染 |
| D5-06 | `widget test nonexistent` | exit 0，提示 not found |
| D5-07 | `completion bash` / `zsh` / `fish` | exit 0 |
| D5-08 | `completion powershell`（不支持） | exit 0，提示 unsupported |
| D5-09 | `mod list` | exit 0，列出 6 内置 preset |
| D5-10 | `mod preview` 合法 mod | exit 0 |
| D5-11 | `mod preview` 不存在 mod | exit 1，stderr 含 error |
| D5-12 | `mod current` | exit 0 |
| D5-13 | `mod reset` | exit 0，config 恢复默认 |
| D5-14 | `mod import` 不存在的文件 | exit 1，stderr 含 error |
| D5-15 | `setup`（settings.json 已存在） | exit 0，打印手动添加提示，不覆盖 settings.json |

### D6 — serve（6 例）

| ID | 覆盖点 | 期望 |
|---|---|---|
| D6-01 | `GET /` | 200，Content-Type text/html |
| D6-02 | `GET /api/data` | 200，Content-Type application/json，JSON 可解析 |
| D6-03 | `GET /api/health` | 200 |
| D6-04 | 未知路由 `/nope` | 404 |
| D6-05 | 服务启动后端口可连、响应在超时内 | 5s 超时 |
| D6-06 | 进程退出后端口释放 | 退出码 0 |

### D7 — dashboard（1 例自动化 + 手工清单）

| ID | 覆盖点 | 期望 |
|---|---|---|
| D7-01 | 非 TTY 环境运行 `dashboard` | exit 1，stderr 含错误信息（不 panic 不挂死，10s 超时） |

手工清单（真实终端人工执行，报告模板附步骤）：
1. 在终端运行 `claude-hud dashboard`，进入全屏 TUI
2. 确认 2x2 网格渲染、各 widget 显示数据
3. `q` 退出，终端恢复，无残留
4. 缩放终端窗口，布局不崩

### D8 — transcript 解析（6 例）

| ID | 覆盖点 | 期望 |
|---|---|---|
| D8-01 | 合法 JSONL transcript（含 tool_use/tool_result/assistant/user 条目） | exit 0，输出正常 |
| D8-02 | transcript 含 agent 数据 | agent 相关 widget（overview/detail）输出变化 |
| D8-03 | transcript 含 skill/mcp 调用 | skills 相关 widget 输出变化 |
| D8-04 | 空文件 transcript | exit 0，不 panic |
| D8-05 | 单行损坏 JSON | exit 0，不 panic（该行跳过或降级） |
| D8-06 | 大文件（~1MB）transcript | 超时内完成（10s） |

## 6. 断言策略

1. **精确断言**：环境无关部分（模型名、百分比、币种符号、行数、分隔符、exit code）
2. **正则断言**：环境相关部分（skills/mcp 计数 `🧩\s+\d+`、`🔌\s+\d+`）
3. **退出码契约**：成功用例 exit 0 且 stderr 为空；失败用例 exit 1（clap 为 2）且 stderr
   含预期错误片段
4. **超时契约**：render 用例 10s；serve/dashboard 常驻用例 5s；超时记为 FAIL
5. **每例记录复现命令**（exe + stdin 文件路径 + 配置摘要），失败可一键复现

## 7. 报告格式

`reports/test-report-YYYYMMDD.md`：

- 头部：环境快照（exe 路径与 mtime、python 版本、运行时间、配置备份状态）
- 汇总：总用例数 / 通过 / 失败 / 通过率 / 耗时
- 用例明细表：ID、维度、输入摘要、期望、实际、结果
- 失败明细：每例附实际输出（截断至 500 字节）与复现命令
- 手工清单模板（dashboard TTY）

## 8. 验收标准

1. `python scripts/test_hud.py` 一键运行，全部用例执行且报告生成
2. D1-01~03（null 回归）必须 PASS——这是本套件存在的首要理由
3. 全程不修改 `src/` 与 `Cargo.toml`；运行结束后真实配置与运行前一致
4. 报告可在无 Claude Code 环境复现（仅需 python3 + exe）
5. 套件可在任意失败用例后单独重跑（幂等，支持 `--case D1-05` 单跑）

## 9. 已知边界

- skills/mcp 计数、MCP 探测依赖真实环境 → 只断言形状，不断言具体值
- dashboard TUI 无法黑盒自动化 → 非 TTY 失败用例 + 手工清单
- `setup` 用例触碰 `~/.claude/settings.json` → 备份-恢复预案
- 测试会短暂改写真实 `config.toml` → 测试期间勿手动操作配置
