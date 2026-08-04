"""Markdown test report generation."""
import datetime
import os

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
REPORTS_DIR = os.path.join(REPO_ROOT, "reports")


def render_report(env_snapshot: dict, results: list, duration_s: float) -> str:
    passed = [r for r in results if r["passed"]]
    failed = [r for r in results if not r["passed"]]
    pass_rate = len(passed) / len(results) * 100 if results else 0.0
    lines = [
        "# Claude HUD 黑盒测试报告",
        "",
        f"- 生成时间：{env_snapshot.get('run_at', 'n/a')}",
        f"- exe：`{env_snapshot.get('exe', 'n/a')}`（mtime {env_snapshot.get('exe_mtime', 'n/a')}）",
        f"- python：{env_snapshot.get('python', 'n/a')}",
        f"- 平台：{env_snapshot.get('platform', 'n/a')}",
        f"- 配置备份状态：{env_snapshot.get('backup_status', 'n/a')}",
        f"- 总耗时：{duration_s:.1f}s",
        "",
        "## 汇总",
        "",
        "| 指标 | 值 |",
        "|---|---|",
        f"| 总用例 | {len(results)} |",
        f"| 通过 | {len(passed)} |",
        f"| 失败 | {len(failed)} |",
        f"| 通过率 | {pass_rate:.1f}% |",
        "",
        "## 用例明细",
        "",
        "| ID | 维度 | 名称 | 结果 | 耗时 | 说明 |",
        "|---|---|---|---|---|---|",
    ]
    for r in results:
        lines.append(
            f"| {r['id']} | {r['dim']} | {str(r['name']).replace('|', '\\|')} | "
            f"{'PASS' if r['passed'] else 'FAIL'} | {r.get('duration_s', 0.0):.2f}s | "
            f"{str(r.get('detail', '')).replace('|', '\\|')} |"
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
            (r.get("stdout") or "")[:500],
            "```",
            "",
            "实际 stderr（截断 500 字节）：",
            "",
            "```",
            (r.get("stderr") or "")[:500],
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
        path = os.path.join(
            REPORTS_DIR,
            f"test-report-{datetime.date.today().isoformat()}.md",
        )
    os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        f.write(markdown)
    return path
