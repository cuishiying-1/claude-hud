# 模型能力同步设计（v0.7）—— 真实上下文窗口 + 真实多币种价格

日期：2026-08-05
状态：已与用户逐节确认；价格机制经三轮收敛为"打包内置表 + GitHub 手动同步"

## 1. 背景与问题

用户通过第三方后端（`ANTHROPIC_BASE_URL=https://api.deepseek.com/anthropic`，模型 `deepseek-v4-flash`）使用 Claude Code。现状两处失真：

1. **上下文窗口**：Claude Code 对第三方模型一律按 200k 兜底（能力检测仅对 `api.anthropic.com` 生效，issue #46416）。实际 `deepseek-v4-flash` 窗口为 **1M**（官方文档）。HUD 的进度条百分比、压缩 ETA、压缩告警全部按错误窗口计算 —— 真实占用 ~8.8% 却显示 44%。
2. **价格**：`deepseek-v4-flash` 未收录内置价格表 → 成本走官方 `total_cost_usd` 透传。官方价格：输入 $0.14/M、输出 $0.28/M、cache 命中 $0.0028/M（USD）；国内价 ¥1/M、¥2/M、¥0.02/M（CNY）。
3. **币种**：价格显示只有 `currency_symbol` 一个全局配置（默认 `$`），中文用户也应看人民币（¥），且应使用**真实人民币价格**而非符号替换。

**机制收敛结论**（用户拍板）：不做任何联网探测（DeepSeek `/v1/models` 无元数据，探测对主流官方服务商无效）；价格/窗口数据**全走表** —— 打包内置表 + `model sync` 手动从 GitHub 同步 + 用户手写覆盖。`CLAUDE_CODE_MAX_CONTEXT_TOKENS` 对第三方后端不保证生效（issue #62070），只作用户自己决定的尽力而为补充（§5）。

## 2. 数据模型 —— `[models]` 模型注册表

config.toml 新增注册表段，与 `[pricing]` 并存：

```toml
[models."deepseek-v4-flash"]
context_window = 1000000    # 真实窗口（sync 写入或手动填；HUD 覆盖 stdin 的 200k）
synced_at = "2026-08-05T12:00:00Z"   # 信息性：最近一次 GitHub 同步时间（手动填可留空/删除）

[models."deepseek-v4-flash".price_usd]
input = 0.14e-6
output = 0.28e-6
cache_read = 0.0028e-6
cache_creation = 0.175e-6    # = input × 1.25（沿用既有约定，未发布时估算）

[models."deepseek-v4-flash".price_cny]
input = 1.0e-6
output = 2.0e-6
cache_read = 0.02e-6
cache_creation = 1.25e-6
```

要点：

- 币种价格**各自独立、真实存储**（来源官方发布），不做运行时汇率换算。缺失币种回退 USD 价。
- 新 `ModelEntry` 结构：`context_window: Option<u64>`、`synced_at: Option<String>`、`price_usd: Option<PriceEntry>`、`price_cny: Option<PriceEntry>`（`PriceEntry` 复用现有结构）。
- 兼容：`[pricing]` 段继续可用（USD 语义），**优先级最高**（用户手写覆盖一切）。
- **无探测、无 synced 跳过标识**：`synced_at` 仅作信息来源显示，运行时无分支。

### 内置表（打包随二进制）

`builtin_pricing()` 扩展为 `builtin_models()`：既有 9 个 Claude 模型（价格不变 + 窗口 200k）+ DeepSeek 种子数据：

| 模型 | 窗口 | USD 输入/输出 | CNY 输入/输出 |
|---|---|---|---|
| deepseek-v4-flash | 1M | $0.14/$0.28（cache 命中 $0.0028） | ¥1/¥2（cache 命中 ¥0.02） |
| deepseek-v4-pro | 1M | $0.435/$0.87（cache 命中 $0.003625，2026-05-24 后永久价） | ¥3/¥6（由官方峰值价 ¥6/¥12 = 基准 ×2 推算，发版前以官方中文价目页核对） |

- 内置表 = 离线兜底 + 发版基线，随二进制发布刷新。
- 官方调价 → 更新内置表 → bump 版本发 patch release（现有 `update check` + 安装脚本升级链路）。
- 用户覆盖无需等发版：手写 `[pricing]`/`[models]` 即覆盖内置（§7 doctor 校验负价/窗口 ≤0）。

### 完整优先级链（最终形态）

```
用户 [pricing] → 用户 [models]（含 sync 写入的条目）→ 内置表 → 透传
```

## 3. 读取链 —— 窗口单点解析

消费点共 5 处（context_bar pct+ETA、alerts 压缩告警、compact 压缩通知、serve web pct、dashboard 上下文卡片）。在**管线入口单点解析**，不散改消费点：

```
resolve_context_window(data, config):
  有效窗口 = [models].context_window → 内置注册表窗口 → stdin 原值（无覆盖）
  若有效窗口 ≠ stdin 值：
    data.context_window_size = 有效窗口
    data.used_percentage = (t_in + t_out) / 有效窗口 × 100（钳制 ≤ 100）
  窗口相同 → 信任 stdin pct（不重算）
```

调用点：compact.rs `render`（run_pipeline 前）、dashboard 入口、serve.rs `build_api_json`。下游全部消费点（含告警、ETA）自动用真实窗口。pct 重算理由：Claude Code 按 200k 发 pct，覆盖窗口后必须自算否则进度条自相矛盾；env 生效（Claude Code 重启）后口径一致不冲突。

## 4. 同步流程 —— `model sync` 从 GitHub 拉表

新模块 `src/core/modelsync.rs`（复用 ureq，10s 超时）：

