#!/usr/bin/env node
/**
 * 端点契约 ↔ GUI（Tauri command）封装 对拍门禁。
 *
 * 背景：`scripts/api_routes.json` 是端点契约的单一来源，目前已有两个消费者 ——
 * Rust 侧 `cli/src/server.rs::api_routes_are_routable`（断言每条都能被 `route()` 路由）
 * 与 JS 侧 `scripts/check_api_parity.mjs`（断言 Web UI 的 `/api/...` 字面量都在 fixture 里）。
 * 桌面 GUI（nctool-gui）是**第三个交付面**：它走 Tauri `invoke`，不含 `/api/` 字面量，
 * 因此 `check_api_parity.mjs` 看不到它 —— 于是「GUI 注册了一个契约里没有的命令」
 * 或「契约声明了某端点但 GUI 忘了接线」这两类漂移此前无人拦截。
 *
 * 本脚本补上这一面，做**双向**校验（漏任一侧即红）：
 *   1. 每条 routes[] 都必须带 `gui` 键；为 null 时必须给非空 `gui_note`
 *      （防「新增端点忘了考虑 GUI」—— 这正是本门禁存在的首要理由）；
 *   2. 从 `gui/src/main.rs` 的 `invoke_handler!(generate_handler![...])` 抽出**实际注册**
 *      的 command 名（抽不到即显式报错，不静默通过）；
 *   3. 断言集合相等（双向都报）：
 *        { routes[].gui 中非 null 的值 } ∪ { gui_only[].command }  ==  实际注册集合；
 *   4. 对每条 `gui` 非 null 的 route：在 `gui/src/commands/*.rs` 里必须能找到匹配的
 *      `run_route(...)` **生产**调用点 —— 该调用点须同时含该 route 的 `"<METHOD>"` 字面量与
 *      `"<path>"`（或 `covers` 前缀）字面量。`get_template` 动态拼路径，靠
 *      `covers: "/api/templates/"` 命中，故调用点取「run_route 行 + 前若干行」的窗口
 *      （`format!` 拼路径在上一行）。
 *      **只认生产调用点**：`#[cfg(test)]` 项（测试模块、测试专用 fn）内的行一律先剔除，
 *      否则测试里同路径的 `run_route(&ctx(), "POST", "/api/render", ...)` 会把生产路径
 *      改错"兜住"而假绿（QA 实测复现的盲区）。同时排除 `fn run_route` **定义行**，
 *      避免它被当成一个调用窗口、让"抽不到调用点即报错"的守卫失效。
 *   5. 每个 command 名必须在 `gui/frontend/src/api/nctool.ts` 里以字符串字面量出现
 *      （后端有能力 ≠ 前端能调到，这条把前端接线也钉住）；
 *   6. 每条 `gui_only` 条目必须有非空 `reason`（豁免必须写明原因，对齐 frontend_only）。
 *
 * 用法：node scripts/check_gui_parity.mjs
 */
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

const FIXTURE = join(root, "scripts", "api_routes.json");
const MAIN_RS = join(root, "gui", "src", "main.rs");
const COMMANDS_DIR = join(root, "gui", "src", "commands");
const FRONTEND_API = join(root, "gui", "frontend", "src", "api", "nctool.ts");

/** 调用点窗口回看行数：容纳 `let path = format!(...)` 这类紧邻 run_route 的拼路径行。 */
const CALL_WINDOW_LOOKBACK = 6;

/**
 * 把注释与字符串字面量替换为等长空格（保留列数）。
 *
 * 仅用于**括号配平**：`#[cfg(test)]` 区间的判定要在"看不到字符串花括号"的文本上做，
 * 否则测试里 `r#"{"x":21}"#` 这类字面量会干扰 `{` / `}` 计数。处理行注释、块注释、
 * 普通字符串与原始字符串（`r"..."` / `r#"..."#`）。
 *
 * **不抹字符字面量**（与 `scripts/check_coverage_caliber.py` 的 `strip_noise` 同款取舍）：
 * 生命周期 `'a` 与字符字面量 `'{'` / `'}'` 在纯文本上难以可靠区分（`'` 后跟标识符还是
 * 后跟单个字符），而测试代码里出现含花括号的字符字面量概率极低。真遇到时下面的括号配平
 * 会失败并**回退到「到文件末尾」**（`end = clean.length - 1`），属保守失败、不会静默错判。
 *
 * @param {string[]} lines 源文件按行切分后的内容
 * @returns {string[]} 等长、已抹掉注释与字符串的副本
 */
