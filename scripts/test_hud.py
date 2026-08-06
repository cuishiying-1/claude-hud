"""claude-hud black-box test suite entry point.

Usage:
  python scripts/test_hud.py                 # full suite
  python scripts/test_hud.py --case D1-01    # single case
  python scripts/test_hud.py --exe <path>    # override exe path
  python scripts/test_hud.py --report <path> # override report output
"""
import argparse
import http.client
import json as _json
import os
import re
import shutil
import subprocess
import sys
import time

# Windows GBK console can't print ¥ etc.; force UTF-8 so report output survives.
for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from hudlib import assertions, cases, env, report, runner  # noqa: E402


# ---------------------------------------------------------------------------
# Fix 1: shared result-dict builder — single source of truth for the 13-key
# result dict consumed by report.render_report.
# ---------------------------------------------------------------------------
def _build_result(case, passed, detail, exit_code, timed_out, stdout,
                  stderr, repro, duration_s):
    """Return a uniform 13-key result dict."""
    return {
        "id": case["id"], "dim": case["dim"], "name": case["name"],
        "passed": passed, "detail": detail, "spec": case["spec"],
        "exit_code": exit_code, "timed_out": timed_out,
        "stdout": stdout, "stderr": stderr, "repro": repro,
        "duration_s": duration_s, "note": case.get("note"),
    }


def _prepare_db(exe_path, sqls):
    """⑪⑫⑬⑭ 预置 history.db：先跑一次 sessions 触发 init_schema 建表，
    再按序执行 SQL。SQLite 异常只告警不中断（避免污染全套件）。"""
    import sqlite3
    runner.run_exe(exe_path, ["sessions"], timeout_s=10)
    db_path = os.path.join(runner.HUD_DIR, "history.db")
    conn = sqlite3.connect(db_path)
    try:
        for sql in sqls:
            conn.execute(sql)
        conn.commit()
    except sqlite3.Error as e:
        print(f"  [WARN] prepare_db_sql failed: {e}")
    finally:
        conn.close()


def prepare_case(case, tmp_dir):
    """Return stdin text for render cases (None when no stdin).

    transcript_copy: copies fixtures/transcript/<name> once into tmp_dir and
    rewrites the stdin JSON's transcript_path to the copy, so P1 cases can
    grow/truncate their transcript without mutating the shared fixture.
    """
    if case.get("run_kind", "render") != "render" and not case.get("pre_render"):
        return None
    if case.get("transcript_copy") and not case.get("_transcript_copy_path"):
        src = cases.fx(os.path.join("transcript", case["transcript_copy"]))
        dst = os.path.join(tmp_dir, f"{case['id']}-transcript.jsonl")
        shutil.copyfile(src, dst)
        case["_transcript_copy_path"] = dst
    if case.get("stdin") is not None:
        text = case["stdin"]
    elif case.get("stdin_file"):
        with open(cases.fx(case["stdin_file"]), encoding="utf-8") as f:
            text = f.read()
    else:
        return None
    if case.get("_transcript_copy_path"):
        data = _json.loads(text)
        data["transcript_path"] = case["_transcript_copy_path"].replace("\\", "/")
        text = _json.dumps(data)
    if "<LARGE_FIXTURE>" in text:
        text = text.replace("<LARGE_FIXTURE>",
                            cases.prepare_large_transcript(tmp_dir))
    return text


