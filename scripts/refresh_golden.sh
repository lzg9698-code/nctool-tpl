#!/usr/bin/env bash
# golden 基线刷新（B4.2 step 化）：把 `NCTOOL_UPDATE_GOLDEN=1 cargo test`
# 包成一条**带守卫**的人工步骤，避免"随手刷新"把真实回归洗成绿。
#
# 为什么需要脚本而不是直接敲那行命令：
#   1. 刷新会**写文件并跳过全部 golden 断言**（`assert_golden` 的刷新分支直接
#      `return`）。一旦在 CI 或脚本里被误触发，21 组正向 + 3 组负向会集体静默
#      退化成"跑得通即通过"。Rust 侧已有 `assert!(CI.is_none())` 兜底，本脚本在
#      shell 侧再挡一道（拒绝 CI / 拒绝 CI_* 环境）。
#   2. 刷新必须在**干净的工作区**上进行：否则 diff 里混进无关改动，人工复核时
#      分不清"哪些是基线变化、哪些是我别的改动"。
#   3. 刷新前先展示当前 diff 概览，让人**先看到变化再决定是否采纳**。
#
# 用法：
#   bash scripts/refresh_golden.sh          # 交互确认
#   bash scripts/refresh_golden.sh --yes    # 跳过确认（自动/脚本调用）
#
# 退出码：0 成功；1 守卫拒绝或刷新失败；2 用法错误。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ASSUME_YES=0
for arg in "$@"; do
    case "$arg" in
        --yes|-y) ASSUME_YES=1 ;;
        -h|--help)
            sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "未知参数: $arg" >&2; exit 2 ;;
    esac
done

# —— 守卫 1：拒绝 CI 环境（Rust 侧 assert_golden 也挡，这里 shell 侧再挡一道）——
if [[ -n "${CI:-}" || -n "${CI_NAME:-}" || -n "${GITHUB_ACTIONS:-}" ]]; then
    echo "✗ 检测到 CI 环境（CI/GITHUB_ACTIONS）。刷新 golden 会跳过全部基线断言，" >&2
    echo "  禁止在 CI 中执行。这是 B4.2 的硬性约束。" >&2
    exit 1
fi

# —— 守卫 2：golden 目录不得有未提交改动之外的混杂（提示而非阻断） ——
dirty=$(git status --porcelain | grep -v '^.. tests/golden/' || true)
if [[ -n "$dirty" ]]; then
    echo "⚠ 工作区存在 tests/golden/ 之外的改动："
    echo "$dirty" | sed 's/^/    /'
    echo "  建议先提交/暂存它们，否则 golden diff 会与无关改动混在一起，难以复核。"
    if [[ "$ASSUME_YES" -ne 1 ]]; then
        read -r -p "仍要继续？[y/N] " ans
        [[ "$ans" == "y" || "$ans" == "Y" ]] || { echo "已取消。"; exit 1; }
    fi
fi

# —— 刷新前快照：记录当前 golden 内容，便于刷新后展示真实差异 ——
echo "== 刷新前：当前 golden 与 HEAD 的差异（应为空，除非你已改过基线）=="
git diff --stat HEAD -- tests/golden/ || true

if [[ "$ASSUME_YES" -ne 1 ]]; then
    echo
    echo "即将执行：NCTOOL_UPDATE_GOLDEN=1 cargo test --workspace"
    echo "这会**重写** tests/golden/ 下的基线文件，并跳过全部 golden 断言。"
    read -r -p "确认刷新？[y/N] " ans
    [[ "$ans" == "y" || "$ans" == "Y" ]] || { echo "已取消。"; exit 1; }
fi

echo
echo "== 刷新中 =="
NCTOOL_UPDATE_GOLDEN=1 cargo test --workspace

echo
echo "== 刷新后：基线变化概览 =="
changed=$(git diff --stat -- tests/golden/ || true)
if [[ -z "$changed" ]]; then
    echo "（golden 无变化 —— 基线本就与当前输出一致）"
else
    echo "$changed"
    echo
    echo "▶ 下一步（**必需**）：逐行复核上面的改动，确认每一处变化都是你预期的，"
    echo "  然后在 commit message 里说明「为什么输出变了」。"
    echo "  新增/删除的基线文件："
    git status --porcelain -- tests/golden/ | sed 's/^/    /'
fi

# —— 守卫 3：刷新后基线数量不得减少（防止误删导致防线缩水） ——
count=$(find tests/golden -type f | wc -l | tr -d ' ')
if [[ "$count" -lt 45 ]]; then
    echo "✗ 刷新后 golden 文件数 = $count，少于 45（21 正向 ×2 + 3 负向）。" >&2
    echo "  可能有基线被误删，请检查 git status。" >&2
    exit 1
fi
echo
echo "✓ 刷新完成，基线文件数 = $count（≥ 45）"