function stripNoise(lines) {
  const out = [];
  let inBlock = false;
  let raw = null; // 原始字符串的结束定界符（如 `"` 或 `"##`）
  for (const line of lines) {
    const buf = [...line];
    let i = 0;
    const n = line.length;
    while (i < n) {
      if (inBlock) {
        if (line.startsWith("*/", i)) {
          buf[i] = buf[i + 1] = " ";
          i += 2;
          inBlock = false;
        } else {
          buf[i] = " ";
          i += 1;
        }
        continue;
      }
      if (raw !== null) {
        if (line.startsWith(raw, i)) {
          for (let k = i; k < Math.min(i + raw.length, n); k++) buf[k] = " ";
          i += raw.length;
          raw = null;
        } else {
          buf[i] = " ";
          i += 1;
        }
        continue;
      }
      const ch = line[i];
      if (ch === "/" && i + 1 < n && line[i + 1] === "/") {
        for (let k = i; k < n; k++) buf[k] = " ";
        break;
      }
      if (ch === "/" && i + 1 < n && line[i + 1] === "*") {
        buf[i] = buf[i + 1] = " ";
        i += 2;
        inBlock = true;
        continue;
      }
      // 原始字符串 `r"..."` / `r#"..."#`：`r` 不能是标识符的一部分
      if (ch === "r" && (i === 0 || !/[A-Za-z0-9_]/.test(line[i - 1]))) {
        let j = i + 1;
        let hashes = 0;
        while (j < n && line[j] === "#") {
          hashes += 1;
          j += 1;
        }
        if (j < n && line[j] === '"') {
          raw = '"' + "#".repeat(hashes);
          for (let k = i; k < Math.min(j + 1, n); k++) buf[k] = " ";
          i = j + 1;
          continue;
        }
      }
      if (ch === '"') {
        buf[i] = " ";
        i += 1;
        while (i < n) {
          if (line[i] === "\\") {
            buf[i] = " ";
            if (i + 1 < n) buf[i + 1] = " ";
            i += 2;
            continue;
          }
          if (line[i] === '"') {
            buf[i] = " ";
            i += 1;
            break;
          }
          buf[i] = " ";
          i += 1;
        }
        continue;
      }
      i += 1;
    }
    out.push(buf.join(""));
  }
  return out;
}

/**
 * 返回源文件里属于 `#[cfg(test)]` 项的**行号集合**（0 起）。
 *
 * 只认 `#[cfg(test)]` 紧跟的项：有 `{` 花括号体（`mod tests { ... }` /
 * `fn helper() { ... }`）就括号配平到匹配的 `}`；无体项（`use ...;`）只到 `;`。
 *
 * **为什么必须剔除**：调用点校验只看"生产"调用点。测试模块里同样会写
 * `run_route(&ctx(), "POST", "/api/render", ...)`，若把这些行算作调用窗口，
 * 生产路径改错（如把 `"/api/render"` 改成 `"/api/renderX"`）仍会被测试里的
 * 同路径调用"兜住"，门禁假绿 —— 正是 QA 实测复现的盲区（P1-1）。
 *
 * @param {string[]} lines 源文件按行切分后的内容
 * @returns {Set<number>} 位于 `#[cfg(test)]` 项内的行下标（0 起）
 */
