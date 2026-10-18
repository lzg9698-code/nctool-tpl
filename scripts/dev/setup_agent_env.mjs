#!/usr/bin/env node
// Create the local Claude Code environment from the tracked .agents/ sources.
// Read-only unless --apply is passed; never touches tracked files or the network.
import {
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  readlinkSync,
  realpathSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const args = new Set(process.argv.slice(2));
const apply = args.has("--apply");
const json = args.has("--json");
const withLaunch = args.has("--with-launch");
const withSettings = args.has("--with-settings");
const withHook = args.has("--with-hook");

const actions = [];
const record = (state, subject, detail = "") =>
  actions.push({ state, subject, detail });

function fail(message) {
  record("error", message);
  report();
  process.exit(2);
}

function log(state, subject, detail = "") {
  record(state, subject, detail);
  if (!json)
    console.log(
      `${state.padEnd(8)} ${subject}${detail ? `\n         ${detail}` : ""}`,
    );
}

function report() {
  if (!json) return;
  console.log(JSON.stringify({ apply, actions }, null, 2));
}

function relativePath(path) {
  return path.slice(root.length + 1).replaceAll("\\", "/");
}

function sameTarget(link, target) {
  try {
    return (
      realpathSync(link).toLowerCase() === realpathSync(target).toLowerCase()
    );
  } catch {
    return false;
  }
}

function linkSkills() {
  const sourceRoot = join(root, ".agents", "skills");
  const linkRoot = join(root, ".claude", "skills");
  if (!existsSync(sourceRoot)) fail(`缺少技能源目录 ${relativePath(sourceRoot)}`);

  const skills = readdirSync(sourceRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .filter((name) => existsSync(join(sourceRoot, name, "SKILL.md")))
    .sort();

  if (!apply) {
    for (const name of skills) {
      const link = join(linkRoot, name);
      const target = join(sourceRoot, name);
      if (!existsSync(link)) {
        log("missing", `.claude/skills/${name}`, "运行 --apply 创建联接");
        continue;
      }
      let stats;
      try {
        stats = lstatSync(link);
      } catch (error) {
        log("error", `.claude/skills/${name}`, String(error));
        continue;
      }
      if (!stats.isSymbolicLink())
        log("conflict", `.claude/skills/${name}`, "已存在的真实目录，请人工确认");
      else if (sameTarget(link, target))
        log("ok", `.claude/skills/${name}`, "联接有效");
      else
        log(
          "broken",
          `.claude/skills/${name}`,
          `指向 ${readlinkSync(link)}，应为 ${relativePath(target)}`,
        );
    }
    return skills;
  }

  mkdirSync(linkRoot, { recursive: true });
  for (const name of skills) {
    const link = join(linkRoot, name);
    const target = join(sourceRoot, name);
    if (existsSync(link)) {
      const stats = lstatSync(link);
      if (!stats.isSymbolicLink()) {
        log("conflict", `.claude/skills/${name}`, "真实目录已存在，未覆盖");
        continue;
      }
      if (sameTarget(link, target)) {
        log("ok", `.claude/skills/${name}`, "联接有效");
        continue;
      }
      unlinkSync(link);
      linkDirectory(target, link);
      log("repaired", `.claude/skills/${name}`, "已重建联接");
      continue;
    }
    linkDirectory(target, link);
    log("created", `.claude/skills/${name}`, `-> ${relativePath(target)}`);
  }
  return skills;
}

function linkDirectory(target, link) {
  try {
    // Junctions need no administrator rights on Windows; 'dir' elsewhere.
    symlinkSync(target, link, process.platform === "win32" ? "junction" : "dir");
  } catch (error) {
    log(
      "error",
      relativePath(link),
      `无法创建联接：${error.code || error.message}；可手动复制 ${relativePath(target)}`,
    );
  }
}

function settingsTemplate() {
  const path = join(root, ".agents", "claude", "settings.template.json");
  // A fresh checkout can generate launch/hooks without a local permissions template.
  const template = existsSync(path)
    ? JSON.parse(readFileSync(path, "utf8"))
    : {
        permissions: { allow: [], ask: [] },
        hooks: { SessionStart: [{ hooks: [{ type: "command", command: 'python "${CLAUDE_PROJECT_DIR}/scripts/dev/session_context.py"', timeout: 20 }] }] },
      };
  if (!withHook) delete template.hooks;
  return template;
}

function mergeSettings(settings, template) {
  settings.permissions ??= {};
  settings.permissions.allow = settings.permissions.allow || [];
  settings.permissions.ask = settings.permissions.ask || [];
  const added = [];
  for (const rule of template.permissions.allow)
    if (!settings.permissions.allow.includes(rule)) {
      settings.permissions.allow.push(rule);
      added.push(rule);
    }
  for (const rule of template.permissions.ask)
    if (!settings.permissions.ask.includes(rule)) {
      settings.permissions.ask.push(rule);
      added.push(rule);
    }
  if (withHook && template.hooks?.SessionStart) {
    settings.hooks ??= {};
    settings.hooks.SessionStart ??= [];
    const present = JSON.stringify(settings.hooks.SessionStart).includes(
      "session_context.py",
    );
    if (!present) {
      settings.hooks.SessionStart.push(...template.hooks.SessionStart);
      added.push("hooks.SessionStart");
    }
  }
  return added;
}

function setupSettings() {
  const path = join(root, ".claude", "settings.json");
  const template = settingsTemplate();
  if (!existsSync(path)) {
    if (!apply) {
      log("missing", ".claude/settings.json", "运行 --apply 生成");
      return;
    }
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, JSON.stringify(template, null, 2) + "\n");
    log("created", ".claude/settings.json", "权限白名单与会话钩子");
    return;
  }

  let settings;
  try {
    settings = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    log("error", ".claude/settings.json", `不是有效 JSON：${error.message}`);
    return;
  }
  const added = mergeSettings(settings, template);
  if (!added.length) {
    log("ok", ".claude/settings.json", "已包含全部规则");
    return;
  }
  if (!apply) {
    log("missing", ".claude/settings.json", `待补充 ${added.length} 条规则`);
    return;
  }
  writeFileSync(
    `${path}.bak-${new Date().toISOString().replace(/[:.]/g, "-")}`,
    readFileSync(path),
  );
  writeFileSync(path, JSON.stringify(settings, null, 2) + "\n");
  log("updated", ".claude/settings.json", `补充 ${added.join(", ")}`);
}

const launchTemplate = () => {
  const preview = (name, variant) => ({
    name,
    runtimeExecutable: process.execPath,
    runtimeArgs: [join(root, "scripts", "dev", "preview_ui.mjs"), variant],
    port: name === "dev-template" ? 18788 : 18789,
  });
  return {
    version: "0.0.1",
    configurations: [
      preview("dev-template", "default"),
      preview("dev-math", "math"),
    ],
  };
};

function setupLaunch() {
  if (!withLaunch) return;
  const path = join(root, ".claude", "launch.json");
  const content = JSON.stringify(launchTemplate(), null, 2) + "\n";
  if (existsSync(path)) {
    if (readFileSync(path, "utf8") === content) {
      log("ok", ".claude/launch.json", "预览配置已是最新");
      return;
    }
    if (!apply) {
      log("pending", ".claude/launch.json", "与模板不同，--apply 时备份后重建");
      return;
    }
    const stamp = new Date().toISOString().replace(/[:.]/g, "-");
    writeFileSync(`${path}.bak-${stamp}`, readFileSync(path));
    writeFileSync(path, content);
    log("updated", ".claude/launch.json", "已备份原配置并重建");
    return;
  }
  if (!apply) {
    log("missing", ".claude/launch.json", "运行 --apply --with-launch 生成");
    return;
  }
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content);
  log("created", ".claude/launch.json", "仅保留默认与 math 两个预览配置");
}

