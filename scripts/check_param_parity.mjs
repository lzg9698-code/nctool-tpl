#!/usr/bin/env node
/**
 * `--param` 取值归一的 Rust / JS 对拍门禁。
 *
 * 背景：CLI 的 `--param` 与 Web UI 的输入框走两条通道，但必须产出同一结果，
 * 否则同一份参数在命令行与界面上会得到不同的 G-code —— 这正是本项目零容忍的
 * 「静默错误」。归一规则却天然有两份实现：
 *   - 后端 cli/src/args.rs :: infer_param_value / coerce_param_value
 *   - 前端 ui/index.html  :: inferParamValue  / coerceParamValue
 *     （cli/ui/index.html 是它的副本，两份必须同步改）
 *
 * 做法：用例与期望值放在 scripts/param_parity_cases.json 这一份 fixture 里，
 * 后端在 cargo test 中消费它（param_coercion_matches_shared_fixture），
 * 本脚本对前端消费同一份。任何一侧改了语义而没同步 fixture，另一侧立刻红。
 *
 * 用法：node scripts/check_param_parity.mjs
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const fixture = JSON.parse(
  readFileSync(join(root, "scripts", "param_parity_cases.json"), "utf8"),
);

/** 前端两份 UI 实现，任一漂移都要拦住 */
const UI_FILES = ["ui/index.html", "cli/ui/index.html"];

/**
 * 从 HTML 里抠出函数源码，而不是在脚本里重写一遍 —— 重写就等于第三份实现，
 * 本身就违背「单一真源」。用花括号配平定位结尾（这几个函数体内没有会干扰
 * 配平的字符串/正则里的花括号，故朴素配平足够）。
 */
function extractFn(src, name, file) {
  const marker = `function ${name}(`;
  const start = src.indexOf(marker);
  if (start < 0) throw new Error(`${file}: 未找到 ${marker}`);
  let depth = 0;
  let end = -1;
  for (let i = src.indexOf("{", start); i < src.length; i++) {
    const ch = src[i];
    if (ch === "{") depth++;
    else if (ch === "}") {
      depth--;
      if (depth === 0) {
        end = i + 1;
        break;
      }
    }
  }
  if (end < 0) throw new Error(`${file}: ${name} 的花括号不配平`);
  return src.slice(start, end);
}

function loadImpl(file) {
  const src = readFileSync(join(root, file), "utf8");
  const names = ["bareValue", "optionValues", "inferParamValue", "coerceParamValue"];
  const body = names.map((n) => extractFn(src, n, file)).join("\n\n");
  // eslint-disable-next-line no-new-func
  const factory = new Function(`${body}\nreturn coerceParamValue;`);
  return factory();
}

/** 前端返回的裸值 -> 与 fixture 一致的带标签形式 */
function describe(v) {
  if (typeof v === "boolean") return { type: "bool", value: v };
  if (typeof v === "number") return { type: "number", value: v };
  if (typeof v === "string") return { type: "string", value: v };
  return { type: `<${typeof v}>`, value: String(v) };
}

function show(v) {
  return `${v.type}(${JSON.stringify(v.value)})`;
}

let failed = 0;
const resultsByFile = new Map();

for (const file of UI_FILES) {
  let coerce;
  try {
    coerce = loadImpl(file);
  } catch (e) {
    console.error(`✗ ${file}: 载入失败 —— ${e.message}`);
    process.exit(1);
  }

  const results = [];
  fixture.cases.forEach((c, i) => {
    const got = describe(coerce(c.spec, c.input));
    results.push(got);
    const want = c.expect;
    if (got.type !== want.type || !Object.is(got.value, want.value)) {
      failed++;
      console.error(
        `✗ ${file} case #${i} input=${JSON.stringify(c.input)} ` +
          `kind=${c.spec.kind}${c.spec.options ? " +options" : ""}\n` +
          `    got  ${show(got)}\n    want ${show(want)}` +
          (c.note ? `\n    note ${c.note}` : ""),
      );
    }
  });
  resultsByFile.set(file, results);
}

/* 两份 UI 互为镜像：即使都过了 fixture，也要确认它们没有各自漂移 */
const [a, b] = UI_FILES.map((f) => resultsByFile.get(f));
a.forEach((got, i) => {
  const other = b[i];
  if (got.type !== other.type || !Object.is(got.value, other.value)) {
    failed++;
    console.error(
      `✗ 两份 UI 不一致 case #${i} input=${JSON.stringify(fixture.cases[i].input)}: ` +
        `${UI_FILES[0]} -> ${show(got)}，${UI_FILES[1]} -> ${show(other)}`,
    );
  }
});

if (failed) {
  console.error(`\n${failed} 项不一致：改任一侧请同步改 scripts/param_parity_cases.json 与另一侧实现。`);
  process.exit(1);
}
console.log(
  `✓ ${fixture.cases.length} 个用例 × ${UI_FILES.length} 份 UI 与后端归一规则一致`,
);
