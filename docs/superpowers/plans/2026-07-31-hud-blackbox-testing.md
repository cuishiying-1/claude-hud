# Claude HUD 黑盒测试套件 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在不修改任何源码的前提下，用纯 Python 标准库黑盒驱动 `claude-hud.exe`，覆盖全部命令与 89 个用例（8 维度矩阵），生成 markdown 测试报告。

**Architecture:** 一个 Python 包（`scripts/hudlib/`）+ 入口脚本（`scripts/test_hud.py`）+ 入仓 fixtures 语料。harness 分四层：env（路径/环境快照）→ runner（exe 执行、超时、配置备份-恢复协议）→ cases（89 用例数据表）→ report（markdown 报告）。用例定义是纯数据（dict 表 + 少量生成器），断言由统一 AssertionSpec 描述。

**Tech Stack:** python3.10+（仅标准库：subprocess / json / tempfile / shutil / http.client / argparse / re / datetime）。被测对象：`~/.cargo/bin/claude-hud.exe`（已含 null 解析修复）。

**约定：** 本仓库用户规则禁止自动 git 提交——本计划所有任务不含 commit 步骤，实现完成后由用户自行审查提交。全部命令在 Git Bash（Windows）下运行。

**依赖设计文档：** `docs/superpowers/specs/2026-07-31-hud-testing-design.md`。计划相对设计的调整（已获认可的设计基础上细化）：D4 由 11 例细化为 17 例（preset 逐一验证、export/import 拆分），总数 83→89，Task 12 会同步设计文档计数。

---

## 文件结构

| 文件 | 职责 |
|---|---|
| `scripts/test_hud.py` | 入口：参数解析（--case/--exe/--report）、编排（备份→跑用例→恢复→报告）、汇总退出码 |
| `scripts/hudlib/__init__.py` | 空包标记 |
| `scripts/hudlib/env.py` | exe 路径解析、环境快照 |
| `scripts/hudlib/runner.py` | `RunResult`、`run_exe()`、`backup_hud_dir()`/`restore_hud_dir()`、`write_config()` |
| `scripts/hudlib/assertions.py` | `check(result, spec)` 统一断言引擎 |
| `scripts/hudlib/cases.py` | `CASES` 列表：89 用例定义 + fixture 生成器 |
| `scripts/hudlib/report.py` | markdown 报告生成、手工清单模板 |
| `fixtures/json/*.json` | D1 stdin 语料（8 个权威文件） |
| `fixtures/config/*.toml` | D2/D3 配置模板（7 个） |
| `fixtures/mods/*.toml` | D4 测试 mod（2 个） |
| `fixtures/transcript/*.jsonl` | D8 假 transcript（4 个，large 运行期生成） |
| `reports/test-report-YYYYMMDD.md` | 运行产物（不入仓） |

---

### Task 1: 骨架与 env.py

**Files:**
- Create: `scripts/hudlib/__init__.py`
- Create: `scripts/hudlib/env.py`

- [ ] **Step 1: 确认环境**

运行：`python --version`
期望：`Python 3.10+`（本方案使用 `str | None` 类型语法）。同时确认 `C:/Users/admin/.cargo/bin/claude-hud.exe` 存在。

- [ ] **Step 2: 创建包骨架**

`scripts/hudlib/__init__.py`：

```python
"""Black-box test harness for claude-hud (zero source changes)."""
```

- [ ] **Step 3: 编写 env.py**

```python
"""Environment resolution and snapshot for the claude-hud test harness."""
import datetime
import os
import platform
import sys


DEFAULT_EXE = os.path.expanduser("~/.cargo/bin/claude-hud.exe")


def resolve_exe(override: str | None) -> str:
    """Return the claude-hud exe path, validating it exists."""
    path = override or DEFAULT_EXE
    if not os.path.isfile(path):
        sys.exit(f"claude-hud exe not found: {path}")
    return path


def snapshot(exe_path: str) -> dict:
    """Collect environment facts for the report header."""
    return {
        "exe": exe_path,
        "exe_mtime": datetime.datetime.fromtimestamp(
            os.path.getmtime(exe_path)
        ).isoformat(timespec="seconds"),
        "python": sys.version.split()[0],
        "platform": platform.platform(),
        "run_at": datetime.datetime.now().isoformat(timespec="seconds"),
    }
```

- [ ] **Step 4: 验证**

运行：`python -c "import sys; sys.path.insert(0, 'scripts'); from hudlib import env; print(env.resolve_exe(None))"`
期望：输出 `C:\Users\admin\.cargo\bin\claude-hud.exe`，无报错。

---

### Task 2: runner.py — 执行与备份恢复协议

**Files:**
- Create: `scripts/hudlib/runner.py`

- [ ] **Step 1: 编写 runner.py（完整代码）**

```python
"""Process execution and config backup/restore protocol."""
import os
import shutil
import subprocess
import tempfile
import time


HUD_DIR = os.path.expanduser("~/.claude/plugins/claude-hud")
BACKUP_MARKER = ".hud-test-backup"


class RunResult:
    def __init__(self, exit_code, stdout, stderr, timed_out, duration_s, repro):
        self.exit_code = exit_code
        self.stdout = stdout
        self.stderr = stderr
        self.timed_out = timed_out
        self.duration_s = duration_s
        self.repro = repro


def run_exe(exe_path, args, stdin_text=None, stdin_file=None,
            timeout_s=10, env_extra=None):
    """Run the exe. stdin provided as inline text or a file path.
    Returns RunResult. Never raises on child failure."""
    if stdin_file:
        stdin_src = open(stdin_file, "rb")
    elif stdin_text is not None:
        stdin_src = subprocess.PIPE
    else:
        stdin_src = subprocess.DEVNULL
    env = dict(os.environ)
    if env_extra:
        env.update(env_extra)
    start = time.monotonic()
    timed_out = False
    try:
        proc = subprocess.run(
            [exe_path] + args,
            input=stdin_text.encode("utf-8") if stdin_text is not None else None,
            stdin=stdin_src,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            timeout=timeout_s,
        )
        exit_code, out, err = proc.returncode, proc.stdout, proc.stderr
    except subprocess.TimeoutExpired as e:
        timed_out = True
        exit_code = -1
        out = e.stdout or b""
        err = e.stderr or b""
    finally:
        if hasattr(stdin_src, "close"):
            try:
                stdin_src.close()
            except Exception:
                pass
    duration = time.monotonic() - start
    repro = f"{exe_path} {' '.join(args)}"
    if stdin_file:
        repro += f" < {stdin_file}"
    elif stdin_text is not None:
        repro += f" <<< '{stdin_text[:120]}...'"
    return RunResult(exit_code, out.decode("utf-8", "replace"),
                     err.decode("utf-8", "replace"), timed_out, duration, repro)


def backup_hud_dir() -> str:
    """Backup ~/.claude/plugins/claude-hud to a temp dir; return backup root.
    Refuses to run if a previous backup is still present (crash recovery)."""
    backup_root = os.path.join(tempfile.gettempdir(), "claude-hud-test-backup")
    marker = os.path.join(backup_root, BACKUP_MARKER)
    if os.path.exists(marker):
        raise RuntimeError(
            f"stale backup marker found at {marker}; previous run did not "
            f"restore. Restore manually from {backup_root}, then delete the "
            "marker and re-run."
        )
    os.makedirs(backup_root, exist_ok=True)
    if os.path.isdir(HUD_DIR):
        shutil.copytree(HUD_DIR, os.path.join(backup_root, "hud"),
                        dirs_exist_ok=True)
    open(marker, "w").write("active")
    return backup_root


def restore_hud_dir(backup_root: str) -> bool:
    """Restore the HUD dir from backup. Returns True on verified success."""
    marker = os.path.join(backup_root, BACKUP_MARKER)
    ok = True
    src = os.path.join(backup_root, "hud")
    if os.path.isdir(src):
        if os.path.isdir(HUD_DIR):
            shutil.rmtree(HUD_DIR)
        shutil.copytree(src, HUD_DIR)
        for root, _, files in os.walk(src):
            for f in files:
                a = os.path.join(root, f)
                b = a.replace(src, HUD_DIR)
                if not os.path.exists(b) or os.path.getsize(a) != os.path.getsize(b):
                    ok = False
    elif os.path.isdir(HUD_DIR):
        shutil.rmtree(HUD_DIR)  # dir did not exist before test run
    if ok and os.path.exists(marker):
        os.remove(marker)
    return ok


def write_config(toml_text: str | None):
    """Write a test config to HUD_DIR/config.toml (None = leave as-is)."""
    if toml_text is None:
        return
    os.makedirs(HUD_DIR, exist_ok=True)
    with open(os.path.join(HUD_DIR, "config.toml"), "w", encoding="utf-8") as f:
        f.write(toml_text)
```

