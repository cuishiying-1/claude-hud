"""Process execution for the claude-hud black-box test harness."""
import os
import shutil
import subprocess
import tempfile
import time


# 测试 HUD 目录 = 每次运行全新临时目录（CLAUDE_HUD_DIR 注入子进程）。
# 绝不指向真实 ~/.claude/plugins/claude-hud：用户活跃会话的 render 进程
# 每 5s 并发写同一目录，会把真实会话结账进测试 history.db（B6-02 偶发
# #3 幽灵会话根因）。隔离后备份/恢复协议不再需要。
TEST_HUD_DIR = os.path.join(tempfile.gettempdir(), "claude-hud-test-run", "hud")
HUD_DIR = TEST_HUD_DIR


def ensure_test_hud_dir() -> str:
    """Reset the isolated HUD dir for this run; return its path."""
    if os.path.isdir(TEST_HUD_DIR):
        shutil.rmtree(TEST_HUD_DIR)
    os.makedirs(TEST_HUD_DIR)
    return TEST_HUD_DIR


class RunResult:
    def __init__(self, exit_code, stdout, stderr, timed_out, duration_s, repro):
        self.exit_code = exit_code
        self.stdout = stdout
        self.stderr = stderr
        self.timed_out = timed_out
        self.duration_s = duration_s
        self.repro = repro


def run_exe(exe_path, args, stdin_text=None, stdin_file=None,
            timeout_s=10, env_extra=None, env=None):
    """Run the exe. stdin provided as inline text or a file path.
    Returns RunResult. Never raises on child failure.
    env: full child environment (overrides os.environ); env_extra: partial
    overrides merged last. CLAUDE_HUD_DIR is always injected (test isolation)."""
    if stdin_file:
        stdin_src = open(stdin_file, "rb")
    elif stdin_text is not None:
        stdin_src = None
    else:
        stdin_src = subprocess.DEVNULL
    env = dict(env) if env is not None else dict(os.environ)
    env["CLAUDE_HUD_DIR"] = TEST_HUD_DIR
    if env_extra:
        env.update(env_extra)
    start = time.monotonic()
    timed_out = False
    try:
        kwargs = {}
        if stdin_file:
            kwargs["stdin"] = stdin_src
        elif stdin_text is None:
            kwargs["stdin"] = subprocess.DEVNULL
        proc = subprocess.run(
            [exe_path] + args,
            input=stdin_text.encode("utf-8") if stdin_text is not None else None,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            timeout=timeout_s,
            **kwargs,
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


def write_config(toml_text: str | None):
    """Write a test config to HUD_DIR/config.toml (None = leave as-is)."""
    if toml_text is None:
        return
    os.makedirs(HUD_DIR, exist_ok=True)
    with open(os.path.join(HUD_DIR, "config.toml"), "w", encoding="utf-8") as f:
        f.write(toml_text)


def prepare_config_path(case):
    """P10 注入：CLAUDE_HUD_CONFIG 指向 temp config（不污染真实配置）。

    config_path 未指定时用共享固定路径 hud-cfg-p10.toml —— P10-03 POST
    写入后 P10-04 GET 可读到新值（跨用例共享 = 测磁盘权威语义）。
    config_content 提供时每次重写（确定性），否则保留磁盘现状。
    """
    path = case.get("config_path")
    if not path:
        path = os.path.join(tempfile.gettempdir(), "hud-cfg-p10.toml")
        case["config_path"] = path
    if case.get("config_content"):
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            f.write(case["config_content"])
    return path
