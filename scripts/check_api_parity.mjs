#!/usr/bin/env node
/**
 * 前后端「接口集合」对拍门禁。
 *
 * 背景：后端路由是一条 `match (method, path)`（cli/src/server.rs::route），前端是
 * 一组 API 封装（ui/index.html 的 `API.*`，cli/ui/index.html 是其副本）。两份各自
 * 演进时没有任何机制提示「前端调的接口后端并没有」—— 第三轮的 P1-8（分类表四份
 * 导致漏掉 grooving）就是同一结构在另一处的产物。
 *
 * 与 `--param` 对拍的区别：那边对拍的是**语义**（同一输入产出同一结果），这边对拍
 * 的是**契约**（接口集合一致）。两者共用同一个思路：集合放在
 * scripts/api_routes.json 单一来源里，后端 cargo test 消费它，本脚本对前端消费它。
 *
 * 检查内容：
 *   1. 前端出现的每个 `/api/...` 字符串字面量，都必须在本文件的 routes 或
 *      frontend_only 里登记（漏登记 = 契约漂移，直接红）；
 *   2. frontend_only 里的豁免项必须写明 reason（防止无限期豁免）。
 *
 * 用法：node scripts/check_api_parity.mjs
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const fixture = JSON.parse(
  readFileSync(join(root, "scripts", "api_routes.json"), "utf8"),
);

const UI_FILES = ["ui/index.html", "cli/ui/index.html"];

/** 路径归一：去掉结尾斜杠，让 "/api/templates/" 与 "/api/templates" 视为同一条 */
const norm = (p) => (p.endsWith("/") ? p.slice(0, -1) : p);

const declared = new Map();
for (const r of fixture.routes || []) {
  declared.set(norm(r.path), r.method);
  if (r.covers) declared.set(norm(r.covers), r.method);
}
for (const r of fixture.frontend_only || []) {
  if (!r.reason) {
    console.error(`✗ frontend_only 的 ${r.method} ${r.path} 缺少 reason：豁免必须写明原因与解除条件`);
    process.exit(1);
  }
  declared.set(norm(r.path), r.method);
}

let failed = 0;

for (const file of UI_FILES) {
  const src = readFileSync(join(root, file), "utf8");
  // 只取字符串字面量里的路径：注释里写的 `POST /api/x` 不算调用点
  const literals = new Set(
    [...src.matchAll(/"(\/api\/[A-Za-z0-9_\-/{}]*)"/g)].map((m) => m[1]),
  );

  if (literals.size === 0) {
    console.error(`✗ ${file}: 未提取到任何 /api/ 路径字面量，正则或文件结构可能已变`);
    process.exit(1);
  }

  for (const lit of [...literals].sort()) {
    if (!declared.has(norm(lit))) {
      failed++;
      console.error(
        `✗ ${file}: 前端使用了 ${lit}，但 scripts/api_routes.json 未登记。\n` +
          `    新增端点请同时改：后端 route() 路由臂 + 本文件 + 前端 API 封装。`,
      );
    }
  }
  console.log(`  ${file}: 检查了 ${literals.size} 个接口路径`);
}

if (failed) {
  console.error(`\n${failed} 处契约漂移：改接口请同步 scripts/api_routes.json。`);
  process.exit(1);
}

const exempt = (fixture.frontend_only || []).map((r) => `${r.method} ${r.path}`);
console.log(
  `✓ 前端接口集合与 scripts/api_routes.json 一致（后端 ${(fixture.routes || []).length} 条` +
    (exempt.length ? `，前端本地豁免 ${exempt.length} 条：${exempt.join("、")}` : "") +
    "）",
);
