#!/usr/bin/env node
// Install Playwright + Chromium into a git-ignored directory for local browser
// acceptance. Read-only unless --apply. Never edits ui/package.json or lockfiles.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const PLAYWRIGHT_VERSION = "1.62.1";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const tools = join(root, "target", "agent-tools");
const modulePath = join(tools, "node_modules", "playwright");
const apply = process.argv.includes("--apply");
const force = process.argv.includes("--force");

const npm = process.platform === "win32" ? "npm.cmd" : "npm";
const shell = process.platform === "win32";

function installPlaywright() {
  const args = [
    "install",
    "--prefix",
    tools,
    "--no-save",
    "--no-package-lock",
    `playwright@${PLAYWRIGHT_VERSION}`,
  ];
  if (!shell) {
    execFileSync(npm, args, { stdio: "inherit" });
    return;
  }
  // Spawn the .cmd shim through cmd.exe instead of using shell:true with args,
  // which Node deprecates because the arguments are only concatenated.
  execFileSync(
    process.env.ComSpec || "cmd.exe",
    ["/d", "/s", "/c", [npm, ...args].map((part) => `"${part}"`).join(" ")],
    { stdio: "inherit" },
  );
}

function installedVersion() {
  const manifest = join(modulePath, "package.json");
  if (!existsSync(manifest)) return null;
  return JSON.parse(readFileSync(manifest, "utf8")).version;
}

function browserPath() {
  return execFileSync(
    process.execPath,
    [
      "-e",
      "console.log(require(process.argv[1]).chromium.executablePath())",
      modulePath,
    ],
    { encoding: "utf8" },
  ).trim();
}

function printEnvironment() {
  console.log(
    [
      "",
      "把下面几行加入当前 shell（或写入你自己的 shell 配置）：",
      `  export NCTOOL_PLAYWRIGHT="${modulePath.replaceAll("\\", "/")}"`,
      `  export NCTOOL_CHROMIUM="${browserPath().replaceAll("\\", "/")}"`,
      `  export NCTOOL_REPORT="$PWD/target/browser-agent"`,
      "",
      "注意：NCTOOL_PLAYWRIGHT 必须指向 playwright 模块目录，而不是 bin 脚本。",
    ].join("\n"),
  );
}

function main() {
  const current = installedVersion();
  if (current === PLAYWRIGHT_VERSION && !force) {
    console.log(`playwright ${current} 已安装在 ${modulePath}`);
    console.log(`chromium: ${browserPath()}`);
    printEnvironment();
    return 0;
  }

  if (!apply) {
    console.log(
      current
        ? `已安装 playwright ${current}，目标版本 ${PLAYWRIGHT_VERSION}；使用 --apply 重新安装。`
        : `playwright ${PLAYWRIGHT_VERSION} 未安装；使用 --apply 安装到 ${modulePath}。`,
    );
    return 1;
  }

  mkdirSync(tools, { recursive: true });
  console.log(`安装 playwright@${PLAYWRIGHT_VERSION} 到 ${tools}`);
  installPlaywright();

  console.log("安装 Chromium（复用 PLAYWRIGHT_BROWSERS_PATH 中已有的浏览器）");
  execFileSync(process.execPath, [join(modulePath, "cli.js"), "install", "chromium"], {
    stdio: "inherit",
  });

  const version = installedVersion();
  if (version !== PLAYWRIGHT_VERSION) {
    console.error(`安装结果版本为 ${version}，预期 ${PLAYWRIGHT_VERSION}`);
    return 2;
  }
  console.log(`playwright ${version} 就绪`);
  console.log(`chromium: ${browserPath()}`);
  printEnvironment();
  return 0;
}

try {
  process.exitCode = main();
} catch (error) {
  console.error(`安装失败：${error.message}`);
  process.exitCode = 2;
}
