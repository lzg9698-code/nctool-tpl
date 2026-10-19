#!/usr/bin/env python3
"""Read-only check of the local NCtool development environment.

Exit codes: 0 = every required check passed, 1 = missing or failing item,
2 = the check itself could not run. Optional items never fail the run.
Mirrors the fail-closed convention of scripts/audit_dependencies.py.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

# JSON and captured diagnostics have a stable encoding on Windows as well.
for stream in (sys.stdout, sys.stderr):
    if hasattr(stream, "reconfigure"):
        stream.reconfigure(encoding="utf-8")

ROOT = Path(__file__).resolve().parents[2]


class Result:
    def __init__(self, name: str, ok: bool, detail: str, fix: str = "", required: bool = True):
        self.name = name
        self.ok = ok
        self.detail = detail
        self.fix = fix
        self.required = required

    def as_dict(self) -> dict:
        return {
            "name": self.name,
            "status": "ok" if self.ok else ("missing" if self.required else "optional"),
            "detail": self.detail,
            "fix": self.fix,
            "required": self.required,
        }


def run(*command: str, timeout: int = 120) -> tuple[int, str]:
    try:
        result = subprocess.run(
            command,
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=timeout,
            encoding="utf-8",
            errors="replace",
            shell=os.name == "nt",
        )
    except (OSError, subprocess.SubprocessError) as error:
        return 127, str(error)
    return result.returncode, (result.stdout + result.stderr).strip()


def version_tuple(text: str) -> tuple[int, ...]:
    match = re.search(r"(\d+)\.(\d+)(?:\.(\d+))?", text)
    return tuple(int(part) for part in match.groups(default="0")) if match else ()


def min_version(name: str, text: str, minimum: tuple[int, ...], fix: str) -> Result:
    actual = version_tuple(text)
    ok = bool(actual) and actual[: len(minimum)] >= minimum
    return Result(name, ok, text or "not found", fix)


def check_toolchain() -> list[Result]:
    results = []
    code, text = run("rustc", "--version")
    results.append(
        min_version(
            "rustc >= 1.89",
            text if code == 0 else "",
            (1, 89),
            "rustup toolchain install stable",
        )
    )
    code, text = run("cargo", "--version")
    results.append(
        min_version(
            "cargo >= 1.89",
            text if code == 0 else "",
            (1, 89),
            "rustup toolchain install stable",
        )
    )
    code, text = run("cargo", "+1.89.0", "--version")
    results.append(
        Result(
            "MSRV toolchain 1.89.0",
            code == 0,
            text if code == 0 else "未安装",
            "rustup toolchain install 1.89.0",
        )
    )
    code, text = run("node", "--version")
    results.append(
        min_version(
            "Node >= 22.12",
            text if code == 0 else "",
            (22, 12),
            "安装 Node.js 22.12 或更新版本",
        )
    )
    code, text = run("npm", "--version")
    results.append(Result("npm", code == 0, text, "随 Node.js 安装"))
    code, text = run("python", "--version")
    results.append(
        min_version(
            "Python >= 3.9",
            text if code == 0 else "",
            (3, 9),
            "安装 Python 3，或在 Windows 使用 py -3.13",
        )
    )
    return results


def check_frontend() -> list[Result]:
    results = []
    modules = ROOT / "ui" / "node_modules"
    results.append(
        Result(
            "ui/node_modules",
            modules.is_dir(),
            "存在" if modules.is_dir() else "未安装前端依赖",
            "npm ci --prefix ui",
        )
    )
    if modules.is_dir():
        code, text = run("npm", "ls", "--prefix", "ui", "--depth=0", timeout=180)
        results.append(
            Result(
                "ui 依赖与锁文件一致",
                code == 0,
                "npm ls 通过" if code == 0 else text.splitlines()[0][:200],
                "npm ci --prefix ui",
            )
        )
    code, text = run("node", "scripts/build_ui.mjs", "--check", timeout=180)
    results.append(
        Result(
            "嵌入资源与 ui/src 一致",
            code == 0,
            "build_ui --check 通过" if code == 0 else text[-400:],
            "node scripts/build_ui.mjs",
        )
    )
    return results


def playwright_module() -> Path | None:
    configured = os.environ.get("NCTOOL_PLAYWRIGHT")
    if configured:
        path = Path(configured)
        return path if path.is_dir() else None
    local = ROOT / "target" / "agent-tools" / "node_modules" / "playwright"
    return local if local.is_dir() else None


def check_browser() -> list[Result]:
    module = playwright_module()
    if module is None:
        return [
            Result(
                "Playwright（可选）",
                False,
                "未安装到独立目录",
                "node scripts/dev/setup_browser.mjs --apply",
                required=False,
            )
        ]
    results = []
    code, text = run(
        "node",
        "-e",
        "const p=require(process.argv[1]);console.log(require(process.argv[1]+'/package.json').version);"
        "const e=p.chromium.executablePath();"
        "console.log(require('node:fs').existsSync(e)?'browser=ok':'browser=missing');"
        "console.log(e);",
        str(module),
        timeout=60,
    )
    detail = text.replace("\n", " / ") if text else "无法加载"
    results.append(
        Result(
            "Playwright（可选）",
            code == 0 and "browser=ok" in text,
            detail[:300],
            "node scripts/dev/setup_browser.mjs --apply",
            required=False,
        )
    )
    return results


def check_agent_env() -> list[Result]:
    results = []
    skill_root = ROOT / ".claude" / "skills"
    source_root = ROOT / ".agents" / "skills"
    skills = (
        sorted(path.name for path in source_root.iterdir() if (path / "SKILL.md").is_file())
        if source_root.is_dir()
        else []
    )
    missing = [name for name in skills if not (skill_root / name).exists()]
    results.append(
        Result(
            "Claude 技能联接",
            not missing,
            f"{len(skills) - len(missing)}/{len(skills)} 个联接有效"
            if skills
            else "未找到 .agents/skills",
            "node scripts/dev/setup_agent_env.mjs --apply",
        )
    )
    settings = ROOT / ".claude" / "settings.json"
    results.append(
        Result(
            ".claude/settings.json",
            settings.is_file(),
            "存在" if settings.is_file() else "未生成",
            "node scripts/dev/setup_agent_env.mjs --apply",
            required=False,
        )
    )
    claude_md = ROOT / "CLAUDE.md"
    imported = claude_md.is_file() and bool(
        re.search(r"^@AGENTS\.md\s*$", claude_md.read_text(encoding="utf-8"), re.MULTILINE)
    )
    results.append(
        Result(
            "CLAUDE.md 导入 AGENTS.md",
            imported,
            "已导入" if imported else "缺失或未导入",
            "恢复 CLAUDE.md 中的 @AGENTS.md 行",
        )
    )
    mcp = ROOT / ".mcp.json"
    ok, detail = False, "未声明"
    if mcp.is_file():
        try:
            servers = json.loads(mcp.read_text(encoding="utf-8")).get("mcpServers", {})
            ok = "basic-memory" in servers
            detail = ", ".join(servers) or "无 server"
        except ValueError as error:
            detail = f"JSON 无效：{error}"
    results.append(
        Result(
            ".mcp.json 声明 basic-memory",
            ok,
            detail,
            "恢复 .mcp.json 中的 basic-memory 条目",
            required=False,
        )
    )
    if shutil.which("basic-memory") is None:
        results.append(
            Result(
                "basic-memory 命令（可选）",
                False,
                "不在 PATH 中",
                "uv tool install basic-memory",
                required=False,
            )
        )
    brief = Path("D:/ai-brain/memory/projects/nctool-rustjinja/_brief.md")
    results.append(
        Result(
            "项目简报（可选）",
            brief.is_file(),
            brief.as_posix() if brief.is_file() else "尚未创建",
            "按 D:/ai-brain/templates/brief.md 创建",
            required=False,
        )
    )
    return results


def check_disk() -> list[Result]:
    usage = shutil.disk_usage(ROOT)
    free_gb = usage.free / 1024**3
    target = ROOT / "target"
    entries = len([p for p in target.iterdir() if p.is_dir()]) if target.is_dir() else 0
    return [
        Result(
            "可用磁盘空间",
            free_gb >= 20,
            f"{free_gb:.1f} GB 可用；target/ 下 {entries} 个子目录",
            "python scripts/dev/clean_targets.py --dry-run",
        )
    ]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", help="machine-readable output")
    parser.add_argument("--brief", action="store_true", help="only show failures")
    options = parser.parse_args()

    groups = [
        ("工具链", check_toolchain),
        ("前端", check_frontend),
        ("浏览器验收", check_browser),
        ("Agent 环境", check_agent_env),
        ("资源", check_disk),
    ]
    results: list[Result] = []
    for title, checker in groups:
        try:
            group = checker()
        except Exception as error:  # noqa: BLE001 - report, do not crash
            print(f"check failed in {title}: {error}", file=sys.stderr)
            return 2
        for result in group:
            result.group = title  # type: ignore[attr-defined]
        results.extend(group)

    failed = [item for item in results if item.required and not item.ok]
    if options.json:
        print(json.dumps([item.as_dict() for item in results], ensure_ascii=False, indent=2))
    else:
        for title, _ in groups:
            rows = [item for item in results if getattr(item, "group", "") == title]
            if options.brief and all(item.ok or not item.required for item in rows):
                continue
            print(f"\n[{title}]")
            for item in rows:
                mark = "OK  " if item.ok else ("FAIL" if item.required else "WARN")
                print(f"  {mark} {item.name}: {item.detail}")
                if not item.ok and item.fix:
                    print(f"       修复：{item.fix}")
        required = [item for item in results if item.required]
        optional = [item for item in results if not item.required]
        print(
            f"\n{sum(1 for item in required if item.ok)}/{len(required)} 项必需检查通过；"
            f"{sum(1 for item in optional if not item.ok)}/{len(optional)} 项可选项未就绪。"
        )
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
