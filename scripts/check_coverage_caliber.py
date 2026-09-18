#!/usr/bin/env python3
"""覆盖率门禁 —— 按**生产口径**判定，而不是 llvm-cov 的原始口径。

## 为什么需要它

`cargo llvm-cov` 把 `src/*.rs` 内 `#[cfg(test)]` 段本身也计入分母。本仓库
`src/lib.rs` 共 1824 行，`#[cfg(test)]` 在第 64 行，后面 1760 行全是测试 —— 于是
「新增测试」会**推高**覆盖率数字，「新增未覆盖的生产代码」却可能被稀释。
结果是：门禁显示 92%，而排除测试段后的生产代码只有 88% —— 门禁绿 ≠ 生产代码达标。

`--ignore-filename-regex` 只能按**文件路径**排除，无法排除 `src/` 内部的
`cfg(test)` 段，所以本脚本直接从 lcov 数据里剔除测试段后重新统计。

## 用法

    python scripts/check_coverage_caliber.py lcov.info [--min 88] [--top 15]

退出码：0 = 达标（或仅用于查看数据），1 = 未达标。
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

IDENT_EXTRA = "_"


def strip_noise(lines: list[str]) -> list[str]:
    """把注释与字符串字面量替换为等长空格（保留下标，便于后续定位）。

    只处理 `//`、`/* */`、`"..."`、`r"..."`、`r#"..."#`。字符字面量一律保留：
    生命周期 `'a` 与 `'{'` 难以可靠区分，而测试代码里出现含花括号的字符字面量
    概率极低；真遇到时下面的括号配平会失败并回退到「到文件末尾」。
    """
    out: list[str] = []
    in_block = False
    raw = None  # 原始字符串的结束定界符
    for line in lines:
        buf = list(line)
        i = 0
        n = len(line)
        while i < n:
            if in_block:
                if line.startswith("*/", i):
                    buf[i] = buf[i + 1] = " "
                    i += 2
                    in_block = False
                else:
                    buf[i] = " "
                    i += 1
                continue
            if raw is not None:
                if line.startswith(raw, i):
                    for k in range(i, min(i + len(raw), n)):
                        buf[k] = " "
                    i += len(raw)
                    raw = None
                else:
                    buf[i] = " "
                    i += 1
                continue
            ch = line[i]
            if ch == "/" and i + 1 < n and line[i + 1] == "/":
                for k in range(i, n):
                    buf[k] = " "
                break
            if ch == "/" and i + 1 < n and line[i + 1] == "*":
                buf[i] = buf[i + 1] = " "
                i += 2
                in_block = True
                continue
            if ch == "r" and (i == 0 or not (line[i - 1].isalnum() or line[i - 1] == "_")):
                j = i + 1
                hashes = 0
                while j < n and line[j] == "#":
                    hashes += 1
                    j += 1
                if j < n and line[j] == '"':
                    raw = '"' + "#" * hashes
                    for k in range(i, min(j + 1, n)):
                        buf[k] = " "
                    i = j + 1
                    continue
            if ch == '"':
                buf[i] = " "
                i += 1
                while i < n:
                    if line[i] == "\\":
                        buf[i] = " "
                        if i + 1 < n:
                            buf[i + 1] = " "
                        i += 2
                        continue
                    if line[i] == '"':
                        buf[i] = " "
                        i += 1
                        break
                    buf[i] = " "
                    i += 1
                continue
            i += 1
        out.append("".join(buf))
    return out


def test_line_ranges(src: str) -> list[tuple[int, int]]:
    """返回 `#[cfg(test)]` 覆盖的行区间（1 起、闭区间）。"""
    lines = src.splitlines()
    if not lines:
        return []
    clean = strip_noise(lines)
    ranges: list[tuple[int, int]] = []

    for idx, text in enumerate(clean):
        stripped = text.strip()
        if not (stripped.startswith("#[") and "cfg(" in stripped and "test" in stripped):
            continue
        start = idx
        # 跳过连续的属性行（`#[cfg(test)]` + `#[allow(...)]` 等）
        j = idx
        while j < len(clean) and clean[j].strip().startswith("#["):
            j += 1
        if j >= len(clean):
            ranges.append((start + 1, len(clean)))
            continue
        # 找到 `mod` 与其后的 `{`：非 mod 项（如 `use ...;`）只标记属性与项本身
        brace_line = None
        k = j
        while k < len(clean) and k < j + 5:
            if "{" in clean[k]:
                brace_line = k
                break
            if ";" in clean[k]:
                break
            k += 1
        if brace_line is None or "mod" not in clean[j]:
            ranges.append((start + 1, min(k, len(clean) - 1) + 1))
            continue
        # 括号配平
        depth = 0
        end = len(clean) - 1
        for m in range(brace_line, len(clean)):
            for ch in clean[m]:
                if ch == "{":
                    depth += 1
                elif ch == "}":
                    depth -= 1
            if depth == 0:
                end = m
                break
        ranges.append((start + 1, end + 1))
    return ranges


def merge_ranges(ranges: list[tuple[int, int]]) -> list[tuple[int, int]]:
    if not ranges:
        return []
    ranges = sorted(ranges)
    merged = [list(ranges[0])]
    for s, e in ranges[1:]:
        if s <= merged[-1][1] + 1:
            merged[-1][1] = max(merged[-1][1], e)
        else:
            merged.append([s, e])
    return [(s, e) for s, e in merged]


def parse_lcov(path: Path) -> dict[str, dict]:
    files: dict[str, dict] = {}
    cur = None
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.rstrip("\n")
        if raw.startswith("SF:"):
            cur = raw[3:]
            files.setdefault(cur, {"LF": 0, "LH": 0, "DA": {}})
        elif cur is None:
            continue
        elif raw.startswith("LF:"):
            files[cur]["LF"] += int(raw[3:])
        elif raw.startswith("LH:"):
            files[cur]["LH"] += int(raw[3:])
        elif raw.startswith("DA:"):
            parts = raw[3:].split(",")
            if len(parts) >= 2:
                try:
                    line_no, hits = int(parts[0]), int(parts[1])
                except ValueError:
                    continue
                files[cur]["DA"][line_no] = files[cur]["DA"].get(line_no, 0) + hits
    return files


def main() -> int:
    ap = argparse.ArgumentParser(description="按生产口径校验 lcov 行覆盖率")
    ap.add_argument("lcov", help="lcov.info 路径")
    ap.add_argument("--min", type=float, default=88.0, help="生产口径行覆盖下限（%%）")
    ap.add_argument("--top", type=int, default=10, help="打印覆盖最低的前 N 个文件")
    ap.add_argument("--root", default=".", help="仓库根（用于定位源码文件）")
    args = ap.parse_args()

    lcov_path = Path(args.lcov)
    if not lcov_path.is_file():
        print(f"::error::找不到覆盖率数据 {lcov_path}", file=sys.stderr)
        return 1
    root = Path(args.root)
    files = parse_lcov(lcov_path)

    rows: list[tuple[float, int, int, str]] = []
    tot_prod = tot_hit = 0
    tot_all = tot_all_hit = 0
    test_lines_total = 0

    for path, data in files.items():
        all_lines = len(data["DA"])
        all_hit = sum(1 for v in data["DA"].values() if v > 0)
        tot_all += all_lines
        tot_all_hit += all_hit

        prod: dict[int, int] = {}
        if path.endswith(".rs"):
            src_file = root / path if not Path(path).is_absolute() else Path(path)
            if not src_file.is_file():
                # 读不到源码就无法区分生产段与测试段：此时"全算生产"会把覆盖率
                # 算高，等于静默放行 —— 宁可硬失败让 CI 暴露路径问题。
                print(
                    f"::error::覆盖率数据里的源码文件不存在，无法判定生产口径: {src_file}",
                    file=sys.stderr,
                )
                return 1
            ranges = merge_ranges(
                test_line_ranges(src_file.read_text(encoding="utf-8", errors="replace"))
            )
            test_lines = set()
            for s, e in ranges:
                test_lines.update(range(s, e + 1))
            test_lines_total += len(test_lines)
            prod = {ln: hits for ln, hits in data["DA"].items() if ln not in test_lines}
        else:
            prod = dict(data["DA"])

        p = len(prod)
        h = sum(1 for v in prod.values() if v > 0)
        tot_prod += p
        tot_hit += h
        if p:
            rows.append((h / p * 100, h, p, path))

    pct = (tot_hit / tot_prod * 100) if tot_prod else 100.0
    raw_pct = (tot_all_hit / tot_all * 100) if tot_all else 100.0

    print("=" * 72)
    print(f"生产口径行覆盖：{tot_hit}/{tot_prod} = {pct:.2f}%   （阈值 {args.min:g}%）")
    print(
        f"对照：llvm-cov 原始口径 {tot_all_hit}/{tot_all} = {raw_pct:.2f}%"
        f"（另有 {test_lines_total} 行源码位于 `#[cfg(test)]` 段，已剔除；"
        "该口径会随「新增测试」虚涨，仅作参考，不作门禁）"
    )
    print("=" * 72)
    print(f"\n覆盖最低的 {args.top} 个文件（生产口径）：")
    for p, h, t, name in sorted(rows)[: args.top]:
        print(f"  {p:6.2f}%  {h:5d}/{t:5d}  {name}")

    if pct + 1e-9 < args.min:
        print(
            f"\n::error::生产口径行覆盖 {pct:.2f}% 低于阈值 {args.min:g}%"
            f"（差 {args.min - pct:.2f}pt ≈ {(args.min - pct) / 100 * tot_prod:.0f} 行未覆盖生产代码）",
            file=sys.stderr,
        )
        return 1
    print(f"\nOK：生产口径行覆盖 {pct:.2f}% ≥ {args.min:g}%")
    return 0


if __name__ == "__main__":
    sys.exit(main())
