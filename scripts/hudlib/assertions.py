"""Unified assertion engine. Spec keys:
- exit: int            (exact exit code; -1 means 'any non-zero')
- stdout_contains: list[str]     (all must appear)
- stdout_regex: str              (re.search)
- stdout_raw_regex: str|list     (re.search on raw stdout incl. ANSI)
- stdout_not_contains: list[str]
- stdout_empty: bool             (stdout must be exactly empty)
- stdout_visible_width_max: int (max visible width of any output line)
- stderr_contains: list[str]
- stderr_empty: bool
- timed_out: False               (must not have timed out)
Returns (passed: bool, detail: str).
"""
import re
import unicodedata

_ANSI_RE = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")


def _strip_ansi(s: str) -> str:
    """Strip CSI color codes so assertions match visible text, not raw bytes."""
    return _ANSI_RE.sub("", s)


def _visible_width(s: str) -> int:
    """Visible column width: ANSI stripped; CJK wide/fullwidth chars count 2."""
    w = 0
    for ch in _strip_ansi(s):
        w += 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1
    return w


def check(result, spec: dict) -> tuple[bool, str]:
    fails = []
    out = _strip_ansi(result.stdout)
    err = _strip_ansi(result.stderr)
    if "exit" in spec:
        want = spec["exit"]
        if want == -1:
            if result.exit_code == 0:
                fails.append("exit: expected non-zero, got 0")
            elif result.timed_out:
                fails.append("exit: timed out, not a valid non-zero exit")
        elif result.exit_code != want:
            fails.append(f"exit: expected {want}, got {result.exit_code}")
    if spec.get("timed_out") is False and result.timed_out:
        fails.append(f"timed out after {result.duration_s:.1f}s")
    for s in spec.get("stdout_contains", []):
        hay = result.stdout if s.startswith("\x1b[") else out
        if s not in hay:
            fails.append(f"stdout missing: {s!r}")
    if "stdout_regex" in spec:
        if not re.search(spec["stdout_regex"], out):
            fails.append(f"stdout regex no match: {spec['stdout_regex']!r}")
    for s in spec.get("stdout_not_contains", []):
        hay = result.stdout if s.startswith("\x1b[") else out
        if s in hay:
            fails.append(f"stdout unexpectedly contains: {s!r}")
    # raw stdout（含 ANSI）上的正则；用于断言"着色文本整体包进色码"
    raw_pats = spec.get("stdout_raw_regex") or []
    if isinstance(raw_pats, str):
        raw_pats = [raw_pats]
    for pat in raw_pats:
        if not re.search(pat, result.stdout):
            fails.append(f"stdout raw regex no match: {pat!r}")
    if spec.get("stdout_empty") and out:
        fails.append(f"stdout not empty: {out[:120]!r}")
    for s in spec.get("stderr_contains", []):
        if s not in err:
            fails.append(f"stderr missing: {s!r}")
    if spec.get("stderr_empty") and err.strip():
        fails.append(f"stderr not empty: {err.strip()[:120]!r}")
    if "stdout_visible_width_max" in spec:
        max_w = max((_visible_width(l) for l in out.splitlines()), default=0)
        if max_w > spec["stdout_visible_width_max"]:
            fails.append(
                f"stdout visible width {max_w} > {spec['stdout_visible_width_max']}"
            )
    if fails:
        return False, "; ".join(fails)
    return True, "ok"


_MISSING = object()


def _dig(node, path):
    """Dig a dot path; _MISSING when any segment is absent.
    Integer segments index into lists (e.g. transcript.agents.0.name)."""
    for part in path.split("."):
        if part.isdigit() and isinstance(node, list):
            i = int(part)
            if i < len(node):
                node = node[i]
                continue
            return _MISSING
        if isinstance(node, dict) and part in node:
            node = node[part]
        else:
            return _MISSING
    return node


def check_state_json(spec: dict, state: dict) -> list[str]:
    """Evaluate a state_json spec against a parsed state dict.
    Spec keys: exists (default True), segments (all must be present),
    absent (must be missing or null), has (must be present),
    min (dot-path -> numeric floor), equals (dot-path -> exact value).
    Returns failure strings (empty = pass).
    """
    fails = []
    if not spec.get("exists", True):
        if state:
            fails.append("state file present but expected absent")
        return fails
    if not state:
        fails.append("state file missing or unparseable")
        return fails
    for seg in spec.get("segments", []):
        if seg not in state:
            fails.append(f"state segment missing: {seg}")
    for key in spec.get("absent", []):
        if _dig(state, key) not in (_MISSING, None):
            fails.append(f"state key present but expected absent: {key}")
    for key in spec.get("has", []):
        if _dig(state, key) is _MISSING:
            fails.append(f"state key missing: {key}")
    for key, want in (spec.get("min") or {}).items():
        got = _dig(state, key)
        if not isinstance(got, (int, float)) or got < want:
            fails.append(f"state.{key}: expected >= {want}, got {got!r}")
    for key, want in (spec.get("equals") or {}).items():
        got = _dig(state, key)
        if got != want:
            fails.append(f"state.{key}: expected {want!r}, got {got!r}")
    return fails
