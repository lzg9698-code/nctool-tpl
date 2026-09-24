#!/usr/bin/env node
/**
 * CLI ↔ Web UI 生成选项对拍门禁。
 *
 * 背景：生成选项有两条入口，但必须映射到同一份 `GenerationOptions`（core 的
 * 结构体），否则同一份参数在命令行与界面上会得到不同的 G-code —— 本项目对
 * 「静默不一致」零容忍。
 *   - CLI：RenderArgs 的 --line-numbers/--line-step/--max-line/--header/
 *     --strip-blank/--ascii/--lenient（commands/render.rs 1:1 赋给结构体）
 *   - Web：POST /api/render 的 options 对象，前端由 normalizeOpts() 归一
 *     （ui/index.html，cli/ui/index.html 是它的副本，两份必须同步改）
 *
 * 隐患修复（2026-09-22）：`--line-step` / `--max-line` 曾只存在于 Web UI，
 * CLI 无法复现带自定义步进的输出。现两侧共用同一份 fixture，任一漂移即红：
 *   - Rust 侧：cli/src/server.rs::option_mapping_matches_shared_fixture 消费
 *     fixture 的 json/expect/cli 三节；
 *   - 前端侧：本脚本把 fixture 的 json 喂给从 HTML 抠出的 normalizeOpts，
 *     断言结果与 expect 一致。
 *
 * 用法：node scripts/check_option_parity.mjs
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const fixture = JSON.parse(
  readFileSync(join(root, "scripts", "option_parity_cases.json"), "utf8"),
);

const UI_FILES = ["ui/index.html", "cli/ui/index.html"];

/** 从 HTML 抠出函数源码（与 check_param_parity.mjs 同一手法，避免第三份实现）。 */
function extractFn(src, name, file) {
  const marker = `function ${name}(`;
  const start = src.indexOf(marker);
  if (start < 0) throw new Error(`${file}: 未找到 ${marker}`);
  let depth = 0;
  for (let i = src.indexOf("{", start); i < src.length; i++) {
    const ch = src[i];
    if (ch === "{") depth++;
    else if (ch === "}") {
      depth--;
      if (depth === 0) return src.slice(start, i + 1);
    }
  }
  throw new Error(`${file}: ${name} 的花括号不配平`);
}

function loadNormalizeOpts(file) {
  const src = readFileSync(join(root, file), "utf8");
  const body = extractFn(src, "normalizeOpts", file);
  // eslint-disable-next-line no-new-func
  return new Function(`${body}\nreturn normalizeOpts;`)();
}

// normalizeOpts 的输出字段名 -> fixture expect 的字段名
const FIELD_MAP = {
  lineNumbers: "line_numbers",
  lineStep: "line_number_step",
  maxLine: "max_line_number",
  addHeader: "add_header_comment",
  stripBlank: "strip_blank_lines",
  ascii: "ascii_only",
  lenient: "lenient",
};

let failed = 0;

for (const file of UI_FILES) {
  let normalize;
  try {
    normalize = loadNormalizeOpts(file);
  } catch (e) {
    console.error(`✗ ${file}: 载入 normalizeOpts 失败 —— ${e.message}`);
    process.exit(1);
  }

  fixture.cases.forEach((c, i) => {
    const got = normalize({ ...c.json });
    const e = c.expect;

    if (got.format !== "gcode") {
      failed++;
      console.error(`✗ ${file} case #${i} ${c.name}: format 应为 gcode，实际 ${got.format}`);
    }

    for (const [jsKey, expectKey] of Object.entries(FIELD_MAP)) {
      if (got[jsKey] !== e[expectKey]) {
        failed++;
        console.error(
          `✗ ${file} case #${i} ${c.name}: ${jsKey} 期望 ${e[expectKey]}，实际 ${got[jsKey]}`,
        );
      }
    }
  });
}

if (failed > 0) {
  console.error(`\n✗ 生成选项对拍失败：${failed} 处不一致`);
  process.exit(1);
}
console.log(
  `✓ 生成选项对拍通过：${fixture.cases.length} 组用例 × ${UI_FILES.length} 份 UI 与后端映射一致`,
);