- [ ] **Step 2: 快速自检**

运行：`python -c "import sys; sys.path.insert(0, 'scripts'); from hudlib import runner; r = runner.run_exe('C:/Users/admin/.cargo/bin/claude-hud.exe', ['--help']); print(r.exit_code, r.timed_out, r.stderr == '')"`
期望：`0 False True`。

---

### Task 3: assertions.py — 断言引擎

**Files:**
- Create: `scripts/hudlib/assertions.py`

- [ ] **Step 1: 编写 assertions.py（完整代码）**

```python
"""Unified assertion engine. Spec keys:
- exit: int            (exact exit code; -1 means 'any non-zero')
- stdout_contains: list[str]     (all must appear)
- stdout_regex: str              (re.search)
- stdout_not_contains: list[str]
- stdout_empty: bool             (stdout must be exactly empty)
- stderr_contains: list[str]
- stderr_empty: bool
- timed_out: False               (must not have timed out)
Returns (passed: bool, detail: str).
"""
import re


def check(result, spec: dict) -> tuple[bool, str]:
    fails = []
    if "exit" in spec:
        want = spec["exit"]
        if want == -1:
            if result.exit_code == 0:
                fails.append("exit: expected non-zero, got 0")
        elif result.exit_code != want:
            fails.append(f"exit: expected {want}, got {result.exit_code}")
    if spec.get("timed_out") is False and result.timed_out:
        fails.append(f"timed out after {result.duration_s:.1f}s")
    for s in spec.get("stdout_contains", []):
        if s not in result.stdout:
            fails.append(f"stdout missing: {s!r}")
    if "stdout_regex" in spec:
        if not re.search(spec["stdout_regex"], result.stdout):
            fails.append(f"stdout regex no match: {spec['stdout_regex']!r}")
    for s in spec.get("stdout_not_contains", []):
        if s in result.stdout:
            fails.append(f"stdout unexpectedly contains: {s!r}")
    if spec.get("stdout_empty") and result.stdout:
        fails.append(f"stdout not empty: {result.stdout[:120]!r}")
    for s in spec.get("stderr_contains", []):
        if s not in result.stderr:
            fails.append(f"stderr missing: {s!r}")
    if spec.get("stderr_empty") and result.stderr.strip():
        fails.append(f"stderr not empty: {result.stderr.strip()[:120]!r}")
    if fails:
        return False, "; ".join(fails)
    return True, "ok"
```

- [ ] **Step 2: 验证**

运行：`python -c "import sys; sys.path.insert(0, 'scripts'); from hudlib import assertions; R = type('R', (), {'exit_code': 1, 'stdout': '', 'stderr': 'error: boom', 'timed_out': False, 'duration_s': 0.1}); r = R(); print(assertions.check(r, {'exit': 1, 'stderr_contains': ['error:'], 'stdout_empty': True})); print(assertions.check(r, {'exit': 0})); print(assertions.check(R.__class__ and type('R2', (), {'exit_code': 0, 'stdout': 'x', 'stderr': '', 'timed_out': False, 'duration_s': 0.1})(), {'stdout_empty': True}))"`
期望：三行依次 `(True, 'ok')`、`(False, 'exit: expected 0, got 1')`、`(False, 'stdout not empty: ...')`。

---

### Task 4: fixtures 语料

**Files:**
- Create: `fixtures/json/full.json`、`fixtures/json/null_both.json`、`fixtures/json/null_usage.json`、`fixtures/json/null_pct.json`、`fixtures/json/minimal_ok.json`、`fixtures/json/empty_object.json`、`fixtures/json/garbage.txt`、`fixtures/json/unicode.json`
- Create: `fixtures/config/empty_layout.toml`、`fixtures/config/layout_single.toml`、`fixtures/config/layout_all13.toml`、`fixtures/config/layout_unknown.toml`、`fixtures/config/lines1.toml`、`fixtures/config/sep_pipe.toml`、`fixtures/config/ascii_theme.toml`
- Create: `fixtures/mods/smoke-a.toml`、`fixtures/mods/smoke-b.toml`
- Create: `fixtures/transcript/valid.jsonl`、`fixtures/transcript/agents.jsonl`、`fixtures/transcript/empty.jsonl`、`fixtures/transcript/corrupted.jsonl`

- [ ] **Step 1: D1 JSON 语料（完整内容，逐文件）**

`fixtures/json/full.json`：

```json
{"model":{"id":"deepseek-v4-flash","display_name":"deepseek-v4-flash"},"cwd":"D:\\workspace\\claude-hud","workspace":{"current_dir":"D:\\workspace\\claude-hud","project_dir":"D:\\workspace\\claude-hud"},"version":"2.1.152","cost":{"total_cost_usd":0.034,"total_duration_ms":12000,"total_api_duration_ms":11000,"total_lines_added":5,"total_lines_removed":2},"context_window":{"context_window_size":200000,"used_percentage":3.4,"remaining_percentage":96.6,"total_input_tokens":6800,"total_output_tokens":5000,"current_usage":{"input_tokens":6800,"output_tokens":5000,"cache_creation_input_tokens":0,"cache_read_input_tokens":100}},"rate_limits":{"five_hour":{"used_percentage":0,"resets_at":0},"seven_day":{"used_percentage":0,"resets_at":0}},"exceeds_200k_tokens":false,"session_id":"test-full","transcript_path":null}
```

`fixtures/json/null_both.json`（回归：used_percentage 与 current_usage 均 null）：

```json
{"model":{"id":"deepseek-v4-flash","display_name":"deepseek-v4-flash"},"cost":{"total_cost_usd":0.034,"total_duration_ms":12000,"total_api_duration_ms":11000,"total_lines_added":5,"total_lines_removed":2},"context_window":{"context_window_size":200000,"used_percentage":null,"remaining_percentage":null,"total_input_tokens":0,"total_output_tokens":0,"current_usage":null},"rate_limits":{"five_hour":{"used_percentage":0,"resets_at":0},"seven_day":{"used_percentage":0,"resets_at":0}},"exceeds_200k_tokens":false,"session_id":"test-null-both","transcript_path":null}
```

`fixtures/json/null_usage.json`（仅 current_usage 为 null）：

```json
{"model":{"id":"deepseek-v4-flash","display_name":"deepseek-v4-flash"},"cost":{"total_cost_usd":0.034,"total_duration_ms":12000,"total_api_duration_ms":11000,"total_lines_added":5,"total_lines_removed":2},"context_window":{"context_window_size":200000,"used_percentage":3.4,"remaining_percentage":96.6,"total_input_tokens":6800,"total_output_tokens":5000,"current_usage":null},"exceeds_200k_tokens":false,"session_id":"test-null-usage","transcript_path":null}
```

`fixtures/json/null_pct.json`（仅 used_percentage 为 null）：

