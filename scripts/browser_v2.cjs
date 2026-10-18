// Real browser acceptance for author/operator workflows and precision-preserving data.
const { chromium } = require(process.env.NCTOOL_PLAYWRIGHT || "playwright");
const { spawn, spawnSync, execFileSync } = require("node:child_process");
const fs = require("node:fs"),
  os = require("node:os"),
  path = require("node:path"),
  assert = require("node:assert/strict");
const root = path.resolve(__dirname, "..");
const reportDir = process.env.NCTOOL_REPORT || path.join(root, "target/ui22-browser");
const binary =
  process.env.NCTOOL_BINARY ||
  path.join(
    root,
    "target/debug/nctool" + (process.platform === "win32" ? ".exe" : ""),
  );
const temp = fs.mkdtempSync(path.join(os.tmpdir(), "nctool-workbench-"));
const home = path.join(temp, "home"),
  workspace = path.join(temp, "workspace");
const children = [];
let browser, page;
const errors = [];
const cli = (...args) =>
  JSON.parse(
    execFileSync(binary, ["--home", home, "--workspace", workspace, ...args], {
      encoding: "utf8",
      cwd: root,
    }),
  );
function serve(profile, overrides = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(
      binary,
      [
        "--home",
        overrides.home || home,
        "--workspace",
        overrides.workspace || workspace,
        "--profile",
        profile,
        "ui",
        "--port",
        "0",
      ],
      { cwd: root },
    );
    children.push(child);
    let log = "";
    const timer = setTimeout(
      () => reject(new Error(log || "startup timeout")),
      15000,
    );
    child.stderr.on("data", (data) => {
      log += data;
      const match = log.match(/http:\/\/127\.0\.0\.1:\d+/);
      if (match) {
        clearTimeout(timer);
        resolve(match[0]);
      }
    });
    child.on("error", reject);
    child.once("exit", (code) => {
      clearTimeout(timer);
      if (code) reject(new Error(log));
    });
  });
}
async function request(url, action, input) {
  const response = await fetch(url + "/api/v2/actions/" + action, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ input }),
  });
  const result = await response.json();
  if (!result.ok) throw new Error(result.error.message);
  return result.data;
}
const content = (label) =>
  page.locator('.cm-content[aria-label="' + label + '"]');
const field = (name) =>
  page.locator('[data-field-path="/' + name + '"] > input');
