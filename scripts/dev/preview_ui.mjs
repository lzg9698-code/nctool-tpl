#!/usr/bin/env node
// Prepare embedded resources and an isolated host before starting the preview.
import { existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const args = process.argv.slice(2);
const variant = args[0];
if (!["default", "math"].includes(variant) || args.slice(1).some((arg) => arg !== "--check")) {
  console.error("Usage: node scripts/dev/preview_ui.mjs <default|math> [--check]");
  process.exit(2);
}
const name = variant === "math" ? "dev-math" : "dev-template";
const target = join(root, "target", "platform-" + variant);
const binary = join(target, "debug", process.platform === "win32" ? "nctool.exe" : "nctool");
const home = join(root, "target", "agent-preview", name, "home");
const workspace = join(root, "target", "agent-preview", name, "workspace");
const port = process.env.PORT || (variant === "math" ? "18789" : "18788");
if (!/^\d+$/.test(port) || Number(port) > 65535) {
  console.error("Invalid preview port");
  process.exit(2);
}
const build = ["build", "-p", "nctool-cli", "--locked", "--target-dir", target];
if (variant === "math") build.push("--features", "math");
const launch = ["--home", home, "--workspace", workspace, "--profile", "template", "ui", "--port", port];
if (args.includes("--check")) {
  console.log(JSON.stringify({ variant, binary, exists: existsSync(binary), build, launch }, null, 2));
  process.exit(0);
}
function run(command, commandArgs) {
  const result = spawnSync(command, commandArgs, { cwd: root, stdio: "inherit" });
  if (result.error) console.error(result.error.message);
  if (result.error || result.status !== 0) process.exit(result.status || 1);
}
const resources = spawnSync(process.execPath, ["scripts/build_ui.mjs", "--check"], { cwd: root, encoding: "utf8" });
if (resources.status !== 0) {
  console.error(resources.stderr || resources.error?.message || "WebUI resources need rebuilding");
  if (!existsSync(join(root, "ui", "node_modules"))) {
    // npm.cmd requires the Windows command processor; arguments here are fixed.
    run(process.platform === "win32" ? "cmd.exe" : "npm", process.platform === "win32"
      ? ["/d", "/s", "/c", "npm ci --prefix ui"] : ["ci", "--prefix", "ui"]);
  }
  run(process.execPath, ["scripts/build_ui.mjs"]);
}
run("cargo", build);
mkdirSync(home, { recursive: true });
mkdirSync(workspace, { recursive: true });
const child = spawn(binary, launch, { cwd: root, stdio: "inherit" });
child.on("error", (error) => { console.error(error.message); process.exitCode = 1; });
child.on("exit", (code, signal) => { process.exitCode = code ?? (signal ? 1 : 0); });
for (const signal of ["SIGINT", "SIGTERM"])
  process.on(signal, () => { if (child.exitCode === null) child.kill(signal); });