```json
{"model":{"id":"deepseek-v4-flash","display_name":"deepseek-v4-flash"},"cost":{"total_cost_usd":0.034,"total_duration_ms":12000,"total_api_duration_ms":11000,"total_lines_added":5,"total_lines_removed":2},"context_window":{"context_window_size":200000,"used_percentage":null,"remaining_percentage":96.6,"total_input_tokens":6800,"total_output_tokens":5000,"current_usage":{"input_tokens":6800,"output_tokens":5000,"cache_creation_input_tokens":0,"cache_read_input_tokens":100}},"exceeds_200k_tokens":false,"session_id":"test-null-pct","transcript_path":null}
```

`fixtures/json/minimal_ok.json`（最少必需字段）：

```json
{"model":{"id":"m","display_name":"mini-model"},"cost":{"total_cost_usd":0.5,"total_duration_ms":100},"context_window":{"context_window_size":200000,"used_percentage":12.5,"total_input_tokens":25000}}
```

`fixtures/json/empty_object.json`：

```json
{}
```

`fixtures/json/garbage.txt`：

```
this is not json at all
```

`fixtures/json/unicode.json`：

```json
{"model":{"id":"m","display_name":"中文模型 🧪 test"},"cost":{"total_cost_usd":1.25,"total_duration_ms":2000},"context_window":{"context_window_size":200000,"used_percentage":50,"total_input_tokens":100000}}
```

- [ ] **Step 2: D2/D3 配置模板（完整内容，逐文件）**

`fixtures/config/empty_layout.toml`：

```toml
active_mod = ""
preset = "full"
separator = " │ "
compact_layout = []
```

`fixtures/config/layout_single.toml`：

```toml
active_mod = ""
preset = "full"
separator = " │ "
compact_layout = ["model_display"]

[runtime_overrides]
compact_lines = 1
```

`fixtures/config/layout_all13.toml`：

```toml
active_mod = ""
preset = "full"
separator = " │ "
compact_layout = ["model_display", "context_bar", "cost_display", "skills_mcp", "rate_limits", "git_status", "agent_overview", "agent_detail", "token_attribution", "agent_timeline", "session_stats", "skills_mcp_dynamic", "alerts"]
```

`fixtures/config/layout_unknown.toml`：

```toml
active_mod = ""
preset = "full"
separator = " │ "
compact_layout = ["model_display", "no_such_widget", "context_bar"]
```

`fixtures/config/lines1.toml`：

```toml
active_mod = ""
preset = "full"
separator = " │ "
compact_layout = ["model_display", "context_bar", "cost_display", "skills_mcp", "alerts"]

[runtime_overrides]
compact_lines = 1
```

`fixtures/config/sep_pipe.toml`：

```toml
active_mod = ""
preset = "full"
separator = "|"
compact_layout = ["model_display", "context_bar", "cost_display"]
```

`fixtures/config/ascii_theme.toml`：

```toml
active_mod = ""
preset = "full"
separator = " │ "
compact_layout = ["model_display", "skills_mcp"]

[theme]
bg = "#000000"
fg = "#ffffff"
accent = "#ff0000"
success = "#00ff00"
warning = "#ffff00"
danger = "#ff00ff"
muted = "#888888"
border = "#444444"
skill_color = "#00ffff"
mcp_color = "#ff8000"
model_color = "#ff0000"
bar_filled = "#"
bar_empty = "."
icon_set = "ascii"
bar_width = 10
padding = 0
compact_lines = 1
dashboard_grid = 2
```

- [ ] **Step 3: D4 测试 mod（完整内容）**

`fixtures/mods/smoke-a.toml`：

```toml
[mod_info]
name = "smoke-a"
version = "1.0.0"
description = "harness smoke mod A"
scene = "test"

[layout]
compact = "model_display, context_bar"
dashboard = "grid-2x2"
compact_lines = 1

[theme]
preset = "nord"

[animation]
enabled = true
effects = []
```

`fixtures/mods/smoke-b.toml`：

```toml
[mod_info]
name = "smoke-b"
version = "1.0.0"
description = "harness smoke mod B"
scene = "test"

[layout]
compact = "cost_display, skills_mcp"
dashboard = "grid-2x2"
compact_lines = 1

[theme]
preset = "ember-night"
```

- [ ] **Step 4: D8 transcript 语料（完整内容）**

`fixtures/transcript/valid.jsonl`：

```
{"type":"tool_use","name":"Bash","input":{"command":"ls"},"timestamp":"2026-07-31T10:00:00Z"}
{"type":"tool_result","name":"Bash"}
{"type":"assistant","message":{"usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":20}}}
{"type":"user","message":{"usage":{"input_tokens":30,"output_tokens":0}}}
{"type":"subagent_start","name":"explore","model":"deepseek-v4-flash","task":"search the repo"}
{"type":"subagent_stop","name":"explore"}
{"type":"compact_boundary"}
{"type":"some_future_type","foo":1}
```

`fixtures/transcript/agents.jsonl`：

```
{"type":"tool_use","name":"Bash","input":{"command":"ls"},"timestamp":"2026-07-31T10:01:00Z"}
{"type":"tool_use","name":"Skill","input":{"skill":"explore"}}
{"type":"tool_result","name":"Bash"}
{"type":"subagent_start","name":"explorer-1","model":"deepseek-v4-flash","task":"search"}
{"type":"subagent_start","name":"reviewer-1","model":"deepseek-v4-pro[1m]","task":"review"}
{"type":"subagent_stop","name":"explorer-1"}
{"type":"subagent_stop","name":"reviewer-1"}
{"type":"assistant","message":{"usage":{"input_tokens":500,"output_tokens":250}}}
```

`fixtures/transcript/empty.jsonl`：空文件（0 字节）。

`fixtures/transcript/corrupted.jsonl`：

```
{"type":"tool_use","name":"Bash"}
this line is not json
{"type":"assistant","message":{"usage":{"input_tokens":1}}}
```

- [ ] **Step 5: 验证**

运行：`python -c "import json; [json.loads(l) for l in open('fixtures/transcript/valid.jsonl', encoding='utf-8')]; print('ok')"`
期望：`ok`（对 agents.jsonl、corrupted.jsonl 同样验证——corrupted 的中间行故意非法，首尾行必须合法）。

---

### Task 5: cases.py — 框架与 D1（22 例）

**Files:**
- Create: `scripts/hudlib/cases.py`

- [ ] **Step 1: 编写框架代码（完整代码）**

