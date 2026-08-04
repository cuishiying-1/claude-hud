# 第一期设计：state.json 数据通路与增量状态

> 日期：2026-07-31
> 来源：TASKS.md 三期拷打决策（任务 ① ②⑧ ⑫ ⑬）+ brainstorming 会话（分期 4 期，第一期 = 基础设施）。
> 本设计为 4 期分期方案的第一期，完成后进入第二期（③ ④ ⑭ 数据契约）。

## 1. 范围

第一期包含四个任务：

| 任务 | 主题 | 关键产物 |
|------|------|---------|
| ① | dashboard/serve 数据通路卡死（TTY 阻塞） | state.json 快照 + IsTerminal 分发 |
| ②⑧ | 增量读取失效 + 状态语义错乱 | TranscriptReader 累计状态 + 快照恢复 + git TTL + 脚本节流 + 时间相位动画 |
| ⑫ | 通知防轰炸 | `[alerts]` 配置 + 跨进程冷却（state.json 去重） |
| ⑬ | 状态栏静默失效 | `[hud err]` stdout 标记 + last_error 落盘 + doctor 检查项 |

第一期验收形态：内部验收（cargo test + 黑盒套件 + doctor 全绿），不发布、不打 tag。

## 2. 架构总览

`state.json`（`~/.claude/plugins/claude-hud/state.json`）是 **render 进程与长驻进程之间的唯一共享层**。

```
render（5s 瞬态）      dashboard（长驻）        serve（长驻）        doctor（按需）
  stdin JSON            IsTerminal 分发          每请求读 state     检查项：state.json
  TranscriptReader      state.json（新鲜）        → JSON 响应        + last render failure
  （从 state 恢复）       transcript tail（内存累计）  + alerts 冷却
  渲染 → stdout          渲染 TUI                    （只读）
  持久化：快照+transcript+缓存+alerts  ←──────────── 共享 state.json ──────────→
  失败：last_error+标记              （快照/transcript/缓存写入方只有 render；
                                      alerts 段 render 为跨进程权威）
```

### 2.1 state.json 形态（单文件全功能，拍板方案 1）

```json
{
  "snapshot": {
    "timestamp_secs": 1789000000,
    "model": { ... },
    "context_window": { ... },
    "cost": { ... },
    "rate_limits": { ... },
    "subagent_status_line": { ... },
    "transcript_path": "..."
  },
  "transcript": {
    "path": "...",
    "last_pos": 4096,
    "agents": [...],
    "skill_calls": [...],
    "mcp_calls": [...],
    "tool_counts": { "Read": 12 },
    "total_tokens": { ... },
    "token_timeline": [...]
  },
  "cache": {
    "git": {
      "branch": { "value": "master", "ts": 1789000000 },
      "dirty": { "value": false, "ts": 1789000000 },
      "ahead_behind": { "value": "0/0", "ts": 1789000000 }
    },
    "script_throttle": { "<widget_id>": 1789000000 }
  },
  "alerts": {
    "context_critical": 1789000000,
    "cost_threshold": 1789000000,
    "rate_limit": 1789000000
  },
  "last_error": { "ts_iso": "2026-07-31T12:00:00+08:00", "msg": "parse stdin JSON: ..." }
}
```

- 全字段 `#[serde(default)]`：缺失/损坏一律读到默认值，绝不硬失败
- 每次 render 全量重写（几 KB，5s 一次，`write_atomic` 原子写）
- 单文件单读：dashboard/serve 一次读全

## 3. 模块布局