function cfgTestLineSet(lines) {
  const clean = stripNoise(lines);
  const marked = new Set();
  for (let idx = 0; idx < clean.length; idx++) {
    const stripped = clean[idx].trim();
    if (!(stripped.startsWith("#[") && stripped.includes("cfg(") && stripped.includes("test"))) {
      continue;
    }
    // 跳过连续的属性行（`#[cfg(test)]` + `#[allow(...)]` 等）
    let j = idx;
    while (j < clean.length && clean[j].trim().startsWith("#[")) j += 1;
    if (j >= clean.length) {
      for (let k = idx; k < clean.length; k++) marked.add(k);
      continue;
    }
    // 找项的起始 `{`（有体）或 `;`（无体）
    let brace = -1;
    let k = j;
    while (k < clean.length && k < j + 5) {
      if (clean[k].includes("{")) {
        brace = k;
        break;
      }
      if (clean[k].includes(";")) break;
      k += 1;
    }
    let end;
    if (brace === -1) {
      end = Math.min(k, clean.length - 1);
    } else {
      // 括号配平
      let depth = 0;
      end = clean.length - 1;
      for (let m = brace; m < clean.length; m++) {
        for (const ch of clean[m]) {
          if (ch === "{") depth += 1;
          else if (ch === "}") depth -= 1;
        }
        if (depth === 0) {
          end = m;
          break;
        }
      }
    }
    for (let k2 = idx; k2 <= end; k2++) marked.add(k2);
  }
  return marked;
}

/**
 * 判断某行是否是 `run_route` 的**函数定义**（而非调用）。
 *
 * `shared.rs` 里 `pub(crate) fn run_route(` 含子串 `run_route(`，会被
 * `line.includes("run_route(")` 误当成一个"调用窗口"。后果：即便把所有**真实**
 * 调用点都删掉，"抽不到调用点就报错"的守卫也永远不触发（P2-2）。
 * 这里按"行内出现 `fn run_route`"排除定义行。
 *
 * @param {string} line 源码行
 * @returns {boolean} true 表示该行是函数定义而非调用
 */
function isRunRouteDefinition(line) {
  return /\bfn\s+run_route\b/.test(line);
}

const fixture = JSON.parse(readFileSync(FIXTURE, "utf8"));

let failed = 0;
/** 记录一处契约漂移（累加失败数并打印）。 */
function fail(msg) {
  failed++;
  console.error(`✗ ${msg}`);
}

// ---------------------------------------------------------------------------
// ① 从 gui/src/main.rs 抽出实际注册的 command 名
// ---------------------------------------------------------------------------
const mainSrc = readFileSync(MAIN_RS, "utf8");
const handlerBlock = mainSrc.match(/generate_handler!\s*\[([\s\S]*?)\]/);
if (!handlerBlock) {
  console.error(
    "✗ gui/src/main.rs: 无法定位 invoke_handler!(tauri::generate_handler![...])，正则或文件结构可能已变",
  );
  process.exit(1);
}
const registered = new Set(
  [...handlerBlock[1].matchAll(/commands::([A-Za-z0-9_]+)::([A-Za-z0-9_]+)/g)].map(
    (m) => m[2],
  ),
);
if (registered.size === 0) {
  console.error(
    "✗ gui/src/main.rs: generate_handler![...] 内未提取到任何 `commands::<mod>::<fn>`，正则或文件结构可能已变",
  );
  process.exit(1);
}

// ---------------------------------------------------------------------------
// ② 汇总契约声明的 command 集合
// ---------------------------------------------------------------------------
const routes = fixture.routes || [];
const guiOnly = fixture.gui_only || [];