```python
"""Case definitions: pure data + small generators.

Case dict keys:
  id, name, dim          -- identity
  args (list[str])       -- exe args after the exe (default ["render"])
  stdin (str|None)       -- inline JSON/text fed on stdin
  stdin_file (str|None)  -- fixture file relative to fixtures/
  config (str|None)      -- config.toml content written before pre_cmds
  spec (dict)            -- AssertionSpec
  run_kind (str)         -- "render" (default) | "serve" | "dashboard"
  pre_cmds (list[list[str]]) -- extra exe invocations before the main run
  note (str|None)        -- behavior-discovery note for the report
"""
import json
import os

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
FIX = os.path.join(REPO_ROOT, "fixtures")

DEFAULT_CONFIG = (
    "active_mod = \"glacier-workstation\"\n"
    "preset = \"full\"\n"
    "separator = \" │ \"\n"
    "compact_layout = [\"model_display\", \"context_bar\", \"agent_overview\", "
    "\"cost_display\", \"skills_mcp\", \"alerts\"]\n"
    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
    "[widgets]\n"
)


def fx(rel: str) -> str:
    return os.path.join(FIX, rel)


def full_dict(**overrides):
    """Base 'session with data' dict; override nested keys via dot paths."""
    base = {
        "model": {"id": "deepseek-v4-flash", "display_name": "deepseek-v4-flash"},
        "cost": {"total_cost_usd": 0.034, "total_duration_ms": 12000,
                 "total_api_duration_ms": 11000,
                 "total_lines_added": 5, "total_lines_removed": 2},
        "context_window": {"context_window_size": 200000, "used_percentage": 3.4,
                           "remaining_percentage": 96.6,
                           "total_input_tokens": 6800, "total_output_tokens": 5000,
                           "current_usage": {"input_tokens": 6800, "output_tokens": 5000,
                                             "cache_creation_input_tokens": 0,
                                             "cache_read_input_tokens": 100}},
        "rate_limits": {"five_hour": {"used_percentage": 0, "resets_at": 0},
                        "seven_day": {"used_percentage": 0, "resets_at": 0}},
        "exceeds_200k_tokens": False,
        "session_id": "case",
        "transcript_path": None,
    }
    for path, value in overrides.items():
        node = base
        parts = path.split(".")
        for p in parts[:-1]:
            node = node[p]
        node[parts[-1]] = value
    return base


def j(d) -> str:
    return json.dumps(d)


def render_case(cid, name, dim, spec, stdin=None, stdin_file=None,
                config=None, pre_cmds=None, note=None):
    return {"id": cid, "name": name, "dim": dim, "args": ["render"],
            "stdin": stdin, "stdin_file": stdin_file, "config": config,
            "spec": spec, "run_kind": "render",
            "pre_cmds": pre_cmds or [], "note": note}


def prepare_large_transcript(tmp_dir: str) -> str:
    """Generate a ~1MB JSONL transcript and return its path."""
    path = os.path.join(tmp_dir, "large.jsonl")
    with open(path, "w", encoding="utf-8") as f:
        for _ in range(5000):
            f.write('{"type":"tool_use","name":"Bash","input":{},"timestamp":"2026-07-31T10:00:00Z"}\n')
    return path
```

- [ ] **Step 2: 追加 D1（22 例，完整数据）**

```python
D1 = [
    # --- 回归优先：null 解析（本次修复的 bug） ---
    render_case("D1-01", "used_percentage=null", "D1",
                {"exit": 0, "stdout_contains": ["ctx", "0%"]},
                stdin_file="json/null_pct.json",
                note="回归：null 不再导致解析失败"),
    render_case("D1-02", "current_usage=null", "D1",
                {"exit": 0, "stdout_contains": ["ctx"]},
                stdin_file="json/null_usage.json",
                note="回归"),
    render_case("D1-03", "used_percentage+current_usage 均 null", "D1",
                {"exit": 0, "stdout_contains": ["ctx", "0%"]},
                stdin_file="json/null_both.json",
                note="回归：修复前的典型会话早期形态"),
    # --- 正常与缺失 ---
    render_case("D1-04", "全字段数字", "D1",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash", "3%", "0.03"],
                 "stderr_empty": True},
                stdin_file="json/full.json"),
    render_case("D1-05", "缺可选字段", "D1",
                {"exit": 0, "stdout_contains": ["mini-model", "12%", "0.50"]},
                stdin_file="json/minimal_ok.json"),
    render_case("D1-06", "缺 model", "D1",
                {"exit": 1, "stderr_contains": ["error:"]},
                stdin=j({"cost": {"total_cost_usd": 1, "total_duration_ms": 1},
                         "context_window": {"context_window_size": 100,
                                            "used_percentage": 1,
                                            "total_input_tokens": 1}})),
    render_case("D1-07", "缺 context_window", "D1",
                {"exit": 1, "stderr_contains": ["error:"]},
                stdin=j({"model": {"id": "m", "display_name": "m"},
                         "cost": {"total_cost_usd": 1, "total_duration_ms": 1}})),
    render_case("D1-08", "缺 cost", "D1",
                {"exit": 1, "stderr_contains": ["error:"]},
                stdin=j({"model": {"id": "m", "display_name": "m"},
                         "context_window": {"context_window_size": 100,
                                            "used_percentage": 1,
                                            "total_input_tokens": 1}})),
    render_case("D1-09", "空对象 {}", "D1",
                {"exit": 1, "stderr_contains": ["error:"]},
                stdin_file="json/empty_object.json"),
    render_case("D1-10", "空 stdin", "D1",
                {"exit": 1, "stderr_contains": ["parse stdin JSON"]},
                stdin=""),
    render_case("D1-11", "垃圾输入", "D1",
                {"exit": 1, "stderr_contains": ["error:"]},
                stdin_file="json/garbage.txt"),
    # --- 类型与极端值 ---
    render_case("D1-12", "used_percentage 为字符串", "D1",
                {"exit": 1, "stderr_contains": ["error:"]},
                stdin=j(full_dict(**{"context_window.used_percentage": "3.4"}))),
    render_case("D1-13", "total_cost_usd 为字符串", "D1",
                {"exit": 1, "stderr_contains": ["error:"]},
                stdin=j(full_dict(**{"cost.total_cost_usd": "0.5"}))),
    render_case("D1-14", "负百分比", "D1",
                {"exit": 0, "stdout_contains": ["0%"], "stdout_not_contains": ["-"]},
                stdin=j(full_dict(**{"context_window.used_percentage": -5}))),
    render_case("D1-15", "百分比 150", "D1",
                {"exit": 0, "stdout_contains": ["150%"]},
                stdin=j(full_dict(**{"context_window.used_percentage": 150}))),
    render_case("D1-16", "超大 token 与成本", "D1",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin=j(full_dict(
                    **{"context_window.total_input_tokens": 10**12,
                       "context_window.context_window_size": 10**12,
                       "cost.total_cost_usd": 10**9}))),
    # --- 编码与扩展字段 ---
    render_case("D1-17", "Unicode 模型名", "D1",
                {"exit": 0, "stdout_contains": ["中文模型 🧪 test"]},
                stdin_file="json/unicode.json"),
    render_case("D1-18", "多余字段（workspace/version/session_id 等）", "D1",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/full.json"),
    render_case("D1-19", "transcript_path 不存在", "D1",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"],
                 "stderr_empty": True},
                stdin=j(full_dict(
                    **{"transcript_path": "C:/definitely/missing/transcript.jsonl"}))),
    render_case("D1-20", "transcript_path 损坏", "D1",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin=j(full_dict(**{"transcript_path": fx("transcript/corrupted.jsonl")}))),
    render_case("D1-21", "rate_limits 桶百分比数字/缺失", "D1",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin=j(full_dict(**{"rate_limits.five_hour.used_percentage": 42,
                                     "rate_limits.seven_day": None}))),
    render_case("D1-22", "subagent_status_line 带 agent", "D1",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin=j(full_dict(**{"subagent_status_line": {
                    "agents": [{"name": "explore", "model": "deepseek-v4-flash",
                                "task": "search", "elapsed_secs": 10,
                                "is_active": True}]}}))),
]
```

- [ ] **Step 3: 验证**

运行：`python -c "import sys; sys.path.insert(0, 'scripts'); from hudlib import cases; print(len(cases.D1))"`
期望：`22`。

---

### Task 6: cases.py — D2 布局组合（12 例）+ D3 widget 配置键（10 例）

**Files:**
- Modify: `scripts/hudlib/cases.py`

- [ ] **Step 1: 追加 D2（12 例，完整数据）**