| 模块 | 变更 |
|------|------|
| `src/core/state.rs`（新增） | `StateFile` 五段结构；`read()`（缺失/损坏→默认）、`write()`（复用 `write_atomic`）、`update(fn)` 读-改-写；快照过期判定（30s 常量） |
| `src/core/transcript.rs` | `TranscriptReader` 重构：累计状态从局部变量提升为 self 字段；`from_snapshot()` / `to_state()`；path 变化或文件截断（last_pos > 文件大小）→ 重置；`read_updates()` 返回**累计** summary（widget 替换语义保持正确，widget 不改） |
| `src/alert.rs`（新增） | `AlertCooldown`（`HashMap<AlertKind, u64>` 各类型 last-fired）+ `check_alerts(data, cfg, &mut AlertCooldown, now)` —— render 与 dashboard 共享同一函数；冷却判定为纯函数（epoch 注入，可测试）；**render 的 cooldown 由 state.alerts 加载/回写，dashboard 的 cooldown 启动时从 state.alerts 播种后仅存内存** |
| `src/core/config.rs` | `state_path()` + `[alerts]` 段（`context_critical_pct=95.0` / `cost_threshold_usd=10.0` / `rate_limit_pct=90.0` / `cooldown_minutes=10`，0=关闭） |
| `src/probe/git.rs` | 探测前查缓存 TTL：branch/dirty 30s、ahead/behind 60s；命中复用不 spawn |
| `src/widgets/script_widget.rs` | 节流 `last_run` 从 state.cache 读，`refresh_seconds` 跨进程生效 |
| `src/widgets/alerts.rs` | 呼吸动画改时间相位驱动：`SystemTime now % 8`，紧凑模式动画跨 5s 快照复活 |
| `src/compact.rs` | render 流程整合：读 stdin → 恢复 reader → 渲染 → 持久化 |
| `src/dashboard.rs` / `serve.rs` | `read_current_data()` 重写：IsTerminal 分发 + state 读取 + 过期占位 |
| `src/main.rs` | render 错误路径：stdout 标记 + last_error 落盘 |
| `src/doctor.rs` | 检查项：state.json 有效且新鲜、last render failure |
| `Cargo.toml` | + `chrono = "0.4"`（⑬ ISO8601；任务④ 复用） |

## 4. 数据流

### 4.1 render 生命周期（5s 管线，顺序执行）

```
① 读 stdin → 解析 SessionData
      │ 失败 ──► stdout: [hud err] ≤80字符 ─► stderr: 完整错误 ─► 写 last_error ─► exit 1
② 读 state.json（缺失/损坏 → 默认值，不中断）
③ TranscriptReader：state.transcript.path == 本次 path？恢复 : 全新
      │ transcript 缺失 → 静默（正常瞬态）
④ read_updates() → 累计 summary → push 给 widgets
⑤ git 探测：查缓存 TTL → 命中复用 / 未命中 spawn 并回写
⑥ 脚本 widget：state 节流 last_run 生效
⑦ check_alerts(data, cfg, &mut cooldown, now)：cooldown 自 state.alerts 加载；越阈 + 超冷却 → 发通知 + 标记 fired
⑧ 渲染 → stdout
⑨ 持久化：snapshot + transcript(to_state) + cache + alerts → write_atomic 一次写
```

### 4.2 dashboard / serve 数据流

```
IsTerminal(stdin) ?
   ├─ 非 TTY（echo ... | claude-hud dashboard）→ 读 stdin（向后兼容，行为不变）
   └─ TTY（正常启动）→ 读 state.json
        ├─ snapshot 新鲜（≤30s）→ 还原 SessionData + transcript_path（修复 dashboard.rs:63-66）
        ├─ 过期/缺失 → "无活跃会话" 占位，事件循环照常（q 可退出）
        └─ dashboard 用 snapshot 初始化自己的 TranscriptReader（内存累计，不写回）
        └─ dashboard 的 alerts：启动时从 state.alerts 播种 AlertCooldown，检查仅在内存去重（不写 state）
serve：每个请求（2s 轮询）读一次 state；无数据 → 占位 JSON
```

## 5. 并发与竞态规则

