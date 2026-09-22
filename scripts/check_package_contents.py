#!/usr/bin/env python3
"""发布包内容守卫（A5）：确保 `nctool-tpl` 的 crates.io 发布物只含库必需文件。

## 背景

根 crate 是**库**，但仓库里同时住着 CLI / Web UI / 开发脚本 / 原型产物。此前
`cargo package` 会把这些一并打进 `.crate`（120 个文件、498 KiB），既浪费下载量，
也让 crates.io 页面上出现一堆与库无关的内容（原型 PNG、验收脚本、模型交接 HTML…）。

## 做法

`Cargo.toml` 的 `exclude` 是唯一真源；本脚本对 `cargo package --list` 的实际输出
做**黑名单断言**（而不是硬编码白名单）——白名单会在库合法新增文件时误报，黑名单
只拦明确不该进包的目录/文件。用 `--offline` 避免 CI 依赖网络索引。

用法：python3 scripts/check_package_contents.py
退出码：0 = 通过；1 = 发现不该进包的内容（或 cargo package 本身失败）。
"""

from __future__ import annotations

import subprocess
import sys

# 明确**不得**出现在 nctool-tpl 发布物中的路径前缀/文件名。
# 每条都对应仓库里的一个非库资产；新增此类资产时在此登记，守卫会自动拦住回归。
FORBIDDEN = [
    "output/",  # UI 原型 PNG / 验收脚本 / 工艺参数 JSON
    "scripts/",  # CI 对拍脚本与 fixture
    "ui/",  # Web UI 单文件（归 cli 打包）
    "docs/",  # 仓库文档（Cargo.toml 已 exclude）
    "examples/multi_op_demo.sh",  # 面向整个 workspace 的演示脚本
    "启动UI.bat",
    "overview.md",
    "CODE_REVIEW_AND_DEV_PLAN.md",
    "templates/README.md",
    "templates/templates.yaml",
    "templates/variables.yaml",
    "templates/grooving/",
    "templates/machines/",
    "templates/turning/undercut.j2",
    "templates/turning/undercut_es.j2",
    "templates/turning/undercut_fs.j2",
    "templates/turning/_undercut_common.j2",
]


def package_list() -> list[str] | None:
    """返回 `cargo package --list` 的文件列表；失败返回 None。"""
    try:
        out = subprocess.run(
            ["cargo", "package", "--list", "--offline", "--allow-dirty"],
            capture_output=True,
            text=True,
            check=True,
            env={"CARGO_INCREMENTAL": "0", "PATH": __import__("os").environ.get("PATH", "")},
        )
    except (subprocess.CalledProcessError, FileNotFoundError) as e:
        print(f"✗ 无法列出发布包内容: {e}", file=sys.stderr)
        if isinstance(e, subprocess.CalledProcessError) and e.stderr:
            print(e.stderr, file=sys.stderr)
        return None
    return [line for line in out.stdout.splitlines() if line.strip()]


def main() -> int:
    files = package_list()
    if files is None:
        return 1

    problems = [
        f for f in files if any(f == bad or f.startswith(bad) for bad in FORBIDDEN)
    ]
    if problems:
        print("✗ nctool-tpl 发布包含有非库资产（请在 Cargo.toml 的 exclude 中补上）：")
        for p in problems:
            print(f"    {p}")
        return 1

    # 正向断言：库必需的最小集合必须在（防止 exclude 写过头把源码也排掉）
    required = ["src/lib.rs", "Cargo.toml", "LICENSE", "README.md"]
    missing = [r for r in required if r not in files]
    if missing:
        print(f"✗ 发布包缺少库必需文件: {missing}")
        return 1

    print(f"✓ 发布包内容检查通过：{len(files)} 个文件，无非库资产")
    return 0


if __name__ == "__main__":
    sys.exit(main())