def run_serve(exe_path, case):
    """Start serve, poll endpoint, assert, terminate, verify port release.

    Amendments A1 + A2 applied:
      - A1: stdin=DEVNULL to avoid read_current_data hang on /api/data
      - A2: post_free check runs AFTER terminate, BEFORE result dict built
    """
    start = time.monotonic()
    fails = []

    # P10 ⑥：仅显式声明 config_path/config_content 的用例注入 CLAUDE_HUD_CONFIG
    # 到 temp 路径（POST 保存不碰真实配置）；其余 serve 用例保持加载真实配置。
    uses_cfg = bool(case.get("config_path") or case.get("config_content"))
    env = dict(os.environ)
    cfg_path = runner.prepare_config_path(case)
    if uses_cfg:
        env["CLAUDE_HUD_CONFIG"] = cfg_path

    # A1: stdin=DEVNULL — serve's /api/data handler reads stdin to EOF;
    # inherited stdin from the python parent would hang the single-threaded
    # server forever.
    proc = subprocess.Popen(
        [exe_path, "serve"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        stdin=subprocess.DEVNULL,
        env=env,
    )
    status = None
    ct = ""
    body = ""
    try:
        deadline = start + 5.0
        while time.monotonic() < deadline:
            if proc.poll() is not None:
                break
            try:
                conn = http.client.HTTPConnection(
                    "127.0.0.1", 9527, timeout=1
                )
                conn.request(
                    case.get("method", "GET"), case["path"],
                    body=case.get("body"),
                )
                resp = conn.getresponse()
                status = resp.status
                ct = resp.getheader("Content-Type", "")
                body = resp.read().decode("utf-8", "replace")
                if case.get("expect_backup"):
                    if not (cfg_path and os.path.exists(cfg_path + ".bak")):
                        fails.append(f"backup {cfg_path}.bak 未生成")
                conn.close()
                break
            except OSError:
                time.sleep(0.2)

        if status is None:
            fails.append(
                f"serve 5s 内未响应（进程退出码 {proc.poll()}）"
            )
        else:
            if status != case["expect_status"]:
                fails.append(
                    f"status: expected {case['expect_status']}, got {status}"
                )
            if case.get("expect_ct") and case["expect_ct"] not in ct:
                fails.append(
                    f"Content-Type: expected {case['expect_ct']}, got {ct}"
                )
            if case.get("expect_json"):
                try:
                    # Strip ANSI escape sequences before parsing JSON;
                    # src/serve.rs embeds compact-render output (which
                    # contains ANSI codes) into JSON string fields.
                    clean = re.sub(r"\x1b\[[0-9;]*m", "", body)
                    parsed = _json.loads(clean)
                except ValueError as e:
                    fails.append(f"body not JSON: {e}")
                else:
                    for field in case.get("expect_json_fields", []):
                        if field not in parsed:
                            fails.append(f"JSON missing field: {field}")
            for want in case.get("expect_body_contains", []):
                if want not in body:
                    fails.append(f"body missing {want!r}")
            for want in case.get("expect_body_not_contains", []):
                if want in body:
                    fails.append(f"body should not contain {want!r}")
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=3)

    # A2: post_free check runs AFTER terminate (port must be released),
    # BEFORE the result dict is built so the verdict lands in passed/detail.
    if case.get("post_free"):
        free = False
        t0 = time.monotonic()
        while time.monotonic() - t0 < 3.0:
            try:
                conn = http.client.HTTPConnection(
                    "127.0.0.1", 9527, timeout=1
                )
                conn.request("GET", "/")
                conn.getresponse()
                conn.close()
                time.sleep(0.2)
            except OSError:
                free = True
                break
        if not free:
            fails.append("port 9527 在进程退出后仍可连接")

    passed = not fails
    # Fix 4: dim now comes from case["dim"] (was hardcoded "D6")
    return _build_result(
        case, passed,
        "; ".join(fails) if fails else "ok",
        proc.poll() if proc.poll() is not None else 0,
        False, body, "",
        f"{exe_path} serve  (GET {case['path']})",
        time.monotonic() - start,
    )


def run_config(exe_path, case):
    """config TUI 非 TTY：单帧渲染后退出（与 dashboard 单帧先例一致）。"""
    result = runner.run_exe(exe_path, case["args"], timeout_s=10)
    fails = []
    out = result.stdout
    if result.exit_code != 0:
        fails.append(f"exit: expected 0, got {result.exit_code}")
    for want in case.get("expect_body_contains", []):
        if want not in out:
            fails.append(f"stdout missing {want!r}")
    passed = not fails
    detail = "; ".join(fails) if fails else "single-frame render ok"
    return {
        "id": case["id"], "dim": case["dim"], "name": case["name"],
        "passed": passed, "detail": detail, "spec": case["spec"],
        "exit_code": result.exit_code, "timed_out": result.timed_out,
        "stdout": out, "stderr": result.stderr, "repro": result.repro,
        "duration_s": result.duration_s, "note": case.get("note"),
    }