- **快照/transcript/缓存段：写入方只有 render**——dashboard/serve 只读这些段，避免 dashboard 用旧快照覆盖 render 的新快照（单文件全量写时此约束是硬性要求）
- **alerts 段：render 是跨进程权威**——render 将 `AlertCooldown` 加载/回写 state.alerts（5s 内完成标记）；**dashboard 启动时播种 cooldown 后仅存内存**，不做 state 写（防旧快照覆盖 + 保"写入方只有 render"规则干净）
- **transcript 段只由 render 写入**：dashboard 长驻、内存累计即可，不持久化——避免两个 reader 共享偏移导致重复计数
- **双窗口隔离**：两个 render 进程各自校验 `transcript.path == 自己的 path` 才恢复，否则重置——会话状态天然隔离，无双计数
- **双窗口同刻越阈**：微秒级竞态窗口，极罕见；文档记录为已知限制（10 分钟冷却下实际不发生），不引入文件锁
- **dashboard 重启边界**：dashboard 触发通知后 5s 内重启（state 未及写入），可能重复触发一次——已知可接受边界，不为此引入额外写
- **通知发送失败**：stderr 警告，但 fired 已标记（防重试轰炸）

## 6. 错误处理矩阵

| 场景 | 行为 | 原则 |
|------|------|------|
| render 解析失败 | stdout 标记 + stderr 全文 + last_error + exit 1 | 故障可见（⑬） |
| transcript 缺失 | 静默，P2 组件 `—` | 正常瞬态 |
| state.json 写失败 | render 继续（快照 best-effort），stderr 警告 | 失败可见不中断 |
| state.json 损坏 | read → 默认值；doctor 报告"损坏或缺失" | 绝不硬失败 |
| snapshot 过期（>30s） | 占位数据，不算故障 | 诚实降级 |
| 通知发送失败 | stderr 警告，fired 已标记 | 防抖优先 |

## 7. 测试计划

### 7.1 单元测试

| 模块 | 用例 |
|------|------|
| `state.rs` | 写→读 round-trip；损坏文件→默认值；缺失→默认值；快照过期判定（30s） |
| `transcript.rs` | 跨进程累计：reader A 读→to_state→reader B from_snapshot 续读→计数递增不倒退；path 变化→重置；文件截断→重置 |
| `alert.rs` | 冷却纯函数（epoch 注入）：首越阈触发、冷却内不触发、冷却后重触发；阈值 0=关闭 |
| `probe/git.rs` | TTL 纯逻辑：新鲜复用 / 过期重跑 |
| `script_widget.rs` | 节流跨进程：last_run 在 state、refresh_seconds 生效 |
| `compact.rs` | `[hud err]` 标记截断 ≤80 字符 |

### 7.2 黑盒套件扩展（scripts/test_hud.py）

1. render 成功 → `state.json` 存在、五段结构完整
2. render 坏 JSON → stdout 含 `[hud err]` + 退出码非零 + `last_error` 落盘
3. 同一 transcript 夹具 render 两次 → state 的 offset 前进（python 读文件断言）
4. render `cost=15`（>10 阈值）→ state.alerts.cost_threshold 标记；冷却内再 render → 不变
5. dashboard 无 stdin（`/dev/null` 管道）→ 占位显示 + 可退出（timeout+q 注入法）
6. serve 无 state → 占位 JSON，非挂死
7. 存量用例全绿（回归）

## 8. 实施顺序（第一期内部）

```
① state.rs 骨架 + state_path + IsTerminal 分发（dashboard/serve 不再卡死）
→ ②⑧ TranscriptReader 重构 + 快照恢复 + git TTL + 脚本节流 + 时间相位动画
→ ⑬ [hud err] 标记 + last_error + doctor 检查项
→ ⑫ [alerts] 配置 + check_alerts 共享函数 + 跨进程冷却（依赖 state 就绪，放最后）
→ 测试收尾 + COMPLETE.md 状态更新（第 20 章：① ②⑧ ⑫ ⑬ → ✅）
```

- 每任务一个 commit（`fix: ...` / `feat: ...` 前缀），由用户手动提交
- 每任务完成即跑 `cargo test` + 黑盒套件

## 9. 验收标准（汇总）

- TASKS.md 任务 ① ②⑧ ⑫ ⑬ 验收清单全绿
- 黑盒套件新增 7 项用例全绿 + 存量回归全绿
- `cargo test` 全绿
- 终端直接跑 `claude-hud dashboard` 不卡死、`q` 可退出
- COMPLETE.md 第 20 章状态表更新（① ②⑧ ⑫ ⑬ → ✅）
- 不发布、不打 tag（内部验收）
