#!/usr/bin/env bash
# 多工序真实场景演示：用 5 个内置模板拼装一个"端面 + 4 孔 + 切断"
# 的完整程序（法兰盘简化版）。
#
# 用法：
#   1. 先构建 CLI：  cargo build -p nctool-cli --bin nctool
#   2. 运行本脚本：  bash examples/multi_op_demo.sh
#   3. 结果在：      out_multi_op.nc / out_multi_op_wfl_m65.nc
#
# 配套文档：docs/REAL_PART_WALKTHROUGH.md

set -euo pipefail

# 定位 nctool 二进制（脚本在 examples/ 下）
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
NCTOOL="$ROOT/target/debug/nctool.exe"

if [[ ! -x "$NCTOOL" ]]; then
    echo "找不到 $NCTOOL，请先 cargo build -p nctool-cli --bin nctool" >&2
    exit 1
fi

OUT="$ROOT/out_multi_op.nc"
OUT_WFL="$ROOT/out_multi_op_wfl_m65.nc"

# 工序设计（法兰盘简化版）：
#   program_header
#   safe_move   × 1   抬刀到安全高度，定位到 (0, 0)
#   drill_cycle × 4   钻 4×Ø8 孔，X 坐标 20/30/40/50，Y=0
#   program_footer
#
# 行号说明：本演示**不勾行号**，避免每段独立 render 时行号都从 N0010
# 重复（手工 sed 改 N 序号的方案见 docs/REAL_PART_WALKTHROUGH.md）。
# 阶段 4 的 `nctool part generate` 会原生支持跨工序行号续编。

# 通用参数
PROG=1001
PART=FLANGE_DEMO
SAFE_Z=5
FEED=80
R_PLANE=2
DEPTH=10

render_for() {
    local machine=$1
    local out=$2
    : > "$out"

    $NCTOOL --machine "$machine" render program_header \
        --param prog=$PROG --param part_name=$PART \
        >> "$out"

    $NCTOOL --machine "$machine" render safe_move \
        --param x=0 --param y=0 --param safe_z=$SAFE_Z \
        >> "$out"

    local x
    for x in 20 30 40 50; do
        $NCTOOL --machine "$machine" render drill_cycle \
            --param x=$x --param y=0 \
            --param r_plane=$R_PLANE --param depth=-$DEPTH --param feed=$FEED \
            >> "$out"
    done

    $NCTOOL --machine "$machine" render program_footer >> "$out"
}

echo "=== generic 机床 ==="
render_for generic "$OUT"
echo "已生成 $OUT"
echo "字节数：$(wc -c < "$OUT")"
echo
echo "=== wfl_m65 机床 ==="
render_for wfl_m65 "$OUT_WFL"
echo "已生成 $OUT_WFL"
echo "字节数：$(wc -c < "$OUT_WFL")"
echo
echo "=== diff（应仅在机床相关键值处不同，如 program_prefix / spindle_on 等） ==="
diff "$OUT" "$OUT_WFL" || true
echo
echo "=== 前 20 行（generic）==="
head -20 "$OUT"
