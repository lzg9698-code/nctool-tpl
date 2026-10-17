// 历史重构专项（WebUI 主题/布局/可访问性工作台验收）：这是 2026-10 前端重构期间的一次性
// 验收脚本，当前活动 App 未采用该重构入口，因此不纳入 CI 与默认验收（check_v2.py 不再检查、
// ci.yml 不再调用）。保留文件仅为回溯当时的重构证据；如要运行，需先自行准备与之匹配的构建。
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const { spawn } = require("node:child_process");
const { chromium } = require(process.env.NCTOOL_PLAYWRIGHT || "playwright");
const root = path.resolve(__dirname, "..");
const binary =
  process.env.NCTOOL_BINARY || path.join(root, "target/debug/nctool.exe");
const output =
  process.env.NCTOOL_REPORT || path.join(root, "target/ui-refactor-browser");
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "nctool-ui-refactor-"));
let server, browser, page;
const errors = [],
  checks = [];
async function start() {
  return new Promise((resolve, reject) => {
    server = spawn(
      binary,
      [
        "--home",
        path.join(temporary, "home"),
        "--workspace",
        path.join(temporary, "workspace"),
        "--profile",
        "template",
        "ui",
        "--port",
        "0",
      ],
      { cwd: root },
    );
    let log = "";
    const timeout = setTimeout(
      () => reject(new Error(log || "startup timeout")),
      15000,
    );
    server.stderr.on("data", (data) => {
      log += data;
      const match = log.match(/http:\/\/127\.0\.0\.1:\d+/);
      if (match) {
        clearTimeout(timeout);
        resolve(match[0]);
      }
    });
    server.once("error", (error) => {
      clearTimeout(timeout);
      reject(error);
    });
    server.once("exit", (code) => {
      clearTimeout(timeout);
      reject(new Error(`server exited ${code}: ${log}`));
    });
  });
}
async function contrast(page) {
  return page.evaluate(() => {
    const rgb = (value) => value.match(/[\d.]+/g).map(Number);
    let checked = 0;
    const luminance = (channels) =>
      channels
        .slice(0, 3)
        .map((value) => {
          value /= 255;
          return value <= 0.04045
            ? value / 12.92
            : ((value + 0.055) / 1.055) ** 2.4;
        })
        .reduce(
          (sum, value, i) => sum + value * [0.2126, 0.7152, 0.0722][i],
          0,
        );
    const problems = [];
    const selectors =
      ".nav-item, .template-item strong, .template-item small, .sidebar-section-title, .filter-summary span, .document-state, .field-heading label, .field-description, .text-button, .pane-tabs button, .empty-state strong, .empty-state p, .export-hint, .action-hint, .filename-field span, .run-status, .workspace-indicator, .workspace-heading span, .workspace-heading strong, .workspace-connection";
    for (const el of document.querySelectorAll(selectors)) {
      if (!el.getClientRects().length || el.closest("button:disabled")) continue;
      checked++;
      const style = getComputedStyle(el);
      let background = [255, 255, 255, 1];
      const ancestors = [];
      for (let node = el; node; node = node.parentElement) ancestors.unshift(node);
      for (const node of ancestors) {
        const color = rgb(getComputedStyle(node).backgroundColor);
        const alpha = color[3] ?? 1;
        background = color
          .slice(0, 3)
          .map((value, i) => value * alpha + background[i] * (1 - alpha));
      }
      const foreground = luminance(rgb(style.color)),
        back = luminance(background);
      const ratio =
        (Math.max(foreground, back) + 0.05) / (Math.min(foreground, back) + 0.05);
      if (ratio < 4.5 || parseFloat(style.fontSize) < 12)
        problems.push({
          text: el.textContent.trim(),
          ratio,
          fontSize: style.fontSize,
        });
    }
    // A check that measured nothing must fail rather than pass silently.
    return { problems, checked };
  });
}
async function main() {
  fs.mkdirSync(output, { recursive: true });
  const url = await start();
  const response = await fetch(url + "/api/v2/actions/template.save", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      input: {
        name: "theme-report",
        asset: {
          source: "报告：{{ name }}\n编号：{{ serial }}",
          schema: {
            type: "object",
            properties: {
              name: { type: "string", title: "名称" },
              serial: { type: "integer", title: "编号" },
            },
          },
          defaults: { name: "样例" },
          metadata: { title: "主题验收模板" },
        },
      },
    }),
  });
  assert.equal((await response.json()).ok, true);
  browser = await chromium.launch({
    headless: true,
    executablePath: process.env.NCTOOL_CHROMIUM,
  });
  page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(url);
  await page
    .locator(".template-item")
    .filter({ hasText: "主题验收模板" })
    .click();
  await page.getByRole("button", { name: "编辑模板", exact: true }).click();
  const source = page.locator('.cm-content[aria-label="模板源码"]');
  await source.fill("报告：{{ name }}\n编号：{{ serial }}\n主题切换保留草稿");
  await source.focus();
  await page.keyboard.press("Control+End");
  await page.keyboard.press("ArrowLeft");
  const before = await page.evaluate(() => {
    window.__themeEditor = document.querySelector(".source-pane .cm-editor");
    const s = getSelection();
    return { text: window.__themeEditor.textContent, anchor: s.anchorOffset };
  });
  await page.getByRole("button", { name: "切换深色主题" }).click();
  assert.equal(
    await page.evaluate(() => document.documentElement.dataset.theme),
    "dark",
  );
  assert.equal(
    await source.innerText(),
    "报告：{{ name }}\n编号：{{ serial }}\n主题切换保留草稿",
  );
  assert.equal(
    await page.evaluate(
      () =>
        window.__themeEditor ===
        document.querySelector(".source-pane .cm-editor"),
    ),
    true,
  );
  const palette = await page.evaluate(() => ({
    background: getComputedStyle(document.querySelector(".cm-editor"))
      .backgroundColor,
    text: getComputedStyle(document.querySelector(".cm-content")).color,
  }));
  assert.notEqual(palette.background, "rgb(255, 255, 255)");
  await page.screenshot({
    path: path.join(output, "workbench-dark.png"),
    fullPage: true,
  });
  await page.reload();
  await source.waitFor();
  assert.equal(
    await page.evaluate(() => document.documentElement.dataset.theme),
    "dark",
  );
  assert.equal(
    await source.innerText(),
    "报告：{{ name }}\n编号：{{ serial }}\n主题切换保留草稿",
  );
  await page.getByRole("button", { name: "切换浅色主题" }).click();
  await page.screenshot({
    path: path.join(output, "workbench-light.png"),
    fullPage: true,
  });
  checks.push(
    "theme persistence, precise draft restoration, editor DOM preserved, dark editor palette",
  );
  await page.getByLabel("更多模板操作").click();
  await page.getByRole("menuitem", { name: "删除模板", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "删除这个模板？" });
  await dialog.waitFor();
  for (let i = 0; i < 7; i++) {
    await page.keyboard.press("Tab");
    assert.equal(
      await page.evaluate(
        () => !!document.activeElement.closest('[role="dialog"]'),
      ),
      true,
    );
  }
  await page.keyboard.press("Escape");
  await dialog.waitFor({ state: "hidden" });
  await page.waitForFunction(
    () => document.activeElement?.getAttribute("aria-label") === "更多模板操作",
  );
  checks.push("Radix menu-to-dialog focus loop and opener restoration");
  const layouts = [];
  for (const viewport of [
    { width: 1280, height: 720 },
    { width: 1440, height: 900 },
    { width: 1920, height: 1080 },
    { width: 375, height: 812 },
  ]) {
    await page.setViewportSize(viewport);
    const dimensions = await page.evaluate(() => ({
      scroll: document.documentElement.scrollWidth,
      width: innerWidth,
    }));
    assert(
      dimensions.scroll <= dimensions.width + 1,
      `horizontal overflow at ${viewport.width}: ${dimensions.scroll}`,
    );
    assert(
      await page.getByRole("button", { name: "保存", exact: true }).isVisible(),
    );
    if (viewport.width > 760) {
      const navigation = await page
        .locator(".sidebar-nav")
        .evaluate((node) => ({
          scroll: node.scrollHeight,
          height: node.clientHeight,
          overflow: getComputedStyle(node).overflowY,
        }));
      assert(
        navigation.scroll <= navigation.height + 1,
        `unnecessary navigation scrollbar at ${viewport.width}`,
      );
      assert.equal(navigation.overflow, "visible");
    }
    layouts.push({ ...viewport, ...dimensions });
  }
  await page.screenshot({
    path: path.join(output, "workbench-mobile.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.getByRole("button", { name: "折叠侧栏" }).click();
  assert(await page.locator(".app-shell.sidebar-collapsed").count());
  await page.getByRole("button", { name: "展开侧栏" }).click();
  checks.push("four viewport sizes without document overflow, collapse/expand");
  // Ordinary workbench text must stay legible in both themes (WCAG 4.5:1, >=12px).
  for (const theme of ["light", "dark"]) {
    if ((await page.evaluate(() => document.documentElement.dataset.theme)) !== theme)
      await page
        .getByRole("button", {
          name: theme === "dark" ? "切换深色主题" : "切换浅色主题",
        })
        .click();
    const { problems, checked } = await contrast(page);
    assert(checked > 0, `contrast check measured no elements (${theme})`);
    assert.deepEqual(
      problems,
      [],
      `ordinary workbench text contrast/font size (${theme})`,
    );
  }
  checks.push("light and dark contrast/font-size floors");
  assert.deepEqual(errors, []);
  fs.writeFileSync(
    path.join(output, "report.json"),
    JSON.stringify(
      {
        status: "passed",
        platform: process.platform,
        timestamp: new Date().toISOString(),
        binary,
        checks,
        layouts,
        palette,
        errors,
      },
      null,
      2,
    ),
  );
  console.log(
    "PASS: themes, preserved editor/drafts, Radix focus and responsive workbench",
  );
}
main()
  .catch(async (error) => {
    fs.mkdirSync(output, { recursive: true });
    fs.writeFileSync(
      path.join(output, "failure.txt"),
      error.stack || String(error),
    );
    fs.writeFileSync(
      path.join(output, "report.json"),
      JSON.stringify(
        { status: "failed", binary, checks, errors, error: String(error) },
        null,
        2,
      ),
    );
    if (page)
      await page
        .screenshot({ path: path.join(output, "failure.png"), fullPage: true })
        .catch(() => {});
    console.error(error);
    process.exitCode = 1;
  })
  .finally(async () => {
    if (browser) await browser.close();
    if (server && server.exitCode === null) server.kill();
    fs.rmSync(temporary, { recursive: true, force: true });
  });