/** routes[].gui 里非 null 的 command 名。 */
const declaredFromRoutes = new Set();
routes.forEach((r, i) => {
  const tag = `${r.method ?? "?"} ${r.path ?? "?"}`;
  if (!("gui" in r)) {
    fail(
      `route #${i} ${tag}: 缺少 gui 键 —— 新增端点必须显式声明由哪个 GUI command 封装，` +
        `或用 null + gui_note 说明为何 GUI 有意不暴露`,
    );
    return;
  }
  if (r.gui === null) {
    if (typeof r.gui_note !== "string" || r.gui_note.trim() === "") {
      fail(`route #${i} ${tag}: gui 为 null 但缺少非空 gui_note —— 豁免必须写明原因`);
    }
    return;
  }
  if (typeof r.gui !== "string" || r.gui.trim() === "") {
    fail(
      `route #${i} ${tag}: gui 必须是 command 名字符串或 null，实际为 ${JSON.stringify(r.gui)}`,
    );
    return;
  }
  declaredFromRoutes.add(r.gui);
});

/** gui_only[].command（不经 route() 的 GUI 专有命令）。 */
const declaredFromGuiOnly = new Set();
guiOnly.forEach((g, i) => {
  if (typeof g.command !== "string" || g.command.trim() === "") {
    fail(`gui_only #${i}: 缺少非空 command`);
  } else {
    declaredFromGuiOnly.add(g.command);
  }
  if (typeof g.reason !== "string" || g.reason.trim() === "") {
    fail(
      `gui_only #${i}（${g.command ?? "?"}）: 缺少非空 reason —— 豁免必须写明原因与解除条件`,
    );
  }
});

const declared = new Set([...declaredFromRoutes, ...declaredFromGuiOnly]);

// ---------------------------------------------------------------------------
// ③ 集合相等（双向都报）
// ---------------------------------------------------------------------------
for (const c of [...declared].sort()) {
  if (!registered.has(c)) {
    fail(
      `契约声明了 command "${c}"，但 gui/src/main.rs 的 generate_handler![...] 未注册 —— ` +
        `要么补注册，要么从 api_routes.json 移除`,
    );
  }
}
for (const c of [...registered].sort()) {
  if (!declared.has(c)) {
    fail(
      `GUI 注册了 command "${c}"，但 scripts/api_routes.json 未登记 —— ` +
        `请在对应 route 的 gui 键（封装端点时）或顶层 gui_only（不经 route() 时）登记`,
    );
  }
}

// ---------------------------------------------------------------------------
// ④ 每条 gui 非 null 的 route 必须有匹配的 run_route 调用点
// ---------------------------------------------------------------------------
const cmdFiles = readdirSync(COMMANDS_DIR).filter((f) => f.endsWith(".rs"));
/** 所有 `run_route(...)` **生产调用**窗口：{ file, line, text }。 */
const callWindows = [];
/** 被 `#[cfg(test)]` 剔除的行数（仅用于日志，证明剔除真的生效）。 */
let testLinesSkipped = 0;
for (const f of cmdFiles.sort()) {
  const lines = readFileSync(join(COMMANDS_DIR, f), "utf8").split(/\r?\n/);
  const testLines = cfgTestLineSet(lines);
  testLinesSkipped += testLines.size;
  lines.forEach((line, i) => {
    if (!line.includes("run_route(")) return;
    // ① 只认**生产**调用点：测试模块里的同路径调用不得充当"命中"
    //    （否则生产路径改错仍会被测试兜住 → 门禁盲区，P1-1）
    if (testLines.has(i)) return;
    // ② 排除函数定义行 `pub(crate) fn run_route(`：它不是调用点，
    //    留着会让"抽不到调用点就报错"的守卫失效（P2-2）
    if (isRunRouteDefinition(line)) return;
    // ⚠️ 判定边界（已知、有意保留）：窗口是「run_route 行 + 前 CALL_WINDOW_LOOKBACK 行」的
    //    文本块，命中条件是「块内**任意**位置含方法字面量」且「块内**任意**位置含路径字面量」
    //    —— 两者**各自独立** `includes`，不要求同行或相邻。故理论上可能出现"方法字面量来自
    //    某行、路径字面量来自窗口内另一行"的**假命中**（把两条不相关调用拼成一条）。影响面：
    //    只可能**假绿**（漏报一处路径漂移），**不会假红**（真实调用点仍在窗口内、不会误报）；
    //    当前 6 行窗口内无此实例。缩小窗口会误伤 `let path = format!(...)` 这类紧邻 run_route
    //    的拼路径行，故维持现状并在此注明边界。
    const start = Math.max(0, i - CALL_WINDOW_LOOKBACK);
    // 窗口同样只取**生产**行：回看窗口里若混入测试行，等于变相放宽判定
    const windowLines = [];
    for (let k = start; k <= i; k++) {
      windowLines.push(testLines.has(k) ? "" : lines[k]);
    }
    callWindows.push({ file: f, line: i + 1, text: windowLines.join("\n") });
  });
}
if (callWindows.length === 0) {
  console.error(
    `✗ gui/src/commands/*.rs: 未找到任何 run_route(...) 生产调用点，正则或文件结构可能已变`,
  );
  process.exit(1);
}