def run_one(exe_path, case, tmp_dir):
    """Run one case; return a result dict consumed by report.render_report.

    Amendment A5: serve/dashboard cases with no explicit config write
    DEFAULT_CONFIG for determinism (including standalone --case runs).
    Render cases inherit on-disk config (designed against suite flow).
    """
    start = time.monotonic()

    # A5: config determinism — write before any pre_cmds or exe dispatch.
    # Order matters: pre_cmds (e.g. mod use) write active_mod into
    # config.toml, which must not be clobbered afterwards.
    if case.get("config") is not None:
        runner.write_config(case["config"])
    elif case["run_kind"] in ("serve", "dashboard"):
        runner.write_config(cases.DEFAULT_CONFIG)

    # P4 ⑨：可选清空 history.db（必须在任何 checkout 渲染之前）。
    # 连同 -wal/-shm/-journal 兄弟文件一起删：stale journal 会把旧行
    # 恢复到新库，导致 session 编号偏移（B6-02 偶发失败根因）。
    if case.get("remove_db"):
        db_path = os.path.join(runner.HUD_DIR, "history.db")
        for p in [db_path] + [db_path + s for s in ("-wal", "-shm", "-journal")]:
            if os.path.isfile(p):
                os.remove(p)

    # ⑨+：可选清空 state.json —— checkout_billed 去重表跨进程持久，
    # 上一用例残留会挡本用例结账（必须在任何 checkout 渲染之前）。
    if case.get("remove_state"):
        state_path = os.path.join(runner.HUD_DIR, "state.json")
        if os.path.isfile(state_path):
            os.remove(state_path)

    # ⑪⑫⑬⑭：可选预置历史库数据（依赖 remove_db 已清空 + 建表在前）
    if case.get("prepare_db_sql"):
        _prepare_db(exe_path, case["prepare_db_sql"])

    # Fix 2: pre_cmd failures must not be silent — collect warnings and
    # surface them in the final detail.  A case may still PASS if its main
    # assertions hold; the warning is informational.
    pre_warnings = []
    for pre in case.get("pre_cmds", []):
        if isinstance(pre, dict):
            r = runner.run_exe(exe_path, pre["args"],
                               stdin_text=pre.get("stdin"),
                               env_extra=case.get("env_extra"),
                               timeout_s=10)
        else:
            r = runner.run_exe(exe_path, pre,
                               env_extra=case.get("env_extra"),
                               timeout_s=10)
        if r.exit_code != 0 or r.timed_out:
            pre_warnings.append(f"pre_cmd exit={r.exit_code}: {pre!r}")
            print(
                f"  [WARN] pre_cmd failed (exit={r.exit_code}): {pre!r}"
            )

    # P1 机制 1：pre_render（默认复用主 stdin，可覆盖）；断言失败即判负
    pre_fails = []
    if case.get("pre_render"):
        pre_text = case.get("pre_render_stdin")
        if pre_text is None:
            pre_text = prepare_case(case, tmp_dir)
        else:
            pre_text = prepare_case({"stdin": pre_text}, tmp_dir)
        r = runner.run_exe(exe_path, ["render"], stdin_text=pre_text,
                           env_extra=case.get("env_extra"),
                           timeout_s=10)
        if case.get("pre_exit") is not None and r.exit_code != case["pre_exit"]:
            pre_fails.append(
                f"pre_render exit={r.exit_code}, expected {case['pre_exit']}"
            )
        for want in case.get("pre_stdout_contains", []):
            if want not in r.stdout:
                pre_fails.append(f"pre_render stdout missing {want!r}")
        case["_pre_state"] = _read_state_json()

    # P1 机制 2：fixture 增删（只在有 per-case transcript 副本时生效）
    _apply_fixture_ops(case, tmp_dir)

    # P1 机制 3：可选清空 state.json（模拟全新状态）
    if case.get("remove_state"):
        sp = os.path.join(runner.HUD_DIR, "state.json")
        if os.path.isfile(sp):
            os.remove(sp)

    if case["run_kind"] == "serve":
        result = run_serve(exe_path, case)
        if pre_warnings:
            result["detail"] = (
                "; ".join(pre_warnings) + "; " + result["detail"]
            )
        return result

    if case["run_kind"] == "config":
        result = run_config(exe_path, case)
        if pre_warnings:
            result["detail"] = "; ".join(pre_warnings) + "; " + result["detail"]
        return result

    if case["run_kind"] == "dashboard":
        r = runner.run_exe(exe_path, case["args"], timeout_s=10)
    else:
        stdin_text = prepare_case(case, tmp_dir)
        r = runner.run_exe(exe_path, case["args"], stdin_text=stdin_text,
                           env_extra=case.get("env_extra"),
                           timeout_s=10)

    passed, detail = assertions.check(r, case["spec"])
    state_fails = []
    if case.get("state_json"):
        state_fails = assertions.check_state_json(
            case["state_json"], _read_state_json()
        )
    if case.get("pre_state_json"):
        state_fails += assertions.check_state_json(
            case["pre_state_json"], case.get("_pre_state", {})
        )
    # P3 ⑥：命令落盘后对 config.toml 的文件级断言（import 保留其他段）
    if case.get("config_file_contains"):
        cfg_path = os.path.join(runner.HUD_DIR, "config.toml")
        try:
            with open(cfg_path, encoding="utf-8") as f:
                cfg_text = f.read()
        except OSError:
            cfg_text = ""
        for s in case["config_file_contains"]:
            if s not in cfg_text:
                state_fails.append(f"config.toml missing: {s!r}")
    for dot in case.get("state_json_same_as_pre", []):
        cur = _dig_state(_read_state_json(), dot)
        pre = _dig_state(case.get("_pre_state", {}), dot)
        if cur != pre:
            state_fails.append(f"state.{dot}: pre={pre!r} now={cur!r}")
    extra = pre_fails + state_fails
    if extra:
        passed = False
        detail = ("; ".join(extra) + "; " + detail) if detail != "ok" else "; ".join(extra)
    if pre_warnings:
        detail = "; ".join(pre_warnings) + "; " + detail
    return _build_result(
        case, passed, detail, r.exit_code, r.timed_out,
        r.stdout, r.stderr, r.repro,
        time.monotonic() - start,
    )