async function complete() {
  await page.locator(".result-pane .run-status.succeeded").waitFor();
}
async function generate() {
  await page
    .getByRole("button", { name: "生成文件", exact: true })
    .first()
    .click();
  await page
    .locator(
      ".result-pane .run-status.queued, .result-pane .run-status.running",
    )
    .waitFor();
  await complete();
}
async function checkEditorStyles() {
  const editor = content("模板源码");
  await editor.fill('{% set count = 12 %}\n{{ "sample" }}\n{# editor style regression #}');
  const tokens = ["keyword", "number", "string", "comment"];
  for (const token of tokens)
    await editor.locator(".syntax-" + token).first().waitFor();
  const styles = await editor.evaluate((element, names) => {
    const rootStyle = getComputedStyle(element);
    const variables = [
      "--editor-text", "--editor-background", "--editor-gutter",
      "--editor-gutter-text", "--editor-active-line", "--editor-active-gutter",
      "--editor-selection", "--editor-search", "--editor-match",
      "--surface-muted", "--warning-line",
      ...names.map((name) => "--syntax-" + name),
    ];
    return {
      missing: variables.filter((name) => !rootStyle.getPropertyValue(name).trim()),
      text: rootStyle.color,
      colors: names.map((name) => getComputedStyle(element.querySelector(".syntax-" + name)).color),
    };
  }, tokens);
  assert.deepEqual(styles.missing, [], "Editor CSS variables must be loaded");
  assert.equal(new Set(styles.colors).size, tokens.length, "Syntax tokens must have distinct colors");
  for (const color of styles.colors)
    assert.notEqual(color, styles.text, "Syntax rules must style tokens beyond inherited text");

  await editor.click();
  await editor.press(process.platform === "darwin" ? "Meta+f" : "Control+f");
  const search = page.locator(".source-pane .cm-search");
  await search.waitFor({ state: "visible" });
  const searchInput = search.locator('input[name="search"]');
  await searchInput.fill("sample");
  await searchInput.press("Enter");
  await page.locator(".source-pane .cm-searchMatch, .source-pane .cm-selectionMatch").first().waitFor();
  const panelStyles = await search.evaluate((element) => {
    const panel = element.closest(".cm-panels");
    const input = element.querySelector(".cm-textfield");
    const panelStyle = getComputedStyle(panel);
    const inputStyle = getComputedStyle(input);
    return { background: panelStyle.backgroundColor, inputBackground: inputStyle.backgroundColor, text: inputStyle.color };
  });
  for (const background of [panelStyles.background, panelStyles.inputBackground]) {
    assert.notEqual(background, "rgba(0, 0, 0, 0)", "Search surfaces must have an opaque background");
    assert.notEqual(background, panelStyles.text, "Search text must remain visible");
  }
  fs.mkdirSync(reportDir, { recursive: true });
  await page.screenshot({ path: path.join(reportDir, "editor-styles.png"), fullPage: true });
  fs.writeFileSync(path.join(reportDir, "editor-styles.json"), JSON.stringify({ styles, panelStyles }, null, 2));
  await searchInput.press("Escape");
  await search.waitFor({ state: "hidden" });
}
async function main() {
  browser = await chromium.launch({
    headless: true,
    ...(process.env.NCTOOL_CHROMIUM
      ? { executablePath: process.env.NCTOOL_CHROMIUM }
      : {}),
    args: ["--no-sandbox"],
  });
  page = await browser.newPage({
    viewport: { width: 1440, height: 900 },
    acceptDownloads: true,
  });
  page.on("pageerror", (e) => errors.push(e.message));
  const url = await serve("template");
  await page.goto(url);
  await page.getByRole("heading", { name: "让模板成为可重复的工作" }).waitFor();
  assert.equal(
    await page.getByRole("button", { name: "机床配置", exact: true }).count(),
    0,
  );
  assert.equal(
    await page
      .locator("body")
      .innerText()
      .then((t) => t.includes("Hello Ada")),
    false,
  );
  // An author creates a template and selects parameter types without writing Schema/JSON.
  await page
    .getByRole("button", { name: "新建模板", exact: true })
    .first()
    .click();
  await page.getByLabel("模板名称", { exact: true }).fill("检查报告");
  await checkEditorStyles();
  await content("模板源码").fill(
    "Hello {{ name }}!{% for item in items %}:{{ item }}{% endfor %}",
  );
  // The syntax reference is reachable next to the source without replacing the draft.
  const authorSource = await content("模板源码").innerText();
  await page.getByRole("button", { name: "语法提示", exact: true }).click();
  await page.getByRole("heading", { name: "模板语法", exact: true }).waitFor();
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  const firstExample = page.locator(".syntax-card").first();
  const snippet = await firstExample.locator(".syntax-code").innerText();
  await firstExample.getByRole("button", { name: /^复制示例/ }).click();
  assert.equal(
    (await page.evaluate(() => navigator.clipboard.readText())).replace(
      /\r\n/g,
      "\n",
    ),
    snippet,
  );
  const syntaxCategories = [
    "基础与数据",
    "条件与循环",
    "过滤器与表达式",
    "引用与复用",
    "空白与注释",
  ];
  let renderedExamples = 0;
  for (const category of syntaxCategories) {
    await page.getByRole("button", { name: category, exact: true }).click();
    const cards = page.locator(".syntax-card");
    for (let i = 0; i < (await cards.count()); i++) {
      const card = cards.nth(i);
      await card.getByRole("button", { name: "试跑示例", exact: true }).click();
      await card.locator(".syntax-result").waitFor();
      assert.equal(await card.getByRole("alert").count(), 0);
      renderedExamples++;
    }
  }
  assert.equal(renderedExamples, 14);
  await page.getByLabel("搜索模板语法", { exact: true }).fill("include");
  assert((await page.locator(".syntax-card").innerText()).includes("include"));
  await page
    .getByLabel("搜索模板语法", { exact: true })
    .fill("找不到的语法123456");
  await page.getByText("没有匹配的语法", { exact: true }).waitFor();
  await page.getByRole("button", { name: "插件语法", exact: true }).click();
  assert.equal(
    await page.getByRole("button", { name: "试跑示例", exact: true }).count(),
    1,
  );
  for (const button of await page
    .getByRole("button", { name: "试跑示例", exact: true })
    .all())
    assert(await button.isDisabled());
  const syntaxOut = process.env.NCTOOL_REPORT ? path.join(reportDir, "syntax-guide") : path.join(root, "target/syntax-guide");
  fs.mkdirSync(syntaxOut, { recursive: true });
  await page.getByRole("button", { name: "基础与数据", exact: true }).click();
  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
    false,
  );
  await page.screenshot({
    path: path.join(syntaxOut, "mobile.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.locator(".page-content").evaluate((element) => {
    element.scrollTop = 0;
  });
  await page.screenshot({
    path: path.join(syntaxOut, "desktop.png"),
    fullPage: true,
  });
  await page.getByRole("button", { name: "返回工作区", exact: true }).click();
  assert.equal(await content("模板源码").innerText(), authorSource);
  await page.getByRole("button", { name: "识别参数", exact: true }).click();
  await page.getByLabel("name类型", { exact: true }).selectOption("string");
  await page.getByLabel("items类型", { exact: true }).selectOption("array");
  await page
    .locator(".definition")
    .filter({
      has: page.locator(".definition-name").getByText("items", { exact: true }),
    })
    .locator("summary")
    .click();
  await page
    .getByLabel("items元素类型", { exact: true })
    .selectOption("number");
  await page.getByRole("button", { name: "使用模板", exact: true }).click();
  const separator = page.getByRole("separator", { name: "调整面板宽度" });
  await separator.focus();
  const widthBefore = await page
    .locator(".parameters-pane")
    .evaluate((el) => el.getBoundingClientRect().width);
  await page.keyboard.press("ArrowRight");
  const widthAfter = await page
    .locator(".parameters-pane")
    .evaluate((el) => el.getBoundingClientRect().width);
  assert(widthAfter > widthBefore);
  await field("name").fill("Ada");
  await page.getByRole("button", { name: "添加items", exact: true }).click();
  await field("items/0").fill("1");
  await page.getByRole("button", { name: "添加items", exact: true }).click();
  await field("items/1").fill("2");
  await generate();
  assert((await content("生成结果").innerText()).includes("Hello Ada!:1:2"));
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "导出文件", exact: true }).click();
  assert.equal(
    fs.readFileSync(await (await download).path(), "utf8"),
    "Hello Ada!:1:2",
  );
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await page.getByRole("status").filter({ hasText: "模板已保存" }).waitFor();
  await page.reload();
  await page.getByText("已恢复上次的工作区草稿").waitFor();
  assert.equal(await field("name").inputValue(), "Ada");
  await complete();
  // Exact numeric text survives form <-> JSON and backend output.
  await field("items/0").fill("9007199254740993");
  await page.getByRole("button", { name: "JSON", exact: true }).click();
  assert(
    (await content("本次参数 JSON").innerText()).includes("9007199254740993"),
  );
  await page.getByRole("button", { name: "表单", exact: true }).click();
  assert.equal(await field("items/0").inputValue(), "9007199254740993");
  await generate();
  assert((await content("生成结果").innerText()).includes("9007199254740993"));
  await field("items/0").fill("1e-400");
  await page
    .getByRole("button", { name: "生成文件", exact: true })
    .first()
    .click();
  await page.getByRole("alert").filter({ hasText: "下溢" }).waitFor();
  assert.equal(await field("items/0").inputValue(), "1e-400");
  assert.equal(
    await page
      .getByRole("button", { name: "导出文件", exact: true })
      .isDisabled(),
    true,
  );
  // Malformed advanced JSON is retained and never replaced by stale valid parameters.
  await page.getByRole("button", { name: "JSON", exact: true }).click();
  await content("本次参数 JSON").fill('{"name":');
  const before = (await (await fetch(url + "/api/v2/runs")).json()).data.runs
    .length;
  await page
    .getByRole("button", { name: "生成文件", exact: true })
    .first()
    .click();
  await page.getByRole("alert").filter({ hasText: "请先修正 JSON" }).waitFor();
  assert.equal(
    (await (await fetch(url + "/api/v2/runs")).json()).data.runs.length,
    before,
  );
  await page.reload();
  await page.getByRole("button", { name: "JSON", exact: true }).click();
  assert((await content("本次参数 JSON").innerText()).includes('{"name":'));
  await content("本次参数 JSON").fill('{"name":"Ada","items":[1,2]}');
  await page.getByRole("button", { name: "表单", exact: true }).click();
  await generate();
  // Drafts survive switching documents and records restore the actual input.
  await page.getByRole("button", { name: "编辑模板", exact: true }).click();
  const draft = await content("模板源码").innerText();
  await page
    .getByRole("button", { name: "新建模板", exact: true })
    .first()
    .click();
  await page.getByLabel("模板名称", { exact: true }).fill("另一个草稿");
  await page
    .locator(".document-tab")
    .getByRole("button", { name: "检查报告", exact: true })
    .click();
  assert.equal(await content("模板源码").innerText(), draft);
  await page.getByRole("button", { name: /执行记录/ }).click();
  await page.locator(".history-row").first().click();
  await page.getByRole("button", { name: "恢复输入", exact: true }).click();
  await page.getByLabel("模板名称", { exact: true }).waitFor();
  assert(
    (await page.getByLabel("模板名称", { exact: true }).inputValue()).includes(
      "历史输入",
    ),
  );
  assert.equal(await field("name").inputValue(), "Ada");
  // A generation failure cannot export a previous successful output as the current result.
  await page.getByRole("button", { name: "编辑模板", exact: true }).click();
  await content("模板源码").fill("{{ missing }}");
  await page
    .getByRole("button", { name: "生成文件", exact: true })
    .first()
    .click();
  await page.locator(".result-pane .run-status.failed").waitFor();
  assert.equal(
    await page
      .getByRole("button", { name: "导出文件", exact: true })
      .isDisabled(),
    true,
  );
  assert.equal(await content("生成结果").count(), 0);
  // Concurrent edits preserve the draft and reloading the disk version retains the active tab.
  const catalog = await request(url, "template.catalog", {});
  const saved = catalog.data.items.find((item) => item.title === "检查报告");
  await page.locator(".template-item").filter({ hasText: "检查报告" }).click();
  await page.getByRole("button", { name: "编辑模板", exact: true }).click();
  const read = await request(url, "template.read", { name: saved.id });
  read.data.asset.source += "\nDISK VERSION";
  await request(url, "template.save", {
    name: saved.id,
    asset: read.data.asset,
    expected: read.data.fingerprint,
  });
  await content("模板源码").fill("LOCAL DRAFT {{ name }}");
  await page.getByRole("button", { name: "保存", exact: true }).click();
  const conflict = page.getByRole("dialog", { name: "模板已被其他编辑修改" });
  await conflict.waitFor();
  assert((await conflict.innerText()).includes("LOCAL DRAFT"));
  assert((await conflict.innerText()).includes("DISK VERSION"));
  assert.equal(await page.locator(".modal-overlay").isVisible(), true);
  const modalLayout = await conflict.evaluate((element) => {
    const bounds = element.getBoundingClientRect();
    return {
      position: getComputedStyle(element).position,
      centerX: bounds.x + bounds.width / 2,
      centerY: bounds.y + bounds.height / 2,
      viewportX: innerWidth / 2,
      viewportY: innerHeight / 2,
    };
  });
  assert.equal(modalLayout.position, "fixed");
  assert(Math.abs(modalLayout.centerX - modalLayout.viewportX) < 2);
  assert(Math.abs(modalLayout.centerY - modalLayout.viewportY) < 2);
  await page.getByRole("button", { name: "加载磁盘版本", exact: true }).click();
  await content("模板源码").waitFor();
  assert((await content("模板源码").innerText()).includes("DISK VERSION"));
  await page.getByLabel("更多模板操作").click();
  await page.getByRole("menuitem", { name: "复制模板", exact: true }).click();
  await page.getByLabel("模板名称", { exact: true }).waitFor();
  await page.waitForFunction(() =>
    document.querySelector('[aria-label="模板名称"]').value.includes("副本"),
  );
  await page.getByLabel("更多模板操作").click();
  await page.getByRole("menuitem", { name: "删除模板", exact: true }).click();
  await page
    .getByRole("dialog", { name: "删除这个模板？" })
    .getByRole("button", { name: "删除模板", exact: true })
    .click();
  await page.getByRole("dialog").waitFor({ state: "hidden" });
  // Installed third-party actions use recursive forms and background records.
  await page.getByRole("button", { name: "插件与设置", exact: true }).click();
  await page
    .getByLabel("插件目录", { exact: true })
    .fill(path.join(root, "examples/plugins/python-report"));
  await page.getByRole("button", { name: "读取插件信息", exact: true }).click();
  await page.locator(".plugin-preview").waitFor();
  await page.getByRole("button", { name: "安装插件", exact: true }).click();
  await page
    .locator(".plugin-config-card")
    .filter({ hasText: "python-report" })
    .waitFor();
  await page
    .locator(".plugin-config-card")
    .filter({ hasText: "python-report" })
    .getByRole("button", { name: "设为启用", exact: true })
    .click();
  await page.getByRole("status").filter({ hasText: "配置检查通过" }).waitFor();
  await page
    .getByRole("button", { name: "验证并保存配置", exact: true })
    .click();
  await page
    .getByRole("status")
    .filter({ hasText: "配置已验证并保存" })
    .waitFor();
  const report = await serve("template");
  await page.goto(report);
  await page.getByRole("button", { name: "数据报告", exact: true }).click();
  await page.getByRole("button", { name: "执行任务", exact: true }).click();
  await page.locator(".plugin-columns .run-status.succeeded").waitFor();
  assert((await content("历史生成结果").innerText()).includes("Total: 60"));
  // 旧加工能力已彻底移除：导航与运行时不提供任何 NC/机床/工序入口。
  for (const label of ["机床配置", "G 代码生成", "多工序程序"]) {
    assert.equal(
      await page.getByRole("button", { name: label, exact: true }).count(),
      0,
    );
  }
  // 能力目录不再包含 NC；旧元数据由兼容守卫识别，不发布伪动作。
  const capabilities = (await (await fetch(url + "/api/v2/capabilities")).json())
    .data;
  assert.equal(
    (capabilities.actions || []).some((entry) => entry.id === "nc.generate"),
    false,
  );
  assert.equal(
    (capabilities.actions || []).some((entry) =>
      /^(nc|machine|process|sync-turning|undercut)\./.test(entry.id),
    ),
    false,
  );
  assert.equal((capabilities.document_actions || []).some(
    (entry) => /^(nc|machine|process|sync-turning|undercut)\./.test(entry.id),
  ), false);

  // 旧加工模板不能降级为普通渲染：既不自动选择生成方式，也不提交任务。
  await page.goto(url);
  await request(url, "template.save", {
    name: "legacy-nc",
    asset: {
      source: "G1 X{{ x | nc_fixed(3) }} S{{ spindle }}",
      schema: {
        type: "object",
        properties: {
          x: { type: "number", title: "位置 X" },
          spindle: { type: "integer", title: "转速" },
        },
        required: ["x", "spindle"],
      },
      defaults: { x: 12.5, spindle: 2000 },
      metadata: {
        title: "旧加工模板",
        nc: { specs: [{ name: "x", kind: "number", min: 0, max: 100 }] },
      },
    },
  });
  await page.reload();
  assert.equal(await page.locator(".template-item").filter({ hasText: "旧加工模板" }).count(), 0);
  let submitted = 0;
  const observe = (request) => {
    if (request.url().endsWith("/api/v2/runs") && request.method() === "POST")
      submitted++;
  };
  page.on("request", observe);
  const legacyFile = path.join(temp, "legacy-nc-template.json");
  fs.writeFileSync(legacyFile, JSON.stringify((await request(url, "template.read", { name: "legacy-nc" })).data.asset));
  await page.locator('input[type="file"]').first().setInputFiles(legacyFile);
  await page.getByRole("alert").filter({ hasText: "已移除的加工意图" }).first().waitFor();
  assert.equal(submitted, 0);
  page.off("request", observe);
  // 后端直接拒绝带有加工意图的资产，不静默改写为通用模板。
  const legacyRender = await fetch(url + "/api/v2/actions/template.render", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ input: { template: "legacy-nc" } }),
  }).then((response) => response.json());
  assert.equal(legacyRender.ok, false);
  assert.equal(legacyRender.error.code, "domain_action_required");

  // 旧 nc profile 不再启动：进程以 invalid_profile 拒绝，且不自动迁移配置。
  const legacyHome = path.join(temp, "legacy-home");
  fs.mkdirSync(legacyHome, { recursive: true });
  const legacyConfig = {
    profile: "nc",
    enabled: ["nc", "process"],
    disabled: [],
    providers: { "nc.generate": "nc" },
    plugins: { nc: {} },
  };
  fs.writeFileSync(
    path.join(legacyHome, "config.json"),
    JSON.stringify(legacyConfig),
  );
  const bootLegacy = spawnSync(
    binary,
    [
      "--home",
      legacyHome,
      "--workspace",
      path.join(temp, "legacy-workspace"),
      "actions",
    ],
    { cwd: root, encoding: "utf8" },
  );
  assert.notEqual(bootLegacy.status, 0);
  assert((bootLegacy.stderr || "").includes("invalid_profile"));
  assert.deepEqual(
    JSON.parse(fs.readFileSync(path.join(legacyHome, "config.json"), "utf8")),
    legacyConfig,
  );
  // 只读预检查给出显式迁移方案，仍不改写磁盘上的旧配置。
  const checkLegacy = spawnSync(
    binary,
    ["--home", legacyHome, "config", "check"],
    { cwd: root, encoding: "utf8" },
  );
  assert.equal(checkLegacy.status, 0);
  const checkReport = JSON.parse(checkLegacy.stdout);
  assert(checkReport.changes.some((line) => line.includes("profile: nc")));
  assert(
    checkReport.changes.some((line) => line.includes("remove nc")),
  );
  assert.deepEqual(
    JSON.parse(fs.readFileSync(path.join(legacyHome, "config.json"), "utf8")),
    legacyConfig,
  );

  // 旧加工历史只读：可查看与导出原产物，但不能重执行或恢复输入。
  const runsDir = path.join(workspace, ".nctool/runs");
  fs.mkdirSync(path.join(runsDir, "_index"), { recursive: true });
  const legacyRunId = "run-legacy-nc-1";
  const legacyText = "G1 X12.500 S2000\n";
  const legacyStarted = Date.now() - 60000;
  fs.writeFileSync(
    path.join(runsDir, legacyRunId + ".json"),
    JSON.stringify({
      id: legacyRunId,
      action: "nc.generate",
      input: { source: "G1 X{{ x }}", context: { x: 12.5 } },
      status: "succeeded",
      started_at: legacyStarted,
      finished_at: legacyStarted + 1000,
      result: {
        data: { text: legacyText },
        artifacts: [
          { name: "legacy.SPF", media_type: "text/plain", text: legacyText },
        ],
        diagnostics: [],
      },
      error: null,
      plugins: [],
      title: "旧加工记录",
    }),
  );
  fs.writeFileSync(
    path.join(runsDir, "_index", legacyRunId + ".json"),
    JSON.stringify({
      id: legacyRunId,
      title: "旧加工记录",
      action: "nc.generate",
      status: "succeeded",
      started_at: legacyStarted,
      finished_at: legacyStarted + 1000,
      diagnostic_count: 0,
    }),
  );
  await page.getByRole("button", { name: /执行记录/ }).first().click();
  await page.locator(".history-row").filter({ hasText: "旧加工记录" }).click();
  await page
    .getByRole("button", { name: "按历史输入重新执行", exact: true })
    .waitFor();
  assert.equal(
    await page
      .getByRole("button", { name: "按历史输入重新执行", exact: true })
      .isDisabled(),
    true,
  );
  assert.equal(
    await page.getByRole("button", { name: "恢复输入", exact: true }).isDisabled(),
    true,
  );
  assert(
    (await page.locator(".run-detail").innerText()).includes("已移除的加工意图"),
  );
  assert.equal(
    await page
      .getByRole("button", { name: "导出历史文件", exact: true })
      .isEnabled(),
    true,
  );
  assert((await content("历史生成结果").innerText()).includes("G1 X12.500"));
  await page.getByRole("button", { name: "返回记录列表", exact: true }).click();

  // 启动配置预检查不执行插件，非法 JSON 也不会被保存。
  await page.getByRole("button", { name: "插件与设置", exact: true }).click();
  await page.getByRole("button", { name: "检查配置", exact: true }).click();
  await page.getByRole("status").filter({ hasText: "配置检查通过" }).waitFor();
  await page.getByText("服务提供方和高级配置", { exact: true }).click();
  await content("启动配置 JSON").fill("{");
  await page
    .getByRole("button", { name: "验证并保存配置", exact: true })
    .click();
  await page
    .getByRole("alert")
    .filter({ hasText: "请先修复启动配置 JSON" })
    .waitFor();

  // 通用资产集合：模板集合完整往返；含退役加工集合的混合包整体拒绝。
  const out = reportDir;
  fs.mkdirSync(out, { recursive: true });
  const bundle = await (await fetch(url + "/api/v2/bundle")).json();
  assert(bundle.data.collections.template, "缺少通用模板集合");
  assert(bundle.data.collections.parameters, "缺少共享参数集合");
  assert.equal(
    bundle.data.manifest.some((entry) => entry.collection === "nc"),
    false,
  );
  await page.getByLabel("工作区资产", { exact: true }).click();
  const [exportDownload] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("menuitem", { name: "导出工作区资产", exact: true }).click(),
  ]);
  const bundleFile = path.join(temp, "bundle.json");
  await exportDownload.saveAs(bundleFile);
  // 只保留通用模板集合，验证到全新工作区的导入闭环。
  const generic = JSON.parse(JSON.stringify(bundle.data));
  delete generic.collections.parameters;
  generic.manifest = generic.manifest.filter(
    (entry) => entry.collection !== "parameters",
  );
  const genericFile = path.join(temp, "generic-bundle.json");
  fs.writeFileSync(genericFile, JSON.stringify(generic));
  const recipient = await serve("template", {
    home: path.join(temp, "recipient-home"),
    workspace: path.join(temp, "recipient-workspace"),
  });
  await page.goto(recipient);
  await page.getByLabel("工作区资产", { exact: true }).click();
  const [bundleChooser] = await Promise.all([
    page.waitForEvent("filechooser"),
    page.getByRole("menuitem", { name: "导入工作区资产", exact: true }).click(),
  ]);
  await bundleChooser.setFiles(genericFile);
  await page.getByRole("dialog", { name: "导入资产集合" }).waitFor();
  assert((await page.getByRole("dialog").innerText()).includes("template"));
  await page.getByRole("button", { name: "确认导入", exact: true }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "资产集合已导入" })
    .waitFor();
  assert.equal(
    (await request(recipient, "template.read", { name: "legacy-nc" })).data
      .asset.defaults.x,
    12.5,
  );
  // 重复导入同一集合触发写入冲突，不覆盖当前工作区。
  await page.getByLabel("工作区资产", { exact: true }).click();
  const [duplicateChooser] = await Promise.all([
    page.waitForEvent("filechooser"),
    page.getByRole("menuitem", { name: "导入工作区资产", exact: true }).click(),
  ]);
  await duplicateChooser.setFiles(genericFile);
  await page.getByRole("alert").filter({ hasText: "写入冲突" }).waitFor();
  // 混合包（含退役 nc 集合）在预检查阶段整体拒绝，不做部分导入。
  const mixed = JSON.parse(JSON.stringify(bundle.data));
  mixed.collections.nc = { machines: {}, templates: {} };
  mixed.manifest.push({
    collection: "nc",
    provider: "nc",
    version: "2.0.0",
    fingerprint: "0".repeat(64),
  });
  for (const route of ["bundle/validate", "bundle/import"]) {
    const rejected = await fetch(recipient + "/api/v2/" + route, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ bundle: mixed }),
    }).then((response) => response.json());
    assert.equal(rejected.ok, false, route);
    assert.equal(rejected.error.code, "collection_missing", route);
  }

  // 通用历史重放：环境一致时 matching，环境变化时 changed，缺环境时 unknown。
  const replayHome = path.join(temp, "replay-home");
  const replayWorkspace = path.join(temp, "replay-workspace");
  const replayUrl = await serve("template", {
    home: replayHome,
    workspace: replayWorkspace,
  });
  const renderOnce = async (x) => {
    const created = await fetch(replayUrl + "/api/v2/runs", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        action: "template.render",
        title: "重放测试",
        input: {
          source: "结果 {{ x }}",
          context: { x },
          schema: { type: "object", properties: { x: { type: "number" } } },
          defaults: {},
        },
      }),
    }).then((response) => response.json());
    const id = created.data.id;
    for (let i = 0; i < 200; i++) {
      const run = (
        await (await fetch(replayUrl + "/api/v2/runs/" + id)).json()
      ).data;
      if (!["queued", "running"].includes(run.status)) {
        assert.equal(run.status, "succeeded");
        return run;
      }
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
    throw new Error("render timeout");
  };
  await renderOnce(1);
  await renderOnce(2);
  await page.goto(replayUrl);
  await page.getByRole("button", { name: /执行记录/ }).first().click();
  await page
    .locator(".history-row")
    .filter({ has: page.locator(".run-status.succeeded") })
    .first()
    .click();
  await page
    .getByLabel("比较历史记录", { exact: true })
    .selectOption({ index: 1 });
  await page.locator(".diff-view").waitFor();
  const original = await content("历史生成结果").innerText();
  const rerunResponse = page.waitForResponse(
    (response) =>
      response.url().endsWith("/rerun") &&
      response.request().method() === "POST",
  );
  await page
    .getByRole("button", { name: "按历史输入重新执行", exact: true })
    .click();
  const rerunId = (await (await rerunResponse).json()).data.id;
  let replay;
  for (let i = 0; i < 100; i++) {
    replay = (
      await (await fetch(replayUrl + "/api/v2/runs/" + rerunId)).json()
    ).data;
    if (!["queued", "running"].includes(replay.status)) break;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  assert.equal(replay.status, "succeeded");
  assert.equal(replay.result.data.text.trim(), original.trim());
  assert.equal(replay.environment.format_version, 1);
  assert.equal(replay.replay.check.status, "matching");
  assert(replay.replay.source_run_id);
  await page.locator('[data-replay-status="matching"]').waitFor();
  // 模拟旧插件版本的历史元数据（不改动提供方）。
  const replayPath = path.join(
    replayWorkspace,
    ".nctool/runs",
    rerunId + ".json",
  );
  const historical = JSON.parse(fs.readFileSync(replayPath, "utf8"));
  historical.environment.plugins.template.version = "0.1.0";
  fs.writeFileSync(replayPath, JSON.stringify(historical));
  await page.getByRole("button", { name: "返回记录列表", exact: true }).click();
  await page.locator(".history-row").first().click();
  await page.locator('[data-replay-status="changed"]').waitFor();
  assert(
    (await page.locator('[data-replay-status="changed"]').innerText()).includes(
      "0.1.0",
    ),
  );
  await page
    .locator('[data-replay-origin="' + replay.replay.source_run_id + '"]')
    .waitFor();
  const changedResponse = page.waitForResponse(
    (response) =>
      response.url().endsWith("/rerun") &&
      response.request().method() === "POST",
  );
  await page
    .getByRole("button", { name: "按历史输入重新执行", exact: true })
    .click();
  const changedId = (await (await changedResponse).json()).data.id;
  for (let i = 0; i < 100; i++) {
    const result = (
      await (await fetch(replayUrl + "/api/v2/runs/" + changedId)).json()
    ).data;
    if (!["queued", "running"].includes(result.status)) {
      assert.equal(result.status, "succeeded");
      assert.equal(result.replay.check.status, "changed");
      assert.equal(result.replay.source_run_id, rerunId);
      assert.equal(result.result.data.text, replay.result.data.text);
      break;
    }
    if (i === 99) throw new Error("changed environment replay timeout");
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  delete historical.environment;
  fs.writeFileSync(replayPath, JSON.stringify(historical));
  await page.getByRole("button", { name: "返回记录列表", exact: true }).click();
  await page.locator(".history-row").nth(1).click();
  await page.locator('[data-replay-status="unknown"]').waitFor();

  // 通用工作台在窄屏下不横向溢出。
  await page.goto(url);
  await page
    .getByRole("button", { name: "模板库", exact: false })
    .first()
    .waitFor();
  await page.setViewportSize({ width: 1366, height: 768 });
  await page.screenshot({
    path: path.join(out, "workbench.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
    false,
  );
  await page.screenshot({ path: path.join(out, "mobile.png"), fullPage: true });
  await page.setViewportSize({ width: 1440, height: 900 });
  assert.deepEqual(errors, []);
  console.log(
    "PASS: author/operator modes, typed arrays, exact numbers, invalid JSON recovery, drafts, searchable syntax reference, history restore, real exports, plugin installation and dependency checks, retired NC navigation/capability/profile/history/config/bundle rejection without auto-migration, generic asset bundle round trip and stale export, generic replay environment drift and legacy records",
  );

}
main()
  .catch(async (error) => {
    console.error(error);
    if (page) {
      console.error((await page.locator("body").innerText()).slice(-2400));
      fs.mkdirSync(reportDir, { recursive: true });
      await page.screenshot({
        path: path.join(reportDir, "failure.png"),
        fullPage: true,
      });
    }
    process.exitCode = 1;
  })
  .finally(async () => {
    if (browser) await browser.close();
    await Promise.all(
      children.map((child) => {
        if (child.exitCode !== null) return Promise.resolve();
        return new Promise((resolve) => {
          child.once("exit", resolve);
          if (process.platform === "win32")
            execFileSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
              stdio: "ignore",
            });
          else child.kill();
        });
      }),
    );
    if (
      path.dirname(temp) !== path.resolve(os.tmpdir()) ||
      !path.basename(temp).startsWith("nctool-")
    )
      throw new Error("unexpected cleanup path");
    fs.rmSync(temp, {
      recursive: true,
      force: true,
      maxRetries: 10,
      retryDelay: 100,
    });
  });
