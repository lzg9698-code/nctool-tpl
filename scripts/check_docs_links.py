"""校验 Markdown 文档：相对链接是否存在、锚点是否可解析。

用法：python scripts/check_docs_links.py <file.md> [...]
"""
import re
import sys
import unicodedata
from pathlib import Path

LINK_RE = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
FENCE_RE = re.compile(r"^```")
HEADING_RE = re.compile(r"^(#{1,6})\s+(.*?)\s*$")


def slug(text: str) -> str:
    """近似 GitHub 的 heading anchor 生成规则。"""
    t = text.strip().lower()
    t = re.sub(r"`|\*|_", "", t)
    # 去掉标点/符号，保留字母数字、空白、连字符与 CJK
    t = "".join(ch for ch in t if ch.isalnum() or ch.isspace() or ch == "-" or unicodedata.category(ch) == "Lo")
    t = t.replace(" ", "-")
    return t


def headings(path: Path):
    out = []
    in_fence = False
    for line in path.read_text(encoding="utf-8").splitlines():
        if FENCE_RE.match(line.strip()):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        m = HEADING_RE.match(line)
        if m:
            out.append(slug(m.group(2)))
    return set(out)


def check(path: Path):
    problems = []
    own = headings(path)
    in_fence = False
    for i, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if FENCE_RE.match(line.strip()):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        for target in LINK_RE.findall(line):
            target = target.strip()
            if not target or target.startswith(("http://", "https://", "mailto:")):
                continue
            file_part, _, anchor = target.partition("#")
            if file_part:
                resolved = (path.parent / file_part).resolve()
                if not resolved.exists():
                    problems.append(f"{path}:{i} 文件不存在: {file_part}")
                    continue
                if anchor and resolved.suffix == ".md":
                    if anchor not in headings(resolved):
                        problems.append(f"{path}:{i} 锚点不存在: {anchor} @ {file_part}")
            else:
                if anchor and anchor not in own:
                    problems.append(f"{path}:{i} 本文件锚点不存在: #{anchor}")
    return problems


def main():
    problems = []
    for arg in sys.argv[1:]:
        p = Path(arg).resolve()
        if p.is_dir():
            for f in sorted(p.rglob("*.md")):
                if "target" in f.parts or "NVIDIA" in f.parts:
                    continue
                problems += check(f)
        else:
            problems += check(p)
    if problems:
        print("\n".join(problems))
        print(f"\n共 {len(problems)} 处问题")
        sys.exit(1)
    print("链接与锚点检查通过")


if __name__ == "__main__":
    main()