function setupMemoryNote() {
  const path = join(root, ".claude", "settings.local.json");
  if (existsSync(path)) {
    try {
      const local = JSON.parse(readFileSync(path, "utf8"));
      const stale = (local.permissions?.allow || []).filter(
        (rule) => rule.length > 300,
      );
      if (stale.length)
        log(
          "note",
          ".claude/settings.local.json",
          `含 ${stale.length} 条超长一次性规则，可手工清理（脚本不改用户本地文件）`,
        );
    } catch {
      log("note", ".claude/settings.local.json", "不是有效 JSON，未处理");
    }
  }
}

function checkClaudeMd() {
  const path = join(root, "CLAUDE.md");
  if (!existsSync(path)) {
    log("missing", "CLAUDE.md", "Claude Code 不会自动读取 AGENTS.md");
    return;
  }
  const text = readFileSync(path, "utf8");
  if (/^@AGENTS\.md\s*$/m.test(text)) log("ok", "CLAUDE.md", "已导入 AGENTS.md");
  else log("error", "CLAUDE.md", "缺少 @AGENTS.md 导入行");
}

function checkMcp() {
  const path = join(root, ".mcp.json");
  if (!existsSync(path)) {
    log("missing", ".mcp.json", "未声明项目级 MCP server");
    return;
  }
  try {
    const config = JSON.parse(readFileSync(path, "utf8"));
    const names = Object.keys(config.mcpServers || {});
    log("ok", ".mcp.json", names.length ? `已声明 ${names.join(", ")}` : "无 server");
  } catch (error) {
    log("error", ".mcp.json", `不是有效 JSON：${error.message}`);
  }
}

function main() {
  // Launch-only setup is independent, but uses the same exit-code contract.
  let skills = [];
  if (withLaunch && !withSettings && !withHook) {
    setupLaunch();
  } else {
    skills = linkSkills();
    checkClaudeMd();
    checkMcp();
    setupSettings();
    setupLaunch();
    setupMemoryNote();
  }

  const errors = actions.filter((action) => action.state === "error");
  const pending = actions.filter((action) =>
    ["missing", "broken", "conflict", "pending"].includes(action.state),
  );
  if (!json)
    console.log(
      `\n${skills.length} 个技能；${errors.length} 个错误，${pending.length} 个待处理。` +
        (apply ? "" : " 使用 --apply 写入本地配置。"),
    );
  report();
  if (errors.length) process.exit(2);
  if (pending.length) process.exit(1);
}

main();