- **仓库数据文件**：`registry/models.toml`（与 `[models]` 同构 + 顶部 `registry_version = "2026-08-05"` 日期），官方调价时提交更新（**提交即生效，不必发版**）。
- **`claude-hud model sync`**：
  1. 拉取 `https://raw.githubusercontent.com/cuishiying-1/claude-hud/master/registry/models.toml`
  2. 解析校验（TOML 合法、窗口 >0、价格 ≥0）—— 失败：报错 + exit 1，**不写任何东西**（不把坏表写进配置）
  3. 成功：合并写入 config.toml `[models]`（远程条目覆盖/补齐本地，带 `synced_at` 时间戳；**不删除**用户手写条目）
  4. 交互询问是否写 env（§5，默认 N）
  5. 输出摘要：`registry_version`、更新了哪些模型、来源提示
- **无自动拉取、无 TTL 缓存**：只有手动 `model sync` 一条通路（用户拍板）。
- 测试钩子：`HUD_REGISTRY_URL` 环境变量可覆盖拉取地址（测试用本地 tiny_http / 本地文件）。

## 5. env 写入 —— 用户自己决定

- `model sync` 写表后交互询问：`当前模型窗口 1000000。是否写入 settings.json env（CLAUDE_CODE_MAX_CONTEXT_TOKENS）？[y/N]`（默认 N）
- 独立命令：`claude-hud model env`（查看现值）/ `model env <window>`（设置）/ `model env off`（清除）
- 写入复用 cc_config merge 机制（带时间戳备份）；文档注明"重启 Claude Code 生效、第三方后端可能无效（尽力而为）"
- **任何自动流程永不碰 env**

## 6. 币种语言感知（¥/$ + 多币种价格选择）

- `currency_symbol` 改为 `Option<String>`（serde default None）；决议点 `AppConfig::currency()`：显式配置 → zh 语言 `¥` → 其他 `$`。约 12 处消费点（pricing 注入 ×2、notify/compact ×3、dashboard、main history ×3、widget 注入）改走 `currency()`。
- **成本计算按语言选币种**：`price_for(lang)` —— zh 优先 `price_cny`（缺则回退 USD 价 + ¥ 符号），en 优先 `price_usd`。选定币种后完整链：**`[pricing]` → `[models]` 该币种价 → 内置表该币种价 → 透传**（手写配置永远压过内置）。
- serve.rs web 仪表盘：JS 硬编码 `'$'` → `/api/data` 下发 `currency_symbol`，JS 用下发值。
- 不做汇率换算：币种价格是真实发布值，按语言选取（见 §2）。

## 7. 错误处理与降级

| 场景 | 行为 |
|---|---|
| sync 拉取失败（网络/超时/404） | 报错 + exit 1，不写任何东西（保留现有配置） |
| sync 解析/校验失败（坏表） | 报错 + exit 1，不写任何东西 |
| 模型无任何表覆盖 | 透传（现状行为，诚实降级） |
| 环境缺失 | sync 拉 GitHub 不需要 base_url/key（公开仓库），不涉及 |
| `[models]` 窗口 ≤ 0 / 价格 < 0 | doctor failure（对齐现有 pricing 负价校验） |
| env 写入失败 | sync 报错但表已写，提示"表已同步，env 写入失败" |

## 8. 测试策略（TDD）

- `resolve_context_window`：单测矩阵（无覆盖 / 窗口不同重算 pct / 相同信任 stdin / 钳制 ≤100 / 窗口 0）
- registry/models.toml 解析：fixture 矩阵（合法双币种 / 缺字段 / 窗口 0 / 负价 / 非法 TOML）
- config 合并链：`[pricing]` → `[models]` → 内置 优先级；sync 合并（覆盖/补齐/不删用户条目）；`synced_at` round-trip
- `price_for(lang)`：zh 有 cny / zh 无 cny 回退 usd / en
- `currency()` 决议：显式 ¥ / zh 默认 ¥ / en 默认 $
- env 写入/清除：复用 cc_config merge 测试模式（含备份）
- 端到端：`HUD_REGISTRY_URL` 指向本地 tiny_http fixture → sync 拉表 → 写 config → env 询问（询问抽成可注入 prompt 函数）

## 9. 命令清单

| 命令 | 说明 |
|---|---|
| `claude-hud model sync` | 从 GitHub 拉最新表 → 写 `[models]` → 询问 env |
| `claude-hud model env [<window>\|off]` | 查看/设置/清除 settings.json env |
| `claude-hud model list` | 列出合并视图（内置 + config，标注来源与币种） |
| doctor | 新增信息项：表来源（内置/GitHub 同步时间）、窗口 ≤0 与负价校验 |

## 10. 已验证的现状结论

- `ANTHROPIC_BASE_URL`/`ANTHROPIC_AUTH_TOKEN` 已在用户环境与 settings.json env 中配置（DeepSeek 官方 Anthropic 兼容口）。
- `https://api.deepseek.com/models`（Bearer 鉴权）返回 `{"object":"list","data":[{"id":"deepseek-v4-flash","owned_by":"deepseek"},{"id":"deepseek-v4-pro","owned_by":"deepseek"}]}` —— 无窗口无价格字段；`/anthropic/v1/models` 404。
- 结论：官方服务商普遍不暴露元数据 → **不做探测，全走表**（§4 机制收敛）。

## 11. 明确不做（YAGNI）

- 联网探测（/v1/models 探测、probe 命令）—— 服务商无元数据，探测无意义
- GitHub 自动拉取 / TTL 缓存 / 本地缓存文件 —— 只留手动 `model sync`
- 运行时汇率换算（币种价格按真实发布值存储选取）
- per-model env 机制（上游 #46416 未实现，无法做）
- 自动写 env（永远用户决定）
