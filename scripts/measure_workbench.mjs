#!/usr/bin/env node
// Independent measurement driver for the workbench derived-value read path.
//
// Bundles scripts/measure_workbench.harness.ts with esbuild (the real ui/src/lib
// modules, with `./api` substituted for an in-memory mock so no HTTP server is
// started), runs it under plain Node and records the emitted medians as JSON in
// the Git-ignored target/agent-preview/optimization/ tree.
//
// Usage:
//   node scripts/measure_workbench.mjs --label before
//   node scripts/measure_workbench.mjs --label after --sections recompute
//   node scripts/measure_workbench.mjs --compare before after
//
// This never starts the WebUI, never touches a user workspace and never writes
// into version-controlled files.

import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const require = createRequire(import.meta.url);
const REPO_ROOT = path.resolve(import.meta.dirname, "..");
const OUT_DIR = path.join(REPO_ROOT, "target", "agent-preview", "optimization");
const HARNESS = path.join(REPO_ROOT, "scripts", "measure_workbench.harness.ts");
const BUNDLE = path.join(OUT_DIR, "harness", "measure_workbench.bundle.mjs");
const PREFIX = "WORKBENCH_METRIC_JSON ";

// esbuild ships with Vite; resolve it from the ui workspace.
const esbuild = require(
  path.join(REPO_ROOT, "ui", "node_modules", "esbuild", "lib", "main.js"),
);

// In-memory stand-in for ui/src/lib/api.ts: the harness drives the mock through
// globalThis.__WORKBENCH_ACTION__, so no HTTP request and no server is involved.
const API_MOCK = `
export class ApiError extends Error {
  constructor(detail) {
    super((detail && detail.message) || "请求失败");
    this.detail = detail;
  }
}
export async function api() {
  throw new Error("api() is not used by the measurement harness");
}
export async function action(id, input) {
  const mock = globalThis.__WORKBENCH_ACTION__;
  if (!mock) throw new Error("workbench catalog mock is not installed");
  if (id === "template.catalog") return mock.catalog();
  return mock.read(input);
}
`;

const apiMockPlugin = {
  name: "workbench-api-mock",
  setup(build) {
    build.onResolve({ filter: /(^|\/)api$/ }, (args) => {
      if (/templateCatalog\.ts$/.test(args.importer))
        return { path: "workbench-api-mock", namespace: "workbench-mock" };
      return null;
    });
    build.onLoad({ filter: /.*/, namespace: "workbench-mock" }, () => ({
      contents: API_MOCK,
      loader: "js",
    }));
  },
};

async function buildBundle() {
  fs.mkdirSync(path.dirname(BUNDLE), { recursive: true });
  await esbuild.build({
    entryPoints: [HARNESS],
    bundle: true,
    platform: "node",
    format: "esm",
    target: "node20",
    outfile: BUNDLE,
    plugins: [apiMockPlugin],
    logLevel: "warning",
  });
  return BUNDLE;
}

function gitRevision() {
  const result = spawnSync("git", ["rev-parse", "HEAD"], {
    cwd: REPO_ROOT,
    encoding: "utf8",
  });
  return result.status === 0 ? result.stdout.trim() : "unknown";
}

function nodeVersion() {
  return process.version;
}

function runHarness(options) {
  const env = {
    ...process.env,
    WORKBENCH_COUNTS: options.counts.join(","),
    WORKBENCH_SIZES: options.sizes.join(","),
    WORKBENCH_ITERATIONS: String(options.iterations),
    WORKBENCH_SECTIONS: options.sections.join(","),
    WORKBENCH_CATALOG_LIMIT: String(options.limit),
  };
  const completed = spawnSync(process.execPath, [BUNDLE], {
    cwd: REPO_ROOT,
    env,
    encoding: "utf8",
  });
  if (completed.status !== 0) {
    process.stderr.write(completed.stdout || "");
    process.stderr.write(completed.stderr || "");
    throw new Error(`harness exited with code ${completed.status}`);
  }
  const metrics = [];
  for (const line of (completed.stdout || "").split(/\r?\n/)) {
    if (line.startsWith(PREFIX))
      metrics.push(JSON.parse(line.slice(PREFIX.length)));
  }
  return metrics;
}