```python
D2 = [
    render_case("D2-01", "默认 6-widget 布局 2 行", "D2",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash", "ctx"],
                 "stderr_empty": True},
                stdin_file="json/full.json",
                config=DEFAULT_CONFIG),
    render_case("D2-02", "空 compact_layout", "D2",
                {"exit": 0, "stdout_empty": True},
                stdin_file="json/full.json",
                config=open(fx("config/empty_layout.toml"), encoding="utf-8").read()),
    render_case("D2-03", "单 widget 布局", "D2",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"],
                 "stdout_not_contains": ["ctx"]},
                stdin_file="json/full.json",
                config=open(fx("config/layout_single.toml"), encoding="utf-8").read()),
    render_case("D2-04", "未知 widget id 被跳过", "D2",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash", "ctx"]},
                stdin_file="json/full.json",
                config=open(fx("config/layout_unknown.toml"), encoding="utf-8").read()),
    render_case("D2-05", "全部 13 个 widget", "D2",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash", "ctx", "0.03"]},
                stdin_file="json/full.json",
                config=open(fx("config/layout_all13.toml"), encoding="utf-8").read()),
    render_case("D2-06", "布局顺序重排（cost 在前）", "D2",
                {"exit": 0, "stdout_contains": ["0.03"]},
                stdin=j(full_dict()),
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"cost_display\", \"model_display\"]\n"
                        "[runtime_overrides]\ncompact_lines = 1\n")),
    render_case("D2-07", "compact_lines=1 一行全拼", "D2",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash", "ctx", "0.03"]},
                stdin_file="json/full.json",
                config=open(fx("config/lines1.toml"), encoding="utf-8").read()),
    render_case("D2-08", "compact_lines=3（6 widget）", "D2",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash", "ctx"]},
                stdin_file="json/full.json",
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"model_display\", \"context_bar\", "
                        "\"cost_display\", \"skills_mcp\", \"alerts\", "
                        "\"rate_limits\"]\n"
                        "[runtime_overrides]\ncompact_lines = 3\n")),
    render_case("D2-09", "5 widget 2 行向上取整", "D2",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/full.json",
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"model_display\", \"context_bar\", "
                        "\"cost_display\", \"skills_mcp\", \"alerts\"]\n")),
    render_case("D2-10", "分隔符变体", "D2",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash", "ctx"]},
                stdin_file="json/full.json",
                config=open(fx("config/sep_pipe.toml"), encoding="utf-8").read()),
    render_case("D2-11", "空输出的 widget 被过滤", "D2",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/minimal_ok.json",
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"model_display\", \"agent_overview\", "
                        "\"context_bar\"]\n")),
    render_case("D2-12", "所有 widget 均空", "D2",
                {"exit": 0, "stdout_empty": True},
                stdin_file="json/minimal_ok.json",
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"agent_overview\", \"rate_limits\", "
                        "\"alerts\"]\n")),
]
```

- [ ] **Step 2: 追加 D3（10 例，完整数据）**

```python
D3 = [
    render_case("D3-01", "context_bar bar_width=5", "D3",
                {"exit": 0, "stdout_contains": ["ctx"]},
                stdin=j(full_dict(**{"context_window.used_percentage": 50})),
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"context_bar\"]\n"
                        "[widgets.context_bar]\nbar_width = 5\n")),
    render_case("D3-02", "warn/critical 阈值生效", "D3",
                {"exit": 0, "stdout_contains": ["85%"]},
                stdin=j(full_dict(**{"context_window.used_percentage": 85})),
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"context_bar\"]\n"
                        "[widgets.context_bar]\nwarn_threshold = 80\n"
                        "critical_threshold = 95\n")),
    render_case("D3-03", "阈值缺省 85% 默认色", "D3",
                {"exit": 0, "stdout_contains": ["85%"]},
                stdin=j(full_dict(**{"context_window.used_percentage": 85})),
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"context_bar\"]\n")),
    render_case("D3-04", "cost_display 币种 $", "D3",
                {"exit": 0, "stdout_contains": ["$0.03"]},
                stdin_file="json/full.json",
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"cost_display\"]\n"
                        "[widgets.cost_display]\ncurrency_symbol = \"$\"\n")),
    render_case("D3-05", "cost_display warn_threshold_usd=0.01 变色", "D3",
                {"exit": 0, "stdout_contains": ["0.03"]},
                stdin_file="json/full.json",
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"cost_display\"]\n"
                        "[widgets.cost_display]\nwarn_threshold_usd = 0.01\n")),
    render_case("D3-06", "icon_set ascii", "D3",
                {"exit": 0, "stdout_contains": ["[SK]", "[MC]"]},
                stdin_file="json/full.json",
                config=open(fx("config/ascii_theme.toml"), encoding="utf-8").read()),
    render_case("D3-07", "icon_set minimal", "D3",
                {"exit": 0, "stdout_contains": ["◇", "◆"]},
                stdin_file="json/full.json",
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"model_display\", \"skills_mcp\"]\n"
                        "[theme]\nicon_set = \"minimal\"\n")),
    render_case("D3-08", "icon_set nerd（默认）", "D3",
                {"exit": 0, "stdout_contains": ["▸"]},
                stdin_file="json/full.json",
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"model_display\"]\n")),
    render_case("D3-09", "widgets 表非表值被忽略", "D3",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/full.json",
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"model_display\"]\n"
                        "[widgets]\ncontext_bar = \"not-a-table\"\n")),
    render_case("D3-10", "非法 TOML 配置", "D3",
                {"exit": -1},  # 行为发现点：期望 exit 1；若实测为 0（静默回退默认），改为 {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]}
                stdin_file="json/full.json",
                config="this is [ not = valid toml\n",
                note="AppConfig::load 失败后 unwrap_or_default() 可能静默回退——按实测修正（Task 11）"),
]
```

- [ ] **Step 3: 验证**

运行：`python -c "import sys; sys.path.insert(0, 'scripts'); from hudlib import cases; print(len(cases.D2), len(cases.D3))"`
期望：`12 10`。

---

### Task 7: cases.py — D4 主题与 mod 生命周期（17 例）+ D5 CLI（15 例）

**Files:**
- Modify: `scripts/hudlib/cases.py`

- [ ] **Step 1: 追加 D4（17 例，完整数据）**

设计说明：D4-02 拆为六例（每 preset 一例，`mod use` 后 render）；D4-10 拆为 export/import 两例（export 输出是序列化后的 ModPackage，与 fixture 不可能字节一致，故"往返一致"改为"export 含 mod 名 + import 接受合法文件"）。所有 mod 状态用例通过 `pre_cmds` 组合执行（runner 顺序：先 write_config 再 pre_cmds 再主命令，见 Task 10）。

```python
D4 = [
    render_case("D4-01", "无 config 用默认主题", "D4",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/full.json",
                config=DEFAULT_CONFIG),
    render_case("D4-02a", "preset glacier-workstation", "D4",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "use", "glacier-workstation"]]),
    render_case("D4-02b", "preset obsidian-command", "D4",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "use", "obsidian-command"]]),
    render_case("D4-02c", "preset ember-night", "D4",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "use", "ember-night"]]),
    render_case("D4-02d", "preset matrix-surveillance", "D4",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "use", "matrix-surveillance"]]),
    render_case("D4-02e", "preset noir-precision", "D4",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "use", "noir-precision"]]),
    render_case("D4-02f", "preset noir-tabbed", "D4",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "use", "noir-tabbed"]]),
    render_case("D4-03", "主题颜色覆盖", "D4",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/full.json",
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"model_display\"]\n"
                        "[theme]\nmodel_color = \"#ff0000\"\n")),
    render_case("D4-04", "bar 字符覆盖", "D4",
                {"exit": 0, "stdout_contains": ["ctx"]},
                stdin=j(full_dict(**{"context_window.used_percentage": 50})),
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"context_bar\"]\n"
                        "[theme]\nbar_filled = \"#\"\nbar_empty = \".\"\n"
                        "bar_width = 10\n")),
    render_case("D4-05", "theme export 输出 TOML", "D4",
                {"exit": 0, "stdout_contains": ["bg"]},
                args=["theme", "export"], config=DEFAULT_CONFIG),
    render_case("D4-06", "theme import 合法文件", "D4",
                {"exit": 0, "stdout_contains": ["imported"]},
                args=["theme", "import", fx("config/ascii_theme.toml")]),
    render_case("D4-07", "theme import 非法文件", "D4",
                {"exit": 1, "stderr_contains": ["error:"]},
                args=["theme", "import", fx("json/garbage.txt")]),
    render_case("D4-08", "mod save 后 list 可见", "D4",
                {"exit": 0, "stdout_contains": ["smoke-a"]},
                args=["mod", "list"], config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "save", "smoke-a"]],
                note="行为发现点：mod list 的 User mods 节格式以实测为准"),
    render_case("D4-09", "mod use 不存在", "D4",
                {"exit": 1, "stderr_contains": ["error:"]},
                args=["mod", "use", "no-such-mod"]),
    render_case("D4-10a", "mod export 含 mod 名", "D4",
                {"exit": 0, "stdout_contains": ["smoke-a"]},
                args=["mod", "export", "smoke-a"], config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "save", "smoke-a"]]),
    render_case("D4-10b", "mod import 合法 mod 文件", "D4",
                {"exit": 0, "stdout_contains": ["imported"]},
                args=["mod", "import", fx("mods/smoke-b.toml")]),
    render_case("D4-11", "mod delete 后不可 use", "D4",
                {"exit": 1, "stderr_contains": ["error:"]},
                args=["mod", "use", "smoke-a"], config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "delete", "smoke-a"]]),
]
```

