"""Environment resolution and snapshot for the claude-hud test harness."""
import datetime
import os
import platform
import sys


_REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
)


def _default_exe() -> str:
    """Prefer the repo's freshly built artifact over the installed binary."""
    for rel in (
        "target/debug/claude-hud.exe",
        "target/release/claude-hud.exe",
    ):
        candidate = os.path.join(_REPO_ROOT, rel)
        if os.path.isfile(candidate):
            return candidate
    return os.path.expanduser("~/.cargo/bin/claude-hud.exe")


DEFAULT_EXE = _default_exe()


def resolve_exe(override: str | None) -> str:
    """Return the claude-hud exe path, validating it exists."""
    # abspath: Windows CreateProcess 无法解析前斜杠相对路径
    # （isfile 可以，导致 FileNotFoundError [WinError 2]）
    path = os.path.abspath(override or DEFAULT_EXE)
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