async function measure(options) {
  await buildBundle();
  const metrics = runHarness(options);
  const sections = {};
  for (const metric of metrics) {
    (sections[metric.section] ||= []).push(metric);
  }
  const payload = {
    label: options.label,
    sections: options.sections,
    counts: options.counts,
    sizes: options.sizes,
    iterations: options.iterations,
    catalog_limit: options.limit,
    git_revision: gitRevision(),
    node: nodeVersion(),
    platform: process.platform,
    arch: process.arch,
    recorded_at: new Date().toISOString(),
    results: sections,
  };
  fs.mkdirSync(OUT_DIR, { recursive: true });
  const target = path.join(OUT_DIR, `workbench-${options.label}.json`);
  fs.writeFileSync(target, JSON.stringify(payload, null, 2) + "\n", "utf8");
  process.stdout.write(JSON.stringify(payload, null, 2) + "\n");
  process.stdout.write(`[measure] recorded ${target}\n`);
  return payload;
}

function load(label) {
  const file = path.join(OUT_DIR, `workbench-${label}.json`);
  if (!fs.existsSync(file))
    throw new Error(`missing report ${file}; measure it first`);
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function compare(labels) {
  if (labels.length !== 2) throw new Error("--compare needs BEFORE AFTER");
  const [beforeLabel, afterLabel] = labels;
  const before = load(beforeLabel);
  const after = load(afterLabel);
  process.stdout.write(
    `before=${beforeLabel} after=${afterLabel}\n`,
  );
  for (const section of before.sections) {
    const b = before.results[section] || [];
    const a = after.results[section] || [];
    if (!b.length || !a.length) continue;
    const numeric = Object.keys(b[0]).filter(
      (key) => typeof b[0][key] === "number",
    );
    process.stdout.write(`\n== ${section} ==\n`);
    for (let i = 0; i < b.length; i += 1) {
      const label = [b[i].count, b[i].size].filter((v) => v !== undefined);
      process.stdout.write(`  ${label.join("x") || "-"}:\n`);
      for (const key of numeric) {
        const bv = b[i][key];
        const av = a[i] ? a[i][key] : NaN;
        const ratio = bv ? av / bv : NaN;
        process.stdout.write(
          `    ${key.padEnd(26)} ${String(bv).padStart(12)} -> ${String(
            av,
          ).padStart(12)}  ${ratio.toFixed(2)}x\n`,
        );
      }
    }
  }
}

function parseList(raw, fallback) {
  if (raw === undefined) return fallback;
  return raw
    .split(",")
    .map((part) => Number(part.trim()))
    .filter((value) => Number.isFinite(value));
}

function parseArgs(argv) {
  const options = {
    label: undefined,
    compare: undefined,
    counts: [10, 100],
    sizes: [10240, 102400],
    iterations: 30,
    limit: 8,
    sections: ["recompute", "filter", "catalog-strategy", "catalog"],
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--label") options.label = argv[++i];
    else if (arg === "--compare") options.compare = [argv[++i], argv[++i]];
    else if (arg === "--counts") options.counts = parseList(argv[++i], options.counts);
    else if (arg === "--sizes") options.sizes = parseList(argv[++i], options.sizes);
    else if (arg === "--iterations") options.iterations = Number(argv[++i]);
    else if (arg === "--limit") options.limit = Number(argv[++i]);
    else if (arg === "--sections")
      options.sections = String(argv[++i])
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
    else if (arg === "--help" || arg === "-h") options.help = true;
    else throw new Error(`unknown argument: ${arg}`);
  }
  const labels = options.compare || (options.label ? [options.label] : []);
  if (labels.some(label => !/^[A-Za-z0-9_-]+$/.test(label || "")))
    throw new Error("labels must contain only letters, digits, hyphens and underscores");
  if ([...options.counts, ...options.sizes, options.iterations, options.limit].some(value => !Number.isSafeInteger(value) || value < 1))
    throw new Error("counts, sizes, iterations and limit must be positive integers");
  return options;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help || (!options.label && !options.compare)) {
    process.stdout.write(
      "usage: node scripts/measure_workbench.mjs --label <name> [--sections ...]\n" +
        "       node scripts/measure_workbench.mjs --compare <before> <after>\n",
    );
    return;
  }
  if (options.compare) compare(options.compare);
  else await measure(options);
}

main();
