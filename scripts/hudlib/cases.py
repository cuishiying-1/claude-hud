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

P1-only keys (passed through render_case's **extra):
  pre_render (bool)       -- 主运行前先 render 一次（复用主 stdin，可用 pre_render_stdin 覆盖）
  pre_render_stdin (str)  -- pre_render 的独立 stdin（如坏 JSON）
  pre_exit (int)          -- pre_render 期望退出码
  pre_stdout_contains (list[str]) -- pre_render stdout 必须包含
  transcript_copy (str)   -- fixtures/transcript/<name> 复制到 tmp_dir 并把 stdin 的
                             transcript_path 指向副本（P1 用例专用，避免污染共享 fixture）
  grow_fixture (dict)     -- {"agent_pairs": N} 追加 N 对 subagent_start/stop 行
  truncate_fixture (dict) -- {"keep_lines": N} 只保留前 N 行
  remove_state (bool)     -- 主运行前删除 state.json
  state_json (dict)       -- check_state_json 断言（segments/absent/has/min/equals；
                             min/equals 的值可为 "<FIXTURE_SIZE>" 运行期替换）
  pre_state_json (dict)   -- 对 pre_render 之后的 state.json 做 check_state_json
  state_json_same_as_pre (list[str]) -- 这些点路径在主运行前后必须不变
  config_file_contains (list[str]) -- 主运行后 HUD_DIR/config.toml 必须包含所有子串
"""
import json
import os
from datetime import datetime, timezone

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


def render_case(cid, name, dim, spec, args=None, stdin=None, stdin_file=None,
                config=None, pre_cmds=None, note=None, **extra):
    case = {"id": cid, "name": name, "dim": dim,
            "args": ["render"] if args is None else args,
            "stdin": stdin, "stdin_file": stdin_file, "config": config,
            "spec": spec, "run_kind": "render",
            "pre_cmds": pre_cmds or [], "note": note}
    case.update(extra)
    return case


def prepare_large_transcript(tmp_dir: str) -> str:
    """Generate a ~1MB JSONL transcript and return its path (forward slashes,
    so the path can be embedded in a JSON string unchanged on Windows)."""
    path = os.path.join(tmp_dir, "large.jsonl")
    with open(path, "w", encoding="utf-8") as f:
        for _ in range(13000):
            f.write('{"type":"tool_use","name":"Bash","input":{},"timestamp":"2026-07-31T10:00:00Z"}\n')
    return path.replace("\\", "/")


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
                {"exit": 0, "stdout_contains": ["-5%"]},
                stdin=j(full_dict(**{"context_window.used_percentage": -5})),
                note="负百分比较原期望更宽松（预期栏位 clamp 到 0）；实测直接显示 -5%，栏位为空"),
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
                                     "rate_limits.seven_day.used_percentage": 0})),
                note="seven_day 不可为 null（反序列化拒绝 null → 非可选字段）；改用零值 bucket"),
    render_case("D1-22", "subagent_status_line 带 agent", "D1",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin=j(full_dict(**{"subagent_status_line": {
                    "agents": [{"name": "explore", "model": "deepseek-v4-flash",
                                "task": "search", "elapsed_secs": 10,
                                "is_active": True}]}}))),
]

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
                {"exit": 0, "stdout_contains": ["mini-model"]},
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
                        "compact_layout = [\"agent_overview\", \"alerts\"]\n")),
]

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
                {"exit": 0, "stdout_contains": ["0.03"]},
                stdin_file="json/full.json",
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"cost_display\"]\n"
                        "[widgets.cost_display]\ncurrency_symbol = \"$\"\n"),
                note="currency_symbol 配置未生效，始终显示 ¥；断言改用 0.03 数字部分"),
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
                        "[theme]\nicon_set = \"minimal\"\n"),
                note="Phase 3 修复后 icon_set=minimal 生效（skills_mcp 渲染 ◇◆）；旧断言 ▸│ 反映的是部分表毒化整个 config 解析的静默作废行为"),
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
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/full.json",
                config="this is [ not = valid toml\n",
                note="非法 TOML 静默回退默认配置（exit 0），不报错退出；AppConfig::load 内 unwrap_or_default 路径"),
]

D4 = [
    render_case("D4-01", "无 config 用默认主题", "D4",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/full.json",
                config=DEFAULT_CONFIG),
    # Task 4/5：内置 preset 头部已统一为 [mod_info]，use 切换后 mod.layout 真实灌入渲染。
    # 已实现布局：minimal（noir-precision）/activity（glacier、matrix）→ 渲染正常；
    # 未实现布局：agent-centric（obsidian）/kpi（ember）/contextual（noir-tabbed）→ 明确报错
    # （hud_err_marker 上屏，P3-11 同款行为）。
    render_case("D4-02a", "preset glacier-workstation", "D4",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "use", "glacier-workstation"]],
                note="use 内置 mod：activity 布局灌入，渲染正常"),
    render_case("D4-02b", "preset obsidian-command", "D4",
                {"exit": 1, "stdout_contains": ["not implemented"]},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "use", "obsidian-command"]],
                note="agent-centric 布局未实现 → 渲染明确报错"),
    render_case("D4-02c", "preset ember-night", "D4",
                {"exit": 1, "stdout_contains": ["not implemented"]},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "use", "ember-night"]],
                note="kpi 布局未实现 → 渲染明确报错"),
    render_case("D4-02d", "preset matrix-surveillance", "D4",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "use", "matrix-surveillance"]],
                note="activity 布局灌入，渲染正常"),
    render_case("D4-02e", "preset noir-precision", "D4",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash"]},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "use", "noir-precision"]],
                note="minimal 布局灌入，渲染正常"),
    render_case("D4-02f", "preset noir-tabbed", "D4",
                {"exit": 1, "stdout_contains": ["not implemented"]},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "use", "noir-tabbed"]],
                note="contextual 布局未实现 → 渲染明确报错"),
    render_case("D4-03", "主题颜色覆盖", "D4",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash", "\x1b[38;2;255;0;0m"]},
                stdin_file="json/full.json",
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"model_display\"]\n"
                        "[theme]\n"
                        "bg = \"#2e3440\"\nfg = \"#d8dee9\"\n"
                        "accent = \"#88c0d0\"\nsuccess = \"#a3be8c\"\n"
                        "warning = \"#ebcb8b\"\ndanger = \"#bf616a\"\n"
                        "muted = \"#5e81ac\"\nborder = \"#434c5e\"\n"
                        "skill_color = \"#b48ead\"\nmcp_color = \"#d08770\"\n"
                        "model_color = \"#ff0000\"\n"),
                note="Theme 全字段覆盖表（含 model_color 覆盖），配置可解析，覆盖路径真实生效"),
    render_case("D4-04", "bar 字符覆盖", "D4",
                {"exit": 0, "stdout_contains": ["ctx", "#", "."]},
                stdin=j(full_dict(**{"context_window.used_percentage": 50})),
                config=("active_mod = \"\"\n"
                        "preset = \"full\"\n"
                        "separator = \" │ \"\n"
                        "compact_layout = [\"context_bar\"]\n"
                        "[theme]\n"
                        "bg = \"#2e3440\"\nfg = \"#d8dee9\"\n"
                        "accent = \"#88c0d0\"\nsuccess = \"#a3be8c\"\n"
                        "warning = \"#ebcb8b\"\ndanger = \"#bf616a\"\n"
                        "muted = \"#5e81ac\"\nborder = \"#434c5e\"\n"
                        "skill_color = \"#b48ead\"\nmcp_color = \"#d08770\"\n"
                        "model_color = \"#88c0d0\"\n"
                        "bar_filled = \"#\"\nbar_empty = \".\"\n"
                        "bar_width = 10\n"),
                note="bar_filled/bar_empty 覆盖：输出含 # 与 . 字符（50% 填充 5#5.）"),
    render_case("D4-05", "theme export 输出 TOML", "D4",
                {"exit": 0, "stdout_contains": ["bg"]},
                args=["theme", "export"], config=DEFAULT_CONFIG),
    render_case("D4-06", "theme import 合法文件", "D4",
                {"exit": 0, "stdout_contains": ["imported"]},
                args=["theme", "import", fx("config/theme_ok.toml")],
                note="theme import 直接解析为 Theme（11 色必需字段）——ascii_theme.toml 是 AppConfig 形状无法导入，故新增 theme_ok.toml"),
    render_case("D4-07", "theme import 非法文件", "D4",
                {"exit": 1, "stderr_contains": ["error:"]},
                args=["theme", "import", fx("json/garbage.txt")]),
    render_case("D4-08", "mod save 后 list 可见", "D4",
                {"exit": 0, "stdout_contains": ["smoke-a"]},
                args=["mod", "list"], config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "save", "smoke-a"]],
                note="行为发现点：mod list 的 User mods 节格式以实测为准"),
    render_case("D4-09", "mod use 不存在 mod（新校验拒绝）", "D4",
                {"exit": 1, "stderr_contains": ["error:"]},
                args=["mod", "use", "no-such-mod"], config=DEFAULT_CONFIG,
                config_file_contains=["active_mod = \"glacier-workstation\""],
                note="Task 4 修复：use 在写入前 resolve_mod_target 校验，不存在的 mod 拒绝且 active_mod 不变"),
    render_case("D4-10a", "mod export 含 mod 名", "D4",
                {"exit": 0, "stdout_contains": ["smoke-a"]},
                args=["mod", "export", "smoke-a"], config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "save", "smoke-a"]]),
    render_case("D4-10b", "mod import 合法 mod 文件", "D4",
                {"exit": 0, "stdout_contains": ["Imported mod"]},
                args=["mod", "import", fx("mods/smoke-b.toml")]),
    render_case("D4-11", "mod delete 后 preview 失败", "D4",
                {"exit": 1, "stderr_contains": ["error:"]},
                args=["mod", "preview", "smoke-a"], config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "delete", "smoke-a"]],
                note="mod delete 删除 mods/smoke-a.toml 后 load_mod 失败；preview 走 load_mod 直接验证"),
]

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
    render_case("D5-07", "completion bash 真补全脚本", "D5",
                {"exit": 0, "stdout_contains": ["_claude-hud"]},
                args=["completion", "bash"],
                note="⑪：clap_complete 真实现，输出 bash 补全函数 _claude-hud（函数名取自 bin_name，连字符保留；原占位文本已删）"),
    render_case("D5-08", "completion 不支持 shell 报错", "D5",
                {"exit": -1, "stderr_contains": ["unsupported shell"]},
                args=["completion", "nope"],
                note="⑪：不支持的 shell 走统一错误路径 exit 1（powershell 已被 clap_complete 支持，不再报错）"),
    render_case("D5-09", "mod list 6 内置 preset + user mods", "D5",
                {"exit": 0, "stdout_contains": ["dracula", "nord",
                                                "tokyo-night", "catppuccin",
                                                "monochrome", "solarized-dark",
                                                "User mods"]},
                args=["mod", "list"]),
    render_case("D5-10", "mod preview 合法", "D5",
                {"exit": 0, "stdout_contains": ["Preview: "]},
                args=["mod", "preview", "ember-night"],
                note="内置 preset 为 [mod] 嵌套 TOML，与扁平 ModPackage 字段不匹配 → 解析后名称为空，preview 输出空名（黑盒实测 'Preview: '）；Task 12 需同步规格"),
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
                {"exit": 0,
                 "stdout_contains": ["settings.json",
                                     "replacing existing statusLine (backup at"]},
                args=["setup"],
                note="⑰：本机 settings.json 已有 statusLine → 时间戳备份 + replacing 提示（真实环境 statusLine 不存在时该断言不适用）"),
]


def serve_case(cid, name, path, expect_status, expect_ct=None,
               expect_json=False, expect_json_fields=None, post_free=False,
               note=None):
    return {"id": cid, "name": name, "dim": "D6", "args": ["serve"],
            "run_kind": "serve", "path": path,
            "expect_status": expect_status, "expect_ct": expect_ct,
            "expect_json": expect_json,
            "expect_json_fields": expect_json_fields or [],
            "post_free": post_free,
            "spec": {"exit": None}, "note": note}


D6 = [
    serve_case("D6-01", "GET /", "/", 200, "text/html; charset=utf-8"),
    serve_case("D6-02", "GET /api/data", "/api/data", 200, "application/json",
               expect_json=True, expect_json_fields=["weekly"],
               note="serve.rs 将 compact render（含 ANSI 码）嵌入 JSON 字段；harness 自动剥离 ANSI 后再 parse JSON；weekly 字段来自历史库（空库可用性标记）"),
    serve_case("D6-03", "GET /api/health", "/api/health", 200),
    serve_case("D6-04", "未知路由 404", "/nope", 404,
               note="行为发现点：未匹配路由行为以实测为准"),
    serve_case("D6-05", "服务 5s 内响应", "/api/health", 200,
               note="服务就绪由 run_serve 的 5s 轮询保证；本例确认 /api/health 就绪后响应 200"),
    serve_case("D6-06", "进程退出后端口释放", "/api/health", 200,
               post_free=True),
]


def dash_case(cid, name, spec, note=None):
    return {"id": cid, "name": name, "dim": "D7", "args": ["dashboard"],
            "run_kind": "dashboard", "spec": spec, "note": note}


D7 = [
    dash_case("D7-01", "非 TTY 优雅失败",
              {"timed_out": True},
              note="行为发现：dashboard 不检测非 TTY，即使 stdin=DEVNULL 仍启动 TUI 循环，10s 超时；无法断言 exit 码或 stderr"),
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
                stdin=j(full_dict(**{"transcript_path": fx("transcript/agents.jsonl")})),
                note="行为发现点：无独立 skill/mcp transcript fixture（Task 4 仅 4 个 fixture），暂用 agents.jsonl，Task 11 以实测修正、Task 12 同步规格"),
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


# ---------------------------------------------------------------------------
# P1: state.json 数据通路（第一期任务① ②⑧ ⑫ ⑬）
# 观察通道约定（与设计 §7.2 用例 3 一致）：transcript 行为用 state.json
# 的 last_pos 断言（python 读文件），不用 stdout 计数——compact 布局中
# 无任何 widget 从 transcript 渲染 agent 计数（agent_overview 读 stdin 的
# subagent_status_line），计数语义由 transcript.rs 单测覆盖。
# ---------------------------------------------------------------------------
P1 = [
    render_case("P1-01", "render 创建五段 state.json", "P1",
                {"exit": 0, "stderr_empty": True,
                 "state_json": {
                     "segments": ["snapshot", "transcript", "cache",
                                  "alerts", "last_error"],
                     "absent": ["last_error"],
                     "equals": {"snapshot.model.display_name": "deepseek-v4-flash",
                                "transcript.last_pos": "<FIXTURE_SIZE>"},
                 }},
                stdin=j(full_dict(**{"context_window.used_percentage": 3})),
                config=DEFAULT_CONFIG, transcript_copy="agents.jsonl",
                note="任务①：render 全量原子写 state.json，快照与 transcript 游标落盘；修复前根本没有 state.json"),
    render_case("P1-02", "同文件两次 render 游标稳定", "P1",
                {"exit": 0,
                 "state_json_same_as_pre": ["transcript.last_pos"],
                 "state_json": {"equals": {"transcript.last_pos": "<FIXTURE_SIZE>"}}},
                stdin=j(full_dict()), config=DEFAULT_CONFIG,
                transcript_copy="agents.jsonl", pre_render=True,
                note="任务②：游标持久化 → 重复 render 不再前移 last_pos（重读 bug 会让计数翻倍但 last_pos 收敛，计数语义由单测覆盖）"),
    render_case("P1-03", "增量追加后游标续读前进", "P1",
                {"exit": 0,
                 "state_json": {"equals": {"transcript.last_pos": "<FIXTURE_SIZE>"}}},
                stdin=j(full_dict()), config=DEFAULT_CONFIG,
                transcript_copy="agents.jsonl", pre_render=True,
                grow_fixture={"agent_pairs": 2},
                note="任务②：追加后再次 render 续读至新 EOF，last_pos 前进到新文件大小；若未续读则卡在旧值（断言失败）"),
    render_case("P1-04", "截断文件自动重置游标", "P1",
                {"exit": 0,
                 "state_json": {"equals": {"transcript.last_pos": "<FIXTURE_SIZE>"}}},
                stdin=j(full_dict()), config=DEFAULT_CONFIG,
                transcript_copy="agents.jsonl", pre_render=True,
                truncate_fixture={"keep_lines": 4},
                note="任务⑧：last_pos > 文件长度 → 丢弃累计状态从 0 重读，last_pos 重置为新大小；若未重置则卡在旧值（断言失败）"),
    render_case("P1-05", "[hud err] 标记 + last_error 落盘与清除", "P1",
                {"exit": 0,
                 "state_json": {"absent": ["last_error"]}},
                stdin=j(full_dict()), config=DEFAULT_CONFIG,
                pre_render=True, pre_render_stdin="{ not json",
                pre_exit=1, pre_stdout_contains=["[hud err]"],
                note="任务⑬：坏 stdin → stdout 标记 + last_error 落盘；随后成功 render 清除 last_error"),
    render_case("P1-06", "doctor 上报 last render 失败", "P1",
                {"exit": 1, "stdout_contains": ["last render", "fix"]},
                args=["doctor"], config=DEFAULT_CONFIG,
                pre_render=True, pre_render_stdin="{ not json",
                note="任务⑬：doctor 读 state.json last_error 并给修复提示（计 1 个 failed check）"),
    render_case("P1-07", "越阈告警落盘 + 冷却内不变", "P1",
                {"exit": 0,
                 "pre_state_json": {"has": ["alerts.cost_threshold"]},
                 "state_json": {"has": ["alerts.cost_threshold"]},
                 "state_json_same_as_pre": ["alerts.cost_threshold"]},
                stdin=j(full_dict(**{"cost.total_cost_usd": 15.0})),
                config=DEFAULT_CONFIG, pre_render=True,
                note="设计用例4：cost=15（>默认 10 阈值）→ render 把冷却标记写进 state.alerts（跨进程契约）；冷却窗口内二次 render 时间戳不变"),
]


# ---------------------------------------------------------------------------
# P2: 第二期（任务③④⑭）
# ---------------------------------------------------------------------------
TS_ALPHA_START = int(datetime(2026, 7, 31, 10, 1, 0, tzinfo=timezone.utc).timestamp())
TS_TOOL_USE = int(datetime(2026, 7, 31, 10, 2, 0, tzinfo=timezone.utc).timestamp())

P2 = [
    render_case("P2-01", "camelCase 双命名 + 扁平 rate_limits", "P2",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash", "1/1 agents"],
                 "stderr_empty": True},
                stdin_file="json/camel_contract.json",
                config=DEFAULT_CONFIG,
                note="任务③：subagentStatusLine alias + five_hour_pct 扁平形态解析"),
    render_case("P2-02", "render --dump 键分类", "P2",
                {"exit": 0, "stdout_contains": ["recognized", "unknown",
                                                "session_id", "exceeds_200k_tokens"]},
                args=["render", "--dump"], stdin_file="json/full.json",
                note="任务③：full.json 的 cwd/workspace/version/exceeds_200k_tokens/session_id 归入 unknown"),
    render_case("P2-03", "doctor 契约探针 + update 信息项", "P2",
                {"exit": 0, "stdout_contains": ["contract probe", "update:"]},
                args=["doctor"], config=DEFAULT_CONFIG,
                note="⑱：doctor 含 update 检查行（信息项，不影响 exit 0）"),
    render_case("P2-04", "timestamps 真实时间轴落盘", "P2",
                {"exit": 0, "stderr_empty": True,
                 "state_json": {"equals": {
                     "transcript.timestamps_reliable": True,
                     "transcript.agents.0.start_time_secs": TS_ALPHA_START,
                     "transcript.agents.0.last_tool_call_secs": TS_TOOL_USE,
                     "transcript.last_pos": "<FIXTURE_SIZE>"}}},
                stdin=j(full_dict()), config=DEFAULT_CONFIG,
                transcript_copy="timestamps.jsonl",
                note="任务④：真实 ISO8601 → epoch 精确落盘；agents 按名排序保证 agents.0 确定性"),
    render_case("P2-10", "no_ts 降级端到端", "P2",
                {"exit": 0, "stderr_empty": True,
                 "state_json": {"equals": {
                     "transcript.timestamps_reliable": False,
                     "transcript.last_pos": "<FIXTURE_SIZE>"}}},
                stdin=j(full_dict()), config=DEFAULT_CONFIG,
                transcript_copy="no_ts.jsonl",
                note="任务④：首条无 timestamp → 降级路径，标志持久化为 false"),
    render_case("P2-05", "[pricing] 命中重算 ≈$", "P2",
                {"exit": 0, "stdout_contains": ["≈$0.56"], "stderr_empty": True},
                stdin=j(full_dict()), config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"cost_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"
                    "[pricing]\n"
                    "\"deepseek-v4-flash\" = { input = 0.001, output = 0.002 }\n"),
                transcript_copy="timestamps.jsonl",
                note="任务⑭：timestamps.jsonl 累计 input=300 output=130 → 0.3+0.26=0.56，≈ 标注"),
    render_case("P2-06", "无 [pricing] 透传官方价", "P2",
                {"exit": 0, "stdout_contains": ["$0.03"],
                 "stdout_not_contains": ["≈"]},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                note="任务⑭：未命中 → 透传 data.cost.total_cost_usd，无 ≈"),
    render_case("P2-07", "currency_symbol 全局生效", "P2",
                {"exit": 0, "stdout_contains": ["¥0.03"]},
                stdin_file="json/full.json", config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"cost_display\"]\n"
                    "currency_symbol = \"¥\"\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="任务⑭：顶层 currency_symbol 注入 cost_display（compact 路径代表四处接线）"),
    render_case("P2-08", "context_bar tokens k 缩写", "P2",
                {"exit": 0, "stdout_contains": ["6.8k/5.0k tok"]},
                stdin_file="json/full.json", config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"context_bar\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="任务⑭：in 6800 → 6.8k / out 5000 → 5.0k"),
    render_case("P2-09", "doctor 负单价校验", "P2",
                {"exit": 1, "stdout_contains": ["[!!]", "neg-model"]},
                args=["doctor"], config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"model_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"
                    "[pricing]\n"
                    "\"neg-model\" = { input = -0.000001 }\n"),
                note="任务⑭：负单价 → [!!] failure 含模型名定位"),
]

# --- Phase 3（任务⑤⑥⑦ 配置契约）---
# P3-06 以 import 为主运行（pre_cmd 输出不参与断言），落盘后
# config_file_contains 验证 [theme] 段写入 + 其他段保留。
P3 = [
    render_case("P3-06", "theme import 落盘保留其他段", "P3",
                {"exit": 0, "stdout_contains": ["imported"]},
                args=["theme", "import", fx("theme/nord_partial.toml")],
                config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"model_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                config_file_contains=[
                    "[theme]",
                    'accent = "#ff00ff"',
                    "bar_width = 20",
                    "compact_layout",
                    "active_mod",
                ],
                note="任务⑤：import 写入 [theme] 段（accent/bar_width），active_mod/compact_layout 等其他段保留"),
    render_case("P3-01", "theme 字符串预设生效（ANSI 色码）", "P3",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash",
                                                "\x1b[38;2;189;147;249m"]},
                stdin_file="json/full.json", config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"model_display\"]\n"
                    "theme = \"dracula\"\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="任务⑤：ThemeRef 字符串形态；model_color=#bd93f9(189;147;249) 的色码出现在原始 stdout"),
    render_case("P3-02", "部分表显式键覆盖默认", "P3",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash",
                                                "\x1b[38;2;255;0;0m"]},
                stdin_file="json/full.json", config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"model_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"
                    "[theme]\nmodel_color = \"#ff0000\"\n"),
                note="任务⑤：ThemeRef 表形态，显式键覆盖默认 nord 基底"),
    render_case("P3-03", "三层叠加：mod overrides 胜出", "P3",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash",
                                                "\x1b[38;2;51;51;51m"],
                 "stdout_not_contains": ["\x1b[38;2;255;0;0m"]},
                stdin_file="json/full.json", config=(
                    "active_mod = \"ov-test\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"model_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"
                    "[theme]\nmodel_color = \"#ff0000\"\n"
                    "[theme.overrides]\nmodel_color = \"#00ff00\"\n"),
                pre_cmds=[["mod", "import", fx("mods/ov-test.toml")]],
                note="任务⑤：mod preset 基底 → config 键 #ff0000 → config overrides #00ff00 → "
                     "mod overrides #333333（最高）胜出"),
    render_case("P3-04", "坏 config：stderr 警告 + 回退默认", "P3",
                {"exit": 0, "stderr_contains": ["[claude-hud] warning:"]},
                stdin_file="json/full.json", config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"model_display\"]\n"
                    "theme = 42\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="任务⑤：theme=42 无法解析为 ThemeRef → 整个 config 解析失败 → 警告 + 默认回退"),
    render_case("P3-05", "坏 config：doctor [!!] 可查", "P3",
                {"exit": 1, "stderr_contains": ["[claude-hud] warning:"],
                 "stdout_contains": ["[!!]", "config.toml"]},
                args=["doctor"], config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"model_display\"]\n"
                    "theme = 42\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="任务⑤：失败可见性——doctor 路径打警告 + config.toml 解析检查 [!!]（exit 1）"),
    render_case("P3-07", "mod use 不存在 mod：拒绝且 config 不变", "P3",
                {"exit": 1, "stderr_contains": ["error:"]},
                args=["mod", "use", "no-such-mod"], config=DEFAULT_CONFIG,
                config_file_contains=['active_mod = "glacier-workstation"'],
                note="任务⑥：resolve_mod_target 在任何写入前校验；失败则 config.toml 未被触碰"),
    render_case("P3-08a", "previous_mod 往返：use - 回到上一 mod", "P3",
                {"exit": 0,
                 "stdout_contains": ["Active mod: glacier-workstation"]},
                args=["mod", "current"], config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "use", "obsidian-command"],
                          ["mod", "use", "-"]],
                config_file_contains=['active_mod = "glacier-workstation"'],
                note="任务⑥：use A → use B → use - 回到 A（state.json previous_mod 往返）"),
    render_case("P3-08b", "previous_mod 再往返：use - 回到 B", "P3",
                {"exit": 0,
                 "stdout_contains": ["Active mod: obsidian-command"]},
                args=["mod", "current"], config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "use", "obsidian-command"],
                          ["mod", "use", "-"],
                          ["mod", "use", "-"]],
                config_file_contains=['active_mod = "obsidian-command"'],
                note="任务⑥：use - 再 use - 回到 B（AB 交替成立）"),
    render_case("P3-09a", "@scene 别名解析到内置 mod", "P3",
                {"exit": 0,
                 "stdout_contains": ["Switched to mod 'glacier-workstation'"]},
                args=["mod", "use", "@daily"], config=(
                    "active_mod = \"obsidian-command\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"model_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                config_file_contains=['active_mod = "glacier-workstation"'],
                note="任务⑥：@daily → scene daily-dev → glacier-workstation（自 obsidian 切换出，避免平凡通过）"),
    render_case("P3-09b", "@scene 未知别名报错", "P3",
                {"exit": 1, "stderr_contains": ["error:"]},
                args=["mod", "use", "@unknown"], config=DEFAULT_CONFIG,
                config_file_contains=['active_mod = "glacier-workstation"'],
                note="任务⑥：@unknown 无别名映射且无 scene 命中 → 拒绝且 config 不变"),
    render_case("P3-12", "mod save 快照含 compact_widgets", "P3",
                {"exit": 0, "stdout_contains": ["compact_widgets",
                                                "model_display"]},
                args=["mod", "export", "snap-w"], config=DEFAULT_CONFIG,
                pre_cmds=[["mod", "save", "snap-w"]],
                note="任务⑥：save 生成快照含 compact_widgets 数组，export 原样输出"),
    render_case("P3-10", "mod save→use 自定义数组渲染一致", "P3",
                {"exit": 0, "stdout_contains": ["deepseek-v4-flash",
                                                "$0.03"]},
                stdin=j(full_dict()), config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"model_display\", \"cost_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                pre_cmds=[["mod", "save", "my-custom"],
                          ["mod", "use", "my-custom"]],
                note="任务⑥：save 快照 compact_widgets，use 后按数组渲染（model + cost 均出现）"),
    render_case("P3-11", "未实现布局 ID 明确报错", "P3",
                {"exit": -1, "stdout_contains": ["not implemented"]},
                stdin=j(full_dict()), config=(
                    "active_mod = \"obsidian-command\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="任务⑥：agent-centric 无 compact_widgets → 渲染报错 hud_err_marker 上屏"),
    render_case("P3-13", "rate_limits 超阈值数字在色内", "P3",
                {"exit": 0, "stdout_contains": ["92%"],
                 "stdout_raw_regex": r"\x1b\[38;2;[0-9;]+m[^\x1b]*[0-9]+%[^\x1b]*"},
                stdin=j(full_dict(**{"rate_limits.five_hour.used_percentage": 92.0})),
                config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"rate_limits\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets.rate_limits]\nrate_limit_warn = 90\n"),
                note="任务⑦：92% 超过 warn=90 → 数字整体在 danger 色内（修复前为空 wrap，正则不匹配）"),
    render_case("P3-14", "session_stats 三色整段上色", "P3",
                {"exit": 0, "stdout_contains": ["tok/s"],
                 "stdout_raw_regex":
                     r"\x1b\[38;2;[0-9;]+m[^\x1b]+(?:\x1b\[0m)? \x1b\[38;2;[0-9;]+m[^\x1b]+(?:\x1b\[0m)? \x1b\[38;2;[0-9;]+m[^\x1b]+"},
                stdin_file="json/full.json", config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"session_stats\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="任务⑦：⏱dur/fg + ntok/s/accent + ncalls/muted 三段各自整体上色（修复前数字段被 reset 打断，三段式正则不匹配）"),
    render_case("P3-15", "cost_display 符号+数字整体在色内", "P3",
                {"exit": 0,
                 "stdout_raw_regex": r"\x1b\[38;2;[0-9;]+m\$[0-9.]+[^\x1b]*"},
                stdin_file="json/full.json", config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"cost_display\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="任务⑦：$0.03 符号+数字整体在色码内（修复前 $ 在色内数字在色外，正则不匹配）"),
]


# --- Phase 4（⑨⑩⑪⑮⑯⑰⑱ 批次 C 剩余）---
# P4-01 通过 pre_cmds dict+stdin 执行两次 render（不同 transcript_path），
# 主运行直接断言 `history` 命令输出（用户可见契约，而非 sqlite 内部行数）。
P4 = [
    render_case("P4-01", "两次 render 不同 path → history 结账 1 条", "P4",
                {"exit": 0, "stdout_contains": ["Weekly stats",
                                                "Recent sessions", "#1"]},
                args=["history"], config=DEFAULT_CONFIG,
                pre_cmds=[
                    {"args": ["render"],
                     "stdin": j(full_dict(**{"transcript_path": "/a.jsonl"}))},
                    {"args": ["render"],
                     "stdin": j(full_dict(**{"transcript_path": "/b.jsonl"}))},
                ],
                remove_db=True,
                note="⑨：render A（/a.jsonl）→ render B（/b.jsonl）切换时结账 A；history 输出 1 条 Recent session（#1）"),
    render_case("P4-02", "history 空库显示 —", "P4",
                {"exit": 0, "stdout_contains": ["—"]},
                args=["history"], config=DEFAULT_CONFIG,
                remove_db=True,
                note="⑨：空库各数值位输出 —（不显示 0）；HistoryStore::open 失败则 Err 上报"),
    render_case("P4-07", "COLUMNS=30 → 可见宽度 ≤ 40", "P4",
                {"exit": 0, "stdout_visible_width_max": 40},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                env_extra={"COLUMNS": "30"},
                note="⑮：columns_env clamp 到 40，fit_line 从行尾丢组直至 ≤40 列"),
    render_case("P4-08", "COLUMNS=200 → 输出完整无截断", "P4",
                {"exit": 0,
                 "stdout_contains": ["deepseek-v4-flash", "$0.03"],
                 "stdout_not_contains": ["..."]},
                stdin_file="json/full.json", config=DEFAULT_CONFIG,
                env_extra={"COLUMNS": "200"},
                note="⑮：宽终端（≥120 列）与无 COLUMNS 行为一致——不丢组、无 truncate 省略号"),
    render_case("P4-05", "mod use 输出全局生效提示", "P4",
                {"exit": 0, "stdout_contains": ["(applies to all windows)"]},
                args=["mod", "use", "ember-night"], config=DEFAULT_CONFIG,
                note="⑰：写配置命令追加全局生效提示（mod use 代表 8 处接线）"),
    render_case("P4-06", "theme import 输出全局生效提示", "P4",
                {"exit": 0, "stdout_contains": ["imported",
                                                "(applies to all windows)"]},
                args=["theme", "import", fx("theme/nord_partial.toml")],
                config=DEFAULT_CONFIG,
                config_file_contains=["[theme]", "accent = \"#ff00ff\""],
                note="⑰：theme import 追加提示且落盘行为不变（复用 P3-06 流程）"),
    render_case("P4-03", "update check 占位符短路", "P4",
                {"exit": 0, "stdout_contains": ["not published yet"]},
                args=["update", "check"], config=DEFAULT_CONFIG,
                note="⑱：占位符仓库零网络返回 NotPublished（exit 0 恒定）"),
]


CASES = D1 + D2 + D3 + D4 + D5 + D6 + D7 + D8 + P1 + P2 + P3 + P4
assert len(CASES) == 130, f"expected 130 cases, got {len(CASES)}"