def _read_state_json() -> dict:
    """Current HUD_DIR/state.json as dict ({} when missing/unparseable)."""
    path = os.path.join(runner.HUD_DIR, "state.json")
    try:
        with open(path, encoding="utf-8") as f:
            return _json.load(f)
    except (OSError, ValueError):
        return {}


def _dig_state(state: dict, dot: str):
    """Dig a dot path in the state dict; None when missing."""
    node = state
    for part in dot.split("."):
        if isinstance(node, dict) and part in node:
            node = node[part]
        else:
            return None
    return node


def _apply_fixture_ops(case, tmp_dir):
    """grow (append agent pairs) / truncate the per-case transcript copy,
    then substitute <FIXTURE_SIZE> in the state_json spec with the size
    after the ops (the expected last_pos for the next render)."""
    if not case.get("_transcript_copy_path") and case.get("transcript_copy"):
        # 无 pre_render 的用例副本尚未就绪（如 P1-01）——先复制再操作
        prepare_case(case, tmp_dir)
    path = case.get("_transcript_copy_path")
    if not path:
        return
    pairs = (case.get("grow_fixture") or {}).get("agent_pairs", 0)
    if pairs:
        with open(path, "a", encoding="utf-8") as f:
            for i in range(pairs):
                f.write(f'{{"type":"subagent_start","name":"extra-{i}","model":"m","task":"t"}}\n')
                f.write(f'{{"type":"subagent_stop","name":"extra-{i}"}}\n')
    keep = (case.get("truncate_fixture") or {}).get("keep_lines")
    if keep is not None:
        with open(path, encoding="utf-8") as f:
            lines = f.readlines()
        with open(path, "w", encoding="utf-8") as f:
            f.writelines(lines[:keep])
    if case.get("state_json"):
        size = os.path.getsize(path)
        for sect in ("equals", "min"):
            for key, val in (case["state_json"].get(sect) or {}).items():
                if val == "<FIXTURE_SIZE>":
                    case["state_json"][sect][key] = size


# ---------------------------------------------------------------------------
# Fix 6: split monolithic main into focused helpers (50-line rule).
# ---------------------------------------------------------------------------

