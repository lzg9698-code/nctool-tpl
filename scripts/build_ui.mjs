#!/usr/bin/env node
// UI 构建：把 `ui/src/*.part.html` 逐字节拼成 `ui/index.html` 与 `cli/ui/index.html`。
//
// 背景：`ui/index.html` 曾是唯一真值（一份 2900+ 行的单文件），而
// `cli/ui/index.html` 是它的**手工副本**（被 `include_str!` 嵌进二进制）。单文件
// 撞上 3000 行上限、副本靠人肉同步 —— 两件迟早出问题的事。本方案把源码上移到
// `ui/src/` 片段，生成物由本脚本产出：**同一份 Buffer 写两次**，两副本天然一致。
//
// 做法：
//   1. 读 `ui/src/*.part.html`，按**文件名升序**（前缀即序号）**逐字节**首尾相接；
//   2. 写模式：同一份 buf 写两份 + 读回自检；`--check`：只比对，不写盘。
//   拼接不插任何分隔符、不改换行、不 trim —— 故片段可跨标签（`<script>` 在
//   `30_`、`</script>` 在 `32_`），只有拼装产物才保证是合法 HTML。
//
// 用法：
//   node scripts/build_ui.mjs          # 生成两份
//   node scripts/build_ui.mjs --check  # 校验生成物与片段一致（CI 门禁，不写盘）
//
// 退出码：0 = 一致 / 已生成；1 = 不一致、片段缺失或生成物缺失。

import { readdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const SRC_DIR = join(root, "ui", "src");
const OUT_FILES = [join(root, "ui", "index.html"), join(root, "cli", "ui", "index.html")];

/** 读片段并按文件名升序拼接（**逐字节**，Buffer 相加以保住中文的字节偏移）。 */
function assemble() {
  if (!existsSync(SRC_DIR)) {
    console.error(`✗ 片段目录不存在：${SRC_DIR}`);
    process.exit(1);
  }
  // 不递归；大小写敏感的 `.part.html` 后缀；显式排序（readdir 顺序不保证）。
  const parts = readdirSync(SRC_DIR)
    .filter((n) => n.endsWith(".part.html"))
    .sort();
  if (parts.length === 0) {
    // 防"真空洞"：片段一个都没有时，拼接结果必然为空，直接放行等于没有门禁。
    console.error(`✗ ${SRC_DIR} 下没有任何 *.part.html 片段`);
    process.exit(1);
  }
  const bufs = parts.map((n) => readFileSync(join(SRC_DIR, n)));
  return { buf: Buffer.concat(bufs), parts, bufs };
}

/** 首个差异字节偏移 + 两侧上下文（"第几行"对多字节字符会错位，故按字节报）。 */
function describeDiff(expected, actual) {
  const n = Math.min(expected.length, actual.length);
  let at = -1;
  for (let i = 0; i < n; i++) {
    if (expected[i] !== actual[i]) {
      at = i;
      break;
    }
  }
  if (at === -1) {
    // 前缀相同 → 差异在长度上（一方是另一方的前缀）。
    return `内容长度不同：片段拼出 ${expected.length} 字节，文件 ${actual.length} 字节`;
  }
  const from = Math.max(0, at - 24);
  const ctx = (b) => JSON.stringify(b.subarray(from, at + 24).toString("utf8"));
  const line = expected.subarray(0, at).toString("utf8").split("\n").length;
  return `首个差异在字节偏移 ${at}（约第 ${line} 行）\n  片段拼出: ${ctx(expected)}\n  文件内容: ${ctx(actual)}`;
}

function main() {
  const check = process.argv.includes("--check");
  const { buf, parts } = assemble();
  const lines = buf.toString("utf8").split("\n").length - 1;
  const md5 = createHash("md5").update(buf).digest("hex");

  if (check) {
    let failed = false;
    for (const out of OUT_FILES) {
      const rel = out.slice(root.length + 1).replace(/\\/g, "/");
      if (!existsSync(out)) {
        // 不能"文件缺失就跳过"：`cli/ui/index.html` 缺失会让 `include_str!` 编译失败。
        console.error(`✗ ${rel} 不存在（片段已就绪，请运行 node scripts/build_ui.mjs）`);
        failed = true;
        continue;
      }
      const actual = readFileSync(out);
      if (!buf.equals(actual)) {
        console.error(`✗ ${rel} 与片段拼接结果不一致\n  ${describeDiff(buf, actual)}`);
        console.error("  → 改了片段就运行 node scripts/build_ui.mjs 重新生成，不要手改生成物。");
        failed = true;
      }
    }
    if (failed) process.exit(1);
    console.log(`✓ 生成物与提交物一致（${parts.length} 个片段，${lines} 行，md5 ${md5}）`);
    return;
  }

  for (const out of OUT_FILES) {
    writeFileSync(out, buf);
  }
  // 读回自检：防外部因素（锁、只读、并发）让两份产出不一致。
  for (const out of OUT_FILES) {
    const readback = readFileSync(out);
    if (!readback.equals(buf)) {
      console.error(`✗ 写入后读回不一致：${out}`);
      process.exit(1);
    }
  }
  const copies = OUT_FILES.map((p) => readFileSync(p));
  if (!copies[0].equals(copies[1])) {
    console.error("✗ 两份生成物不一致（同一份 buf 写出，不应发生）");
    process.exit(1);
  }
  console.log(`✓ 已生成两份（${lines} 行，md5 ${md5}）`);
}

main();
