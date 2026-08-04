"""Process execution and config backup/restore protocol."""
import filecmp
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
        stdin_src = None
    else:
        stdin_src = subprocess.DEVNULL
    env = dict(os.environ)
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
    prev = os.path.join(backup_root, "hud")
    if os.path.isdir(prev):
        shutil.rmtree(prev)
    if os.path.isdir(HUD_DIR):
        shutil.copytree(HUD_DIR, prev, dirs_exist_ok=True)
    open(marker, "w").write("active")
    return backup_root


def restore_hud_dir(backup_root: str) -> bool:
    """Restore the HUD dir from backup. Returns True on verified success."""
    marker = os.path.join(backup_root, BACKUP_MARKER)
    ok = True
    try:
        src = os.path.join(backup_root, "hud")
        if os.path.isdir(src):
            if os.path.isdir(HUD_DIR):
                shutil.rmtree(HUD_DIR)
            shutil.copytree(src, HUD_DIR)
            for root, _, files in os.walk(src):
                for f in files:
                    a = os.path.join(root, f)
                    b = a.replace(src, HUD_DIR)
                    if not os.path.exists(b) or not filecmp.cmp(a, b, shallow=False):
                        ok = False
        elif os.path.isdir(HUD_DIR):
            shutil.rmtree(HUD_DIR)  # dir did not exist before test run
    except Exception:
        return False
    # Fix 5: marker-removal failure must not be silent.  If the byte-verify
    # passed but os.remove(marker) fails (e.g. permission error), the marker
    # would survive and block the next run.  Return False so the caller knows
    # the marker is intentionally left in place (safe crash-recovery state).
    if ok and os.path.exists(marker):
        try:
            os.remove(marker)
        except OSError:
            ok = False
    return ok


def write_config(toml_text: str | None):
    """Write a test config to HUD_DIR/config.toml (None = leave as-is)."""
    if toml_text is None:
        return
    os.makedirs(HUD_DIR, exist_ok=True)
    with open(os.path.join(HUD_DIR, "config.toml"), "w", encoding="utf-8") as f:
        f.write(toml_text)