def _select_cases(selected_id, all_cases):
    """Return cases matching selected_id, or all cases if selected_id is None.
    Raises SystemExit(2) with message when selected_id is given but not found.
    """
    selected = [
        c for c in all_cases if not selected_id or c["id"] == selected_id
    ]
    if selected_id and not selected:
        print(f"case {selected_id} not found")
        sys.exit(2)
    return selected


def _backup_protocol(exe_path, snap):
    """Backup HUD dir; return backup_root.  Exits on stale marker."""
    try:
        backup_root = runner.backup_hud_dir()
    except RuntimeError as e:
        print(f"[hud-test] ERROR: {e}")
        sys.exit(1)
    return backup_root


def _run_suite(exe_path, snap, selected, backup_root):
    """Run selected cases with settings protection and tmp lifecycle.

    Fix 3: settings backup and tmp_dir creation live inside the try so a
    failure there still triggers hud restore + settings restore + tmp cleanup.
    """
    settings_path = os.path.expanduser("~/.claude/settings.json")
    had_settings = os.path.isfile(settings_path)
    _settings_bak = None
    results = []
    restored = False
    tmp_dir = os.path.join(backup_root, "tmp")
    try:
        # A4: settings.json protection — backup INSIDE try per Fix 3
        if had_settings:
            with open(settings_path, "rb") as f:
                _settings_bak = f.read()
            with open(
                os.path.join(backup_root, "settings.json.bak"), "wb"
            ) as f:
                f.write(_settings_bak)

        os.makedirs(tmp_dir, exist_ok=True)

        for case in selected:
            results.append(run_one(exe_path, case, tmp_dir))
            passed_count = sum(1 for r in results if r["passed"])
            total = len(results)
            last = results[-1]
            print(
                f"  [{passed_count}/{total}] {case['id']}: "
                f"{'PASS' if last['passed'] else 'FAIL'} "
                f"{last.get('detail', '')}"
            )
    finally:
        restored = runner.restore_hud_dir(backup_root)
        if not restored:
            print(
                "!! CONFIG RESTORE FAILED — check "
                "~/.claude/plugins/claude-hud manually"
            )

        # A4: restore settings.json
        backup_status_parts = [
            "config-restored" if restored else "config-restore-failed"
        ]
        settings_ok = True
        try:
            if had_settings:
                current_bytes = b""
                if os.path.isfile(settings_path):
                    with open(settings_path, "rb") as f:
                        current_bytes = f.read()
                if current_bytes != _settings_bak:
                    with open(settings_path, "wb") as f:
                        f.write(_settings_bak)
                    with open(settings_path, "rb") as f:
                        if f.read() != _settings_bak:
                            settings_ok = False
            else:
                if os.path.isfile(settings_path):
                    os.remove(settings_path)
        except Exception:
            settings_ok = False

        if settings_ok:
            backup_status_parts.append("settings-restored")
        else:
            backup_status_parts.append("settings-restore-failed")
            print(
                "!! SETTINGS.JSON RESTORE FAILED — check "
                "~/.claude/settings.json manually"
            )

        snap["backup_status"] = "; ".join(backup_status_parts)

        # Fix 3: tmp cleanup after everything else
        shutil.rmtree(tmp_dir, ignore_errors=True)

    return results, restored


def main():
    """Parse args, select cases, run suite, write report."""
    parser = argparse.ArgumentParser(
        description="claude-hud black-box test suite"
    )
    parser.add_argument(
        "--case", help="run a single case id, e.g. D1-01"
    )
    parser.add_argument(
        "--exe",
        help=f"claude-hud exe path (default: {env.DEFAULT_EXE})",
    )
    parser.add_argument("--report", help="report output path override")
    args = parser.parse_args()

    exe_path = env.resolve_exe(args.exe)
    snap = env.snapshot(exe_path)

    print(f"[hud-test] exe: {exe_path}")
    selected = _select_cases(args.case, cases.CASES)

    backup_root = _backup_protocol(exe_path, snap)

    overall_start = time.monotonic()
    results, _restored = _run_suite(exe_path, snap, selected, backup_root)

    md = report.render_report(
        snap, results, time.monotonic() - overall_start
    )
    out_path = report.write_report(md, args.report)
    print(f"[hud-test] report: {out_path}")
    failed = [r for r in results if not r["passed"]]
    print(
        f"[hud-test] {len(results) - len(failed)}/{len(results)} passed"
    )
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