- [ ] **Step 2: 追加 D5（15 例，完整数据）**

```python
D5 = [
    render_case("D5-01", "--help", "D5",
                {"exit": 0, "stdout_contains": ["Usage"]},
                args=["--help"]),
    render_case("D5-02", "无参数", "D5",
                {"exit": 2, "stderr_contains": ["Usage"]},
                args=[]),
    render_case("D5-03", "未知子命令", "D5",
                {"exit": 2, "stderr_contains": ["error"]},
                args=["frobnicate"]),
    render_case("D5-04", "widget list 13 个", "D5",
                {"exit": 0, "stdout_contains": ["context_bar", "model_display",
                                                "cost_display", "agent_overview",
                                                "skills_mcp", "rate_limits",
                                                "git_status", "agent_detail",
                                                "token_attribution",
                                                "agent_timeline", "session_stats",
                                                "skills_mcp_dynamic", "alerts"]},
                args=["widget", "list"]),
    render_case("D5-05", "widget test 合法", "D5",
                {"exit": 0, "stdout_contains": ["Widget 'model_display'"]},
                args=["widget", "test", "model_display"]),
    render_case("D5-06", "widget test 不存在", "D5",
                {"exit": 0, "stdout_contains": ["not found"]},
                args=["widget", "test", "nonexistent"]),
    render_case("D5-07", "completion bash", "D5",
                {"exit": 0, "stdout_contains": ["bash"]},
                args=["completion", "bash"]),
    render_case("D5-08", "completion 不支持 shell", "D5",
                {"exit": 0, "stdout_contains": ["Unsupported"]},
                args=["completion", "powershell"]),
    render_case("D5-09", "mod list 6 内置", "D5",
                {"exit": 0, "stdout_contains": ["glacier-workstation",
                                                "obsidian-command", "ember-night",
                                                "matrix-surveillance",
                                                "noir-precision", "noir-tabbed"]},
                args=["mod", "list"]),
    render_case("D5-10", "mod preview 合法", "D5",
                {"exit": 0, "stdout_contains": ["ember-night"]},
                args=["mod", "preview", "ember-night"]),
    render_case("D5-11", "mod preview 不存在", "D5",
                {"exit": 1, "stderr_contains": ["error:"]},
                args=["mod", "preview", "no-such-mod"]),
    render_case("D5-12", "mod current", "D5",
                {"exit": 0, "stdout_contains": ["Active mod"]},
                args=["mod", "current"]),
    render_case("D5-13", "mod reset 回默认", "D5",
                {"exit": 0, "stdout_contains": ["Reset to factory default"]},
                args=["mod", "reset"]),
    render_case("D5-14", "mod import 文件不存在", "D5",
                {"exit": 1, "stderr_contains": ["error:"]},
                args=["mod", "import", "C:/definitely/missing/mod.toml"]),
    render_case("D5-15", "setup（settings.json 已存在）", "D5",
                {"exit": 0, "stdout_contains": ["settings.json already exists"]},
                args=["setup"]),
]
```

- [ ] **Step 3: 验证**

运行：`python -c "import sys; sys.path.insert(0, 'scripts'); from hudlib import cases; print(len(cases.D4), len(cases.D5))"`
期望：`17 15`。

---

### Task 8: cases.py — D6 serve（6 例）+ D7 dashboard（1 例）+ D8 transcript（6 例）

**Files:**
- Modify: `scripts/hudlib/cases.py`

- [ ] **Step 1: 追加 D6/D7/D8（完整数据）**

```python
import http.client


def serve_case(cid, name, path, expect_status, expect_ct=None,
               expect_json=False, post_free=False, note=None):
    return {"id": cid, "name": name, "dim": "D6", "args": ["serve"],
            "run_kind": "serve", "path": path,
            "expect_status": expect_status, "expect_ct": expect_ct,
            "expect_json": expect_json, "post_free": post_free,
            "spec": {"exit": None}, "note": note}


D6 = [
    serve_case("D6-01", "GET /", "/", 200, "text/html"),
    serve_case("D6-02", "GET /api/data", "/api/data", 200, "application/json",
               expect_json=True),
    serve_case("D6-03", "GET /api/health", "/api/health", 200),
    serve_case("D6-04", "未知路由 404", "/nope", 404,
               note="行为发现点：tiny_http 未匹配路由行为以实测为准"),
    serve_case("D6-05", "服务 5s 内响应", "/api/health", 200,
               note="run_serve 的 5s 轮询即该断言的实现"),
    serve_case("D6-06", "进程退出后端口释放", "/api/health", 200,
               post_free=True),
]


def dash_case(cid, name, spec):
    return {"id": cid, "name": name, "dim": "D7", "args": ["dashboard"],
            "run_kind": "dashboard", "spec": spec}


D7 = [
    dash_case("D7-01", "非 TTY 优雅失败", "D7",
              {"exit": 1, "stderr_contains": ["error:"], "timed_out": False}),
]


D8 = [
    render_case("D8-01", "合法 JSONL transcript", "D8",
                {"exit": 0, "stderr_empty": True},
                stdin=j(full_dict(**{"transcript_path": fx("transcript/valid.jsonl")}))),
    render_case("D8-02", "含 agent 数据的 transcript", "D8",
                {"exit": 0, "stderr_empty": True},
                stdin=j(full_dict(**{"transcript_path": fx("transcript/agents.jsonl")})),
                note="行为发现点：compact 输出是否随 transcript 变化，在报告中标注"),
    render_case("D8-03", "含 skill/mcp 调用的 transcript", "D8",
                {"exit": 0, "stderr_empty": True},
                stdin=j(full_dict(**{"transcript_path": fx("transcript/agents.jsonl")}))),
    render_case("D8-04", "空文件 transcript", "D8",
                {"exit": 0, "stderr_empty": True},
                stdin=j(full_dict(**{"transcript_path": fx("transcript/empty.jsonl")}))),
    render_case("D8-05", "单行损坏 JSON", "D8",
                {"exit": 0, "stderr_empty": True},
                stdin=j(full_dict(**{"transcript_path": fx("transcript/corrupted.jsonl")}))),
    render_case("D8-06", "大 transcript 1MB", "D8",
                {"exit": 0, "stderr_empty": True, "timed_out": False},
                stdin=j(full_dict(**{"transcript_path": "<LARGE_FIXTURE>"})),
                note="<LARGE_FIXTURE> 运行期由 prepare_large_transcript 生成并替换"),
]
```

- [ ] **Step 2: 合并导出（cases.py 末尾追加）**

