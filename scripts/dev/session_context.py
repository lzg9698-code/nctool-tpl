#!/usr/bin/env python3
"""Print a compact project baseline for the SessionStart hook.

Read-only: no writes, no network. Always exits 0 so a broken hook never blocks
the session; missing pieces are reported instead of guessed.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys

# Hooks and captured command output use UTF-8 on every platform.
for stream in (sys.stdout, sys.stderr):
    if hasattr(stream, "reconfigure"):
        stream.reconfigure(encoding="utf-8")

ROOT = Path(__file__).resolve().parents[2]
BRIEF = Path("D:/ai-brain/memory/projects/nctool-rustjinja/_brief.md")
LIMIT = 6000


def run(*command: str) -> str:
    try:
        result = subprocess.run(
            command,
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=10,
            encoding="utf-8",
            errors="replace",
        )
    except (OSError, subprocess.SubprocessError) as error:
        return f"<unavailable: {error}>"
    if result.returncode != 0:
        return f"<exit {result.returncode}>"
    return result.stdout.strip()


def status_summary() -> tuple[str, str]:
    branch = run("git", "rev-parse", "--abbrev-ref", "HEAD") or "<unknown>"
    head = run("git", "rev-parse", "--short", "HEAD") or "<unknown>"
    status = run("git", "status", "--short")
    if status.startswith("<"):
        return f"{branch} @ {head}", status
    lines = status.splitlines()
    if not lines:
        return f"{branch} @ {head}", "工作树干净"
    visible = lines[:20]
    suffix = f"（另有 {len(lines) - len(visible)} 行未显示）" if len(lines) > len(visible) else ""
    return f"{branch} @ {head}（{len(lines)} 项未提交）", "\n".join(visible) + suffix


def brief_text() -> str:
    if not BRIEF.is_file():
        return (
            f"项目简报不存在：{BRIEF.as_posix()}\n"
            "（这是正常状态；不要编造简报内容，也不要在本会话中创建它。）"
        )
    try:
        text = BRIEF.read_text(encoding="utf-8").strip()
    except OSError as error:
        return f"项目简报无法读取：{error}"
    if len(text) > 2000:
        text = text[:2000].rstrip() + "\n…（已截断）"
    return text


def build() -> str:
    head, status = status_summary()
    parts = [
        "## NCtool 会话基线",
        f"- 仓库：{ROOT.as_posix()}",
        f"- HEAD：{head}",
        "- 未提交改动：",
        "```",
        status,
        "```",
        "- 项目简报：",
        brief_text(),
        "- 规则入口：AGENTS.md（经 CLAUDE.md 导入）；开发入口 docs/README.md。",
        "- 环境自检：`python scripts/dev/check_env.py`。",
    ]
    text = "\n".join(parts)
    if len(text) > LIMIT:
        suffix = "\n…（会话基线已截断）"
        text = text[:LIMIT - len(suffix)].rstrip() + suffix
    return text


def main() -> int:
    try:
        context = build()
        output = {
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": context,
            }
        }
        print(json.dumps(output, ensure_ascii=False))
    except Exception as error:  # noqa: BLE001 - a hook must never fail the session
        print(f"session_context: {error}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    os.chdir(ROOT)
    raise SystemExit(main())
