#!/usr/bin/env python3
"""覆盖率门禁 —— 按**生产口径**判定，而不是 llvm-cov 的原始口径。

## 为什么需要它

`cargo llvm-cov` 把 `src/*.rs` 内 `#[cfg(test)]` 段本身也计入分母。本仓库
`src/lib.rs` 共 1824 行，`#[cfg(test)]` 在第 64 行，后面 1760 行全是测试 —— 于是
「新增测试」会**推高**覆盖率数字，「新增未覆盖的生产代码」却可能被稀释。
结果是：门禁显示 92%，而排除测试段后的生产代码只有 88% —— 门禁绿 ≠ 生产代码达标。

`--ignore-filename-regex` 只能按**文件路径**排除，无法排除 `src/` 内部的
`cfg(test)` 段，所以本脚本直接从 lcov 数据里剔除测试段后重新统计。

## 为什么还要剔除「不可执行行」

llvm-cov 的 lcov 导出会为**注释行、空行、纯分隔符行**也写 `DA:<line>,0`。这些行不含
任何可执行语句，**任何测试都不可能覆盖它们**。若把它们计入分母，门禁测的其实是
「注释 + 空行的覆盖率」，而且这个数字会随 llvm-cov / rustc 版本漂移（同一份源码，换
一个工具链，注释行的 DA 记录有无就会变），并不能反映真实代码覆盖率。

实测（本仓库，2026-09-26）：某函数 `pub fn build_registry` 命中 388 次，而它函数体内
紧跟其后的注释行却是 `DA:137,0`；`impl Ctx {` 是 `DA:62,0`。把「注释 / 空行 / 纯分隔符」
计入分母时生产口径为 **88.76%**，只统计可执行行后为 **92.88%** —— 后者才接近真实生产
代码覆盖率。故本脚本在剔除 `#[cfg(test)]` 段之后，**再剔除不可执行行**
（注释 / 空行 / 纯分隔符）。

**已知保守残留（有意保留，量化后 < 0.1pt）**：控制流块头（`if cond {` / `} else {`）、
match 分支头（`None => {`）与**无语句**的 `impl` / `mod` 头行里，llvm-cov 也会写 `DA:0`，
但它们要么与真实代码**纠缠**（条件求值、分支是否被走到本身就是覆盖信息），要么总量极小
（`impl` / `mod` 头行实测约 **3 行 ≈ 0.04pt**）—— 按文本剔除得不偿失。特别注意
**`fn` 签名行不是残留**：它承载**函数入口覆盖**（实测本仓库 1299 行 `fn` 签名 `DA>0`、
仅 26 行为 0），剔除会抹掉「某函数从未被调用」这类真实信号，故一律保留。

## 用法

    python scripts/check_coverage_caliber.py lcov.info [--min 89] [--top 15]

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


def executable_line_mask(src: str) -> set[int]:
    """返回源码中**可执行行**的行号集合（1 起）。

    判定：把注释与字符串字面量抹成空格后，该行仍有非空白字符，且不只是
    `( ) { } [ ] , ; .` 这类纯分隔符 —— 才算可执行行。

    llvm-cov 会给注释 / 空行 / 纯括号行也写 `DA:0`（见模块 docstring 的实测），
    这些行不含语句、无法被任何测试覆盖，必须从生产分母里剔除；否则门禁会去测
    「注释与空行的覆盖率」，且数值随工具链版本漂移。

    已知保守残留（**有意保留，量化后 < 0.1pt**）：

    * **`fn` 签名行不算残留**。llvm-cov 用 `fn` 签名行承载**函数入口覆盖**
      （实测本仓库 1299 行 `fn` 签名 `DA>0`、仅 26 行为 0），它是真实覆盖信号，
      剔除会抹掉「某函数从未被调用」这类信息。故 `fn` 签名一律保留。
    * 真正无歧义的非可执行声明头（`impl Ctx {` / `mod m {` 这类**无语句**行）
      在分母中仅约 **3 行 ≈ 0.04pt**，低于 0.1pt 的判定门槛，故不单独剔除，
      仅在此注明 —— 它方向保守（略压低分母），不会掩盖真实未覆盖代码。
    * 控制流块头（`if cond {` / `} else {`）与 match 分支头（`None => {`）同样
      与真实代码纠缠，一并保留。
    """
    clean = strip_noise(src.splitlines())
    mask: set[int] = set()
    for i, text in enumerate(clean):
        s = text.strip()
        if not s:
            continue
        if all(ch in "(){}[],;." for ch in s):
            continue
        mask.add(i + 1)
    return mask


def test_line_ranges(src: str) -> list[tuple[int, int]]:
    """返回 `#[cfg(test)]` 覆盖的行区间（1 起、闭区间）。

    覆盖三种形态：

    1. `#[cfg(test)] mod tests { ... }` —— 括号配平整块剔除；
    2. `#[cfg(test)] fn helper() { ... }` / `pub(crate) fn ...` —— **同样要配平**：
       这类"测试专用函数"（如 `cli/src/context.rs::Ctx::for_test`）的函数体
       （构造 `Ctx` 的结构体字面量等）是纯测试代码，若只标记到函数签名行，
       函数体会被算进**生产**分母 —— 既不公平，也违背本脚本"剔除 cfg(test) 段"
       的设计意图。**括号配平对 fn / mod 一视同仁**，不靠关键字判断；
    3. `#[cfg(test)] use ...;` / `static ...;` 等**无函数体**项 —— 只标记属性行与
       该项自身（到最近的 `;`），维持原有行为。

    判定依据是"紧跟属性后的项有没有 `{` 花括号体"，而非关键字：这样
    `#[cfg(test)] pub(crate) fn f()` 这类带可见性的函数也能被正确配平。
    """
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
        # 找到项的起始 `{`（有函数体/模块体）或 `;`（无体项）。
        # 窗口取 5 行：容纳多行函数签名（`pub(crate) fn f(\n  a: A,\n) -> R {`）。
        brace_line = None
        k = j
        while k < len(clean) and k < j + 5:
            if "{" in clean[k]:
                brace_line = k
                break
            if ";" in clean[k]:
                break
            k += 1
        if brace_line is None:
            # 无花括号体（`use ...;` / `static ...;` 等）：只标记属性行 + 该项本身
            ranges.append((start + 1, min(k, len(clean) - 1) + 1))
            continue
        # 括号配平：从项的起始 `{` 到与之匹配的 `}`，整段剔除。
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
    ap.add_argument("--min", type=float, default=89.0, help="生产口径行覆盖下限（%%）")
    ap.add_argument("--top", type=int, default=10, help="打印覆盖最低的前 N 个文件")
    ap.add_argument("--root", default=".", help="仓库根（用于定位源码文件）")
    ap.add_argument(
        "--strip-prefix",
        default="",
        help=(
            "剥掉 lcov 里 SF 路径的前缀，再按 --root 定位源码。"
            "用于分析 CI 产物：runner 上记录的是绝对路径"
            "（如 /home/runner/work/<repo>/<repo>/cli/src/args.rs），"
            "本地没有该路径，剥掉前缀后即可映射到本仓库。"
        ),
    )
    args = ap.parse_args()

    lcov_path = Path(args.lcov)
    if not lcov_path.is_file():
        print(f"::error::找不到覆盖率数据 {lcov_path}", file=sys.stderr)
        return 1
    root = Path(args.root)
    files = parse_lcov(lcov_path)

    rows: list[tuple[float, int, int, str]] = []
    tot_prod = tot_hit = 0
    tot_mid = tot_mid_hit = 0
    tot_all = tot_all_hit = 0
    test_lines_total = 0
    non_exec_total = 0

    for path, data in files.items():
        all_lines = len(data["DA"])
        all_hit = sum(1 for v in data["DA"].values() if v > 0)
        tot_all += all_lines
        tot_all_hit += all_hit

        # mid = 中间口径（已剔 `#[cfg(test)]` 段，但**尚未**剔不可执行行）；
        # prod = 生产口径（在 mid 基础上再剔不可执行行）。两者都留总数便于并排核对。
        mid: dict[int, int] = {}
        if path.endswith(".rs"):
            rel = path[len(args.strip_prefix):] if args.strip_prefix and path.startswith(args.strip_prefix) else path
            src_file = root / rel if not Path(rel).is_absolute() else Path(rel)
            if not src_file.is_file():
                # 读不到源码就无法区分生产段与测试段：此时"全算生产"会把覆盖率
                # 算高，等于静默放行 —— 宁可硬失败让 CI 暴露路径问题。
                print(
                    f"::error::覆盖率数据里的源码文件不存在，无法判定生产口径: {src_file}",
                    file=sys.stderr,
                )
                return 1
            src_text = src_file.read_text(encoding="utf-8", errors="replace")
            ranges = merge_ranges(test_line_ranges(src_text))
            test_lines = set()
            for s, e in ranges:
                test_lines.update(range(s, e + 1))
            test_lines_total += len(test_lines)
            exec_lines = executable_line_mask(src_text)
            # 中间口径：只剔 `#[cfg(test)]` 段。
            for ln, hits in data["DA"].items():
                if ln in test_lines:
                    continue
                mid[ln] = hits
            # 生产口径：再剔不可执行行（注释 / 空行 / 纯分隔符）。
            # 这些行不含语句，任何测试都覆盖不到，计入分母等于测「注释覆盖率」。
            prod: dict[int, int] = {}
            for ln, hits in mid.items():
                if ln not in exec_lines:
                    non_exec_total += 1
                    continue
                prod[ln] = hits
        else:
            # 非 .rs（理论上没有）：不区分测试段，两口径相同。
            mid = dict(data["DA"])
            prod = dict(data["DA"])

        tot_mid += len(mid)
        tot_mid_hit += sum(1 for v in mid.values() if v > 0)
        p = len(prod)
        h = sum(1 for v in prod.values() if v > 0)
        tot_prod += p
        tot_hit += h
        if p:
            rows.append((h / p * 100, h, p, path))

    pct = (tot_hit / tot_prod * 100) if tot_prod else 100.0
    mid_pct = (tot_mid_hit / tot_mid * 100) if tot_mid else 100.0
    raw_pct = (tot_all_hit / tot_all * 100) if tot_all else 100.0

    print("=" * 72)
    print("覆盖率三档口径（同一份 lcov，逐档多剔一类行 —— 便于单独核对每一步的效果）：")
    print(
        f"  ① 原始口径  不剔任何行                          → {tot_all_hit}/{tot_all} = {raw_pct:.2f}%"
        "（llvm-cov 原样；随「新增测试」虚涨，仅参考）"
    )
    print(
        f"  ② 中间口径  剔 `#[cfg(test)]` 段（保留不可执行行）    → {tot_mid_hit}/{tot_mid} = {mid_pct:.2f}%"
    )
    print(
        f"  ③ 生产口径  再剔不可执行行[注释/空行/纯分隔符]       → {tot_hit}/{tot_prod} = {pct:.2f}%"
        f"   ← 门禁判定用（阈值 {args.min:g}%）"
    )
    print(
        f"  剔除明细：①→② 剔 {tot_all - tot_mid} 行（`#[cfg(test)]` 段内的 DA 行；"
        f"该段源码共 {test_lines_total} 行）；②→③ 再剔 {non_exec_total} 行不可执行行。"
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