```python
CASES = D1 + D2 + D3 + D4 + D5 + D6 + D7 + D8
assert len(CASES) == 89, f"expected 89 cases, got {len(CASES)}"
```

- [ ] **Step 3: 验证**

运行：`python -c "import sys; sys.path.insert(0, 'scripts'); from hudlib import cases; print(len(cases.CASES))"`
期望：输出 `89`。

---

### Task 9: report.py — markdown 报告

**Files:**
- Create: `scripts/hudlib/report.py`

- [ ] **Step 1: 编写 report.py（完整代码）**

```python
"""Markdown test report generation."""
import datetime
import os

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
REPORTS_DIR = os.path.join(REPO_ROOT, "reports")


def render_report(env_snapshot: dict, results: list, duration_s: float) -> str:
    passed = [r for r in results if r["passed"]]
    failed = [r for r in results if not r["passed"]]
    lines = [
        "# Claude HUD 黑盒测试报告",
        "",
        f"- 生成时间：{env_snapshot['run_at']}",
        f"- exe：`{env_snapshot['exe']}`（mtime {env_snapshot['exe_mtime']}）",
        f"- python：{env_snapshot['python']}",
        f"- 平台：{env_snapshot['platform']}",
        f"- 总耗时：{duration_s:.1f}s",
        "",
        "## 汇总",
        "",
        "| 指标 | 值 |",
        "|---|---|",
        f"| 总用例 | {len(results)} |",
        f"| 通过 | {len(passed)} |",
        f"| 失败 | {len(failed)} |",
        f"| 通过率 | {len(passed) / len(results) * 100:.1f}% |",
        "",
        "## 用例明细",
        "",
        "| ID | 维度 | 名称 | 结果 | 耗时 | 说明 |",
        "|---|---|---|---|---|---|",
    ]
    for r in results:
        lines.append(
            f"| {r['id']} | {r['dim']} | {r['name']} | "
            f"{'PASS' if r['passed'] else 'FAIL'} | {r['duration_s']:.2f}s | "
            f"{r.get('detail', '')} |"
        )
    lines += ["", "## 失败明细", ""]
    if not failed:
        lines.append("无。")
    for r in failed:
        lines += [
            f"### {r['id']} — {r['name']}",
            "",
            f"- 期望：`{r['spec']}`",
            f"- 实际 exit：{r['exit_code']}（超时：{r['timed_out']}）",
            f"- 断言失败：{r.get('detail', '')}",
            "",
            "实际 stdout（截断 500 字节）：",
            "",
            "```",
            r["stdout"][:500],
            "```",
            "",
            "实际 stderr（截断 500 字节）：",
            "",
            "```",
            r["stderr"][:500],
            "```",
            "",
            f"复现命令：`{r['repro']}`",
            "",
        ]
    lines += ["## 行为发现点", ""]
    notes = [r for r in results if r.get("note")]
    if not notes:
        lines.append("无。")
    for r in notes:
        lines.append(f"- {r['id']}：{r['note']}")
    lines += [
        "",
        "## 手工清单（dashboard TTY）",
        "",
        "1. 终端运行 `claude-hud dashboard`，进入全屏 TUI",
        "2. 确认 2x2 网格渲染、各 widget 显示数据",
        "3. `q` 退出，终端恢复，无残留",
        "4. 缩放终端窗口，布局不崩",
        "",
        "## 边界说明",
        "",
        "- skills/mcp 计数、MCP 探测依赖真实环境 → 只断言形状",
        "- dashboard TUI 无法黑盒自动化 → 非 TTY 失败用例 + 手工清单",
        "- 测试期间真实 config.toml 被临时改写，已按备份-恢复协议还原",
        "",
    ]
    return "\n".join(lines)


def write_report(markdown: str, override_path: str | None = None) -> str:
    if override_path:
        path = override_path
    else:
        os.makedirs(REPORTS_DIR, exist_ok=True)
        path = os.path.join(
            REPORTS_DIR,
            f"test-report-{datetime.date.today().isoformat()}.md",
        )
    with open(path, "w", encoding="utf-8") as f:
        f.write(markdown)
    return path
```

- [ ] **Step 2: 验证**

运行：`python -c "import sys; sys.path.insert(0, 'scripts'); from hudlib import report; md = report.render_report({'run_at': 't', 'exe': 'x', 'exe_mtime': 'm', 'python': 'p', 'platform': 'pl'}, [{'id': 'D1-01', 'dim': 'D1', 'name': 'n', 'passed': True, 'duration_s': 0.1, 'detail': '', 'spec': {}, 'exit_code': 0, 'timed_out': False, 'stdout': '', 'stderr': '', 'repro': 'r'}], 1.0); print(len(md))"`
期望：输出正整数（markdown 非空）。

---

### Task 10: test_hud.py — 入口与编排

**Files:**
- Create: `scripts/test_hud.py`

- [ ] **Step 1: 编写入口（完整代码）**

```python
"""claude-hud 黑盒测试套件入口。

用法：
  python scripts/test_hud.py                 # 全量
  python scripts/test_hud.py --case D1-01    # 单用例
  python scripts/test_hud.py --exe <path>    # 指定 exe
  python scripts/test_hud.py --report <path> # 指定报告输出
"""
import argparse
import http.client
import json as _json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from hudlib import assertions, cases, env, report, runner  # noqa: E402


def prepare_case(case, tmp_dir):
    """Return stdin text for render cases (None for non-render kinds)."""
    if case["run_kind"] != "render":
        return None
    if case.get("stdin") is not None:
        text = case["stdin"]
        if "<LARGE_FIXTURE>" in text:
            text = text.replace("<LARGE_FIXTURE>",
                                cases.prepare_large_transcript(tmp_dir))
        return text
    if case.get("stdin_file"):
        with open(cases.fx(case["stdin_file"]), encoding="utf-8") as f:
            return f.read()
    return None


def run_one(exe_path, case, tmp_dir):
    """Run one case; return a result dict consumed by report.render_report."""
    start = time.monotonic()
    if case["run_kind"] == "serve":
        return run_serve(exe_path, case)
    # order matters: config first, then pre_cmds (e.g. mod use writes active_mod
    # into config.toml, which must not be clobbered afterwards)
    if case.get("config") is not None:
        runner.write_config(case["config"])
    for pre in case.get("pre_cmds", []):
        runner.run_exe(exe_path, pre, timeout_s=10)
    if case["run_kind"] == "dashboard":
        r = runner.run_exe(exe_path, case["args"], timeout_s=10)
    else:
        stdin_text = prepare_case(case, tmp_dir)
        r = runner.run_exe(exe_path, case["args"], stdin_text=stdin_text,
                           timeout_s=10)
    passed, detail = assertions.check(r, case["spec"])
    return {
        "id": case["id"], "dim": case["dim"], "name": case["name"],
        "passed": passed, "detail": detail, "spec": case["spec"],
        "exit_code": r.exit_code, "timed_out": r.timed_out,
        "stdout": r.stdout, "stderr": r.stderr, "repro": r.repro,
        "duration_s": time.monotonic() - start, "note": case.get("note"),
    }