for (const r of routes) {
  if (typeof r.gui !== "string" || r.gui.trim() === "") continue;
  const methodLit = `"${r.method}"`;
  // 动态拼路径的 route（带 covers）用**前缀字面量**命中 —— 源里是
  // `format!("/api/templates/{}", …)`，故前缀只含开引号、不含收尾引号；
  // 其余用完整路径字面量（含收尾引号，避免 "/api/templates" 误配 "/api/templates/…"）。
  const pathLits = [`"${r.path}"`];
  if (typeof r.covers === "string" && r.covers.trim() !== "") pathLits.push(`"${r.covers}`);

  const hit = callWindows.find(
    (w) => w.text.includes(methodLit) && pathLits.some((p) => w.text.includes(p)),
  );
  if (!hit) {
    fail(
      `route ${r.method} ${r.path}（gui="${r.gui}"）: gui/src/commands/*.rs 里找不到匹配的 ` +
        `run_route 调用点 —— 该调用点须同时含 ${methodLit} 与 ${pathLits.join(" 或 ")}`,
    );
  }
}

// ---------------------------------------------------------------------------
// ⑤ 每个 command 名必须在 GUI 前端 API 封装里接线
// ---------------------------------------------------------------------------
const apiSrc = readFileSync(FRONTEND_API, "utf8");
for (const c of [...declared].sort()) {
  // 前端统一经 `call<T>(cmd, args)` 包装再 invoke；命令名以字符串字面量出现
  // （`call<...>("<name>"` / `invoke("<name>"` 两种形态都接受）。
  if (!apiSrc.includes(`"${c}"`)) {
    fail(
      `command "${c}" 未在 gui/frontend/src/api/nctool.ts 里接线 —— ` +
        `后端有能力 ≠ 前端能调到，请补 API 封装`,
    );
  }
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------
if (failed) {
  console.error(
    `\n${failed} 处 GUI 契约漂移：改端点 / command 请同步 scripts/api_routes.json、` +
      `gui/src/main.rs、gui/frontend/src/api/nctool.ts。`,
  );
  process.exit(1);
}

const routeWrapped = [...declaredFromRoutes].sort();
const exemptRoutes = routes
  .filter((r) => r.gui === null)
  .map((r) => `${r.method} ${r.path}`);
console.log(
  `  gui/src/main.rs: 提取到 ${registered.size} 个已注册 command`,
);
console.log(
  `  routes[]: ${routes.length} 条端点，其中 ${routeWrapped.length} 条由 GUI 封装，` +
    `${exemptRoutes.length} 条 GUI 有意不暴露（已写 gui_note）`,
);
console.log(`  gui_only: ${guiOnly.length} 条 GUI 专有命令（已写 reason）`);
console.log(
  `  gui/src/commands/*.rs: 校验了 ${callWindows.length} 个 run_route **生产**调用点` +
    `（另有 ${testLinesSkipped} 行位于 #[cfg(test)] 段，已排除）`,
);
console.log(
  `✓ GUI command 面与 scripts/api_routes.json 一致（封装端点 ${routeWrapped.length} 条 + ` +
    `GUI 专有 ${guiOnly.length} 条 == 注册 ${registered.size} 个 command）`,
);