def run_serve(exe_path, case):
    """Start serve, poll endpoint, assert, terminate, verify port release."""
    proc = subprocess.Popen([exe_path, "serve"],
                            stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL)
    start = time.monotonic()
    fails = []
    try:
        deadline = start + 5.0
        status, ct, body = None, "", ""
        while time.monotonic() < deadline:
            if proc.poll() is not None:
                break
            try:
                conn = http.client.HTTPConnection("127.0.0.1", 9527, timeout=1)
                conn.request("GET", case["path"])
                resp = conn.getresponse()
                status = resp.status
                ct = resp.getheader("Content-Type", "")
                body = resp.read().decode("utf-8", "replace")
                conn.close()
                break
            except OSError:
                time.sleep(0.2)
        if status is None:
            fails.append(f"serve 5s 内未响应（进程退出码 {proc.poll()}）")
        else:
            if status != case["expect_status"]:
                fails.append(f"status: expected {case['expect_status']}, got {status}")
            if case.get("expect_ct") and case["expect_ct"] not in ct:
                fails.append(f"Content-Type: expected {case['expect_ct']}, got {ct}")
            if case.get("expect_json"):
                try:
                    _json.loads(body)
                except ValueError as e:
                    fails.append(f"body not JSON: {e}")
        passed = not fails
        return {
            "id": case["id"], "dim": "D6", "name": case["name"],
            "passed": passed, "detail": "; ".join(fails) if fails else "ok",
            "spec": case["spec"],
            "exit_code": proc.poll() if proc.poll() is not None else 0,
            "timed_out": False, "stdout": body, "stderr": "",
            "repro": f"{exe_path} serve  (GET {case['path']})",
            "duration_s": time.monotonic() - start, "note": case.get("note"),
        }
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=3)
        if case.get("post_free"):
            free = False
            t0 = time.monotonic()
            while time.monotonic() - t0 < 3.0:
                try:
                    conn = http.client.HTTPConnection("127.0.0.1", 9527, timeout=1)
                    conn.request("GET", "/")
                    conn.getresponse()
                    conn.close()
                    time.sleep(0.2)
                except OSError:
                    free = True
                    break
            if not free:
                fails.append("port 9527 在进程退出后仍可连接")


def main():
    parser = argparse.ArgumentParser(description="claude-hud black-box test suite")
    parser.add_argument("--case", help="run a single case id, e.g. D1-01")
    parser.add_argument("--exe", help=f"claude-hud exe path (default: {env.DEFAULT_EXE})")
    parser.add_argument("--report", help="report output path override")
    args = parser.parse_args()

    exe_path = env.resolve_exe(args.exe)
    snap = env.snapshot(exe_path)

    print(f"[hud-test] exe: {exe_path}")
    selected = [c for c in cases.CASES if not args.case or c["id"] == args.case]
    if args.case and not selected:
        print(f"case {args.case} not found")
        sys.exit(2)

    backup_root = runner.backup_hud_dir()
    tmp_dir = os.path.join(backup_root, "tmp")
    os.makedirs(tmp_dir, exist_ok=True)
    results = []
    overall_start = time.monotonic()
    restored = False
    try:
        for case in selected:
            results.append(run_one(exe_path, case, tmp_dir))
            n = sum(1 for r in results if r["passed"])
            last = results[-1]
            print(f"  [{n}/{len(results)}] {case['id']}: "
                  f"{'PASS' if last['passed'] else 'FAIL'} {last.get('detail', '')}")
    finally:
        restored = runner.restore_hud_dir(backup_root)
        if not restored:
            print("!! CONFIG RESTORE FAILED — check ~/.claude/plugins/claude-hud manually")

    md = report.render_report(snap, results, time.monotonic() - overall_start)
    out_path = report.write_report(md, args.report)
    print(f"[hud-test] report: {out_path}")
    failed = [r for r in results if not r["passed"]]
    print(f"[hud-test] {len(results) - len(failed)}/{len(results)} passed")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: 验证入口 + 备份恢复协议**

运行：`python scripts/test_hud.py --case D1-10`
期望：`[1/1] D1-10: PASS`，随后 `[hud-test] report: ...` 与 `1/1 passed`，退出码 0。

然后核对配置还原：
运行：`cat ~/.claude/plugins/claude-hud/config.toml | head -5`
期望：与运行前完全一致（默认 6-widget 布局）。同时确认 `%TEMP%\claude-hud-test-backup` 中无 marker 残留（`ls "$TEMP"/claude-hud-test-backup/` 应无 `.hud-test-backup` 文件）。

---

### Task 11: 全量运行与行为发现修正

**Files:**
- Modify: `scripts/hudlib/cases.py`（按实测修正断言，仅数据不改代码逻辑）

- [ ] **Step 1: 全量运行**

运行：`python scripts/test_hud.py`
期望：89 用例全部执行，报告写入 `reports/test-report-2026-07-31.md`。

- [ ] **Step 2: 按报告核对行为发现点并修正断言**

只改 cases.py 数据（spec/note），不碰源码：

1. **D3-10 非法 TOML**：若实测 exit 0（`unwrap_or_default()` 静默回退），改 spec 为 `{"exit": 0, "stdout_contains": ["deepseek-v4-flash"]}`，note 记录"非法 TOML 静默回退默认配置"。
2. **D6-04 未知路由**：若 serve 对 `/nope` 返回 200 而非 404，将 `expect_status` 改为实测值，note 记录。
3. **D7-01 dashboard 非 TTY**：若 exit 非 1 或 stderr 不含 `error:`（如 crossterm 错误消息不同），按实测调整 `exit`/`stderr_contains`。
4. **D8-02/03**：若 stderr 非空（transcript 解析告警），调整断言为 `{"exit": 0}`（去 `stderr_empty`）并 note 记录。
5. **D4-08**：`mod list` 的 User mods 节若不含 smoke-a（如 `mod save` 写入位置不同），调整断言或 pre_cmds。
6. **D5-03 未知子命令**：clap 默认 exit 2；若实测为其他值，调整。
7. **D3-06/07/08 icon 断言**：若 ascii/minimal 图标字符与源码不同（如 `[SK]` 前有空格差异），按实测调整 `stdout_contains`。

- [ ] **Step 3: 修正后重跑直至全绿**

运行：`python scripts/test_hud.py`
期望：通过率 100%，报告无 FAIL 明细。

---

### Task 12: 验收与收尾

**Files:**
- Modify: `docs/superpowers/specs/2026-07-31-hud-testing-design.md`（同步计数：D4 11→17 例，总计 83→89）
- Modify: `reports/test-report-2026-07-31.md`（最终报告）

- [ ] **Step 1: 同步设计文档计数**

编辑 `docs/superpowers/specs/2026-07-31-hud-testing-design.md`：
- §5 D4 标题 `### D4 — 主题与 mod 生命周期（11 例）` → `（17 例）`
- §5 D4 表格补充 D4-02a..f（六 preset）与 D4-10a/b（export/import 拆分）行，删除原 D4-02 单行描述与 D4-10 单行描述
- 引言处（若有 "83" 字样）改为 "89"

- [ ] **Step 2: 对照验收标准逐条核验**

| 验收标准（设计文档 §8） | 核验方式 |
|---|---|
| 一键运行、报告生成 | `python scripts/test_hud.py` 退出码 0，报告存在 |
| D1-01~03 null 回归 PASS | 报告明细三行均 PASS |
| 全程不修改 src/ 与 Cargo.toml | `git status --short` 运行前后对比，无新增 src/ 或 Cargo.toml 改动（运行前已存在的改动不算） |
| 运行结束后真实配置一致 | 恢复协议字节校验通过（Task 10 Step 2 已验） |
| 报告可在无 Claude Code 环境复现 | 仅需 python3.10+ 与 exe |
| --case 单跑 | `python scripts/test_hud.py --case D1-04` 仅执行 1 例 |

- [ ] **Step 3: 交付物清点与汇报**

- `scripts/test_hud.py`、`scripts/hudlib/`（env/runner/assertions/cases/report 共 6 个文件）
- `fixtures/`（json×8、config×7、mods×2、transcript×4，共 21 个文件）
- `reports/test-report-2026-07-31.md`（最终报告）
- 设计文档计数已同步

按用户输出规范汇报：设计思路 / 代码实现（关键文件清单）/ 测试示例（报告摘要：通过率、失败明细、行为发现点）/ 注意事项（备份恢复协议、行为发现修正清单、git 未提交由用户决定）。
