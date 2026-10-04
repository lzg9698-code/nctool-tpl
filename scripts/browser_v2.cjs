// Real browser acceptance for author/operator workflows and precision-preserving data.
const { chromium } = require(process.env.NCTOOL_PLAYWRIGHT || "playwright");
const { spawn, execFileSync } = require("node:child_process");
const fs = require("node:fs"),
  os = require("node:os"),
  path = require("node:path"),
  assert = require("node:assert/strict");
const root = path.resolve(__dirname, "..");
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
    await page.evaluate(() => navigator.clipboard.readText()),
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
    2,
  );
  for (const button of await page
    .getByRole("button", { name: "试跑示例", exact: true })
    .all())
    assert(await button.isDisabled());
  const syntaxOut = path.join(root, "target/syntax-guide");
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
  await page.getByRole("dialog", { name: "模板已被其他编辑修改" }).waitFor();
  assert((await page.getByRole("dialog").innerText()).includes("LOCAL DRAFT"));
  await page.getByRole("button", { name: "加载磁盘版本", exact: true }).click();
  await content("模板源码").waitFor();
  assert((await content("模板源码").innerText()).includes("DISK VERSION"));
  await page.getByLabel("更多模板操作").click();
  await page.getByRole("button", { name: "复制模板", exact: true }).click();
  await page.getByLabel("模板名称", { exact: true }).waitFor();
  await page.waitForFunction(() =>
    document.querySelector('[aria-label="模板名称"]').value.includes("副本"),
  );
  await page.getByLabel("更多模板操作").click();
  await page.getByRole("button", { name: "删除模板", exact: true }).click();
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
  // Domain contributions are proper resource and operation editors, without JSON commands.
  const out = path.join(root, "target/ui22-browser");
  fs.mkdirSync(out, { recursive: true });
  const nc = await serve("nc");
  await page.goto(nc);
  await page.getByRole("button", { name: "模板语法", exact: true }).click();
  await page.getByRole("button", { name: "插件语法", exact: true }).click();
  const pluginExamples = page.locator(".syntax-card");
  for (let i = 0; i < (await pluginExamples.count()); i++) {
    await pluginExamples
      .nth(i)
      .getByRole("button", { name: "试跑示例", exact: true })
      .click();
    await pluginExamples.nth(i).locator(".syntax-result").waitFor();
  }
  assert(
    (
      await pluginExamples.nth(1).locator(".syntax-result").innerText()
    ).includes("X12.500 Y+2.50"),
  );
  await page.getByRole("button", { name: "返回工作区", exact: true }).click();
  // Library generation selects the domain action without injecting NC into plain rendering.
  await request(nc, "template.save", {
    name: "document-nc",
    asset: {
      source: "G1 X{{ x | nc_fixed(3) }} S{{ spindle }}",
      schema: {
        type: "object",
        properties: {
          x: { type: "number", title: "位置 X" },
          tool: { type: "string", title: "刀具", enum: ["A", "B"] },
          spindle: { type: "integer", title: "转速" },
        },
        required: ["x", "tool", "spindle"],
      },
      defaults: { x: 12.5, tool: "A", spindle: 2000 },
      metadata: {
        title: "NC 工作台回归",
        output_name: "document-test.SPF",
        nc: {
          render_options: { trim_blocks: true, lstrip_blocks: true },
          specs: [
            { name: "x", kind: "number", min: 0, max: 100 },
            { name: "tool", kind: "string", options: ["A", "B"] },
            {
              name: "spindle",
              kind: "integer",
              derive: {
                from: "tool",
                table: [
                  ["A", 2000],
                  ["B", 3000],
                ],
              },
            },
          ],
        },
      },
    },
  });
  await page.reload();
  await page
    .locator(".template-item")
    .filter({ hasText: "NC 工作台回归" })
    .click();
  assert.equal(
    await page.getByLabel("模板生成方式", { exact: true }).inputValue(),
    "nc.generate",
  );
  await page.getByRole("button", { name: "编辑模板", exact: true }).click();
  await content("模板源码").fill(
    "G1 X{{ x | nc_fixed(3) }} S{{ spindle }}\n; UNSAVED",
  );
  await page
    .getByRole("button", { name: "生成文件", exact: true })
    .first()
    .click();
  await page.locator(".result-pane .run-status.succeeded").waitFor();
  assert((await content("生成结果").innerText()).includes("X12.500 S2000"));
  assert((await content("生成结果").innerText()).includes("UNSAVED"));
  const ncDownload = page.waitForEvent("download");
  await page.getByRole("button", { name: "导出文件", exact: true }).click();
  assert.equal((await ncDownload).suggestedFilename(), "document-test.SPF");
  await page
    .getByLabel("模板生成方式", { exact: true })
    .selectOption("template.render");
  assert(
    await page
      .getByRole("button", { name: "导出文件", exact: true })
      .isDisabled(),
  );
  await page
    .getByLabel("模板生成方式", { exact: true })
    .selectOption("nc.generate");
  await page.getByRole("button", { name: "使用模板", exact: true }).click();
  await page.locator('[data-field-path="/tool"] select').selectOption('"B"');
  await page
    .getByRole("button", { name: "生成文件", exact: true })
    .first()
    .click();
  await page.locator(".result-pane .run-status.succeeded").waitFor();
  await page.waitForFunction(() =>
    document
      .querySelector('.cm-content[aria-label="生成结果"]')
      ?.textContent.includes("S3000"),
  );
  assert((await content("生成结果").innerText()).includes("S3000"));
  await field("x").fill("101");
  await page
    .getByRole("button", { name: "生成文件", exact: true })
    .first()
    .click();
  await page.locator(".result-pane .run-status.failed").waitFor();
  assert.equal(await content("生成结果").count(), 0);
  assert(
    await page
      .getByRole("button", { name: "导出文件", exact: true })
      .isDisabled(),
  );
  await request(nc, "template.save", {
    name: "move-nc",
    asset: {
      source: "G1 X{{ x | nc_fixed(3) }}",
      schema: {
        type: "object",
        properties: { x: { type: "number", title: "位置 X" } },
        required: ["x"],
      },
      defaults: { x: 10 },
      metadata: { title: "移动测试" },
    },
  });
  await page.getByRole("button", { name: "机床配置", exact: true }).click();
  await page.getByLabel("机床标识", { exact: true }).fill("virtual");
  await page.getByLabel("机床厂商", { exact: true }).fill("Test");
  await page.getByLabel("机床型号", { exact: true }).fill("Virtual");
  await page.getByLabel("行号前缀", { exact: true }).fill("L");
  await page.getByRole("button", { name: "保存机床", exact: true }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "机床配置已保存" })
    .waitFor();
  await page.getByRole("button", { name: "G 代码生成", exact: true }).click();
  await page.getByLabel("加工模板", { exact: true }).selectOption("move-nc");
  await page.getByLabel("目标机床", { exact: true }).selectOption("virtual");
  await page.locator('[data-field-path="/x"] input').fill("12.5");
  await page.getByText("行号与输出格式", { exact: true }).click();
  await page.getByLabel("连续行号", { exact: true }).check();
  await page.getByRole("button", { name: "生成 NC 文件", exact: true }).click();
  await page.locator(".nc-columns .run-status.succeeded").waitFor();
  assert(
    (await content("历史生成结果").innerText()).includes("L0010 G1 X12.500"),
  );
  await page.locator(".page-content").evaluate((element) => {
    element.scrollTop = 0;
  });
  await page.screenshot({ path: path.join(out, "nc.png"), fullPage: true });
  await page.locator('[data-field-path="/x"] input').fill("15");
  assert.equal(
    await page
      .getByRole("button", { name: "导出历史文件", exact: true })
      .isDisabled(),
    true,
  );
  await page.getByRole("button", { name: "多工序程序", exact: true }).click();
  await page.getByLabel("程序名称", { exact: true }).fill("工序测试");
  await page.getByLabel("默认机床", { exact: true }).selectOption("virtual");
  await page.getByRole("button", { name: "添加工序", exact: true }).click();
  await page.getByLabel("工序模板1", { exact: true }).selectOption("move-nc");
  await page.getByLabel("复制工序1", { exact: true }).click();
  await page
    .locator(".operation-card")
    .nth(1)
    .locator('[data-field-path="/x"] input')
    .fill("20");
  await page.getByLabel("上移工序2", { exact: true }).click();
  await page.getByRole("button", { name: "生成完整程序", exact: true }).click();
  await page.locator(".nc-columns .run-status.succeeded").waitFor();
  assert(
    (await content("历史生成结果").innerText()).includes("L0010 G1 X20.000"),
  );
  await page.locator(".page-content").evaluate((element) => {
    element.scrollTop = 0;
  });
  await page.screenshot({
    path: path.join(out, "process.png"),
    fullPage: true,
  });
  const op = page.locator(".operation-card").first();
  await op.getByRole("button", { name: "临时源码", exact: true }).click();
  await content("工序源码1").fill("X{{ missing }}");
  await page.getByRole("button", { name: "生成完整程序", exact: true }).click();
  await page.locator(".nc-columns .run-status.failed").waitFor();
  assert.equal(
    await page
      .getByRole("button", { name: "导出历史文件", exact: true })
      .count(),
    0,
  );
  assert((await page.locator(".operation-card.failed").count()) > 0);
  // Configuration dependency prechecks block an invalid save and never execute plugins.
  await page.getByRole("button", { name: "插件与设置", exact: true }).click();
  await page.getByLabel("默认功能", { exact: true }).selectOption("nc");
  await page.getByRole("button", { name: "检查配置", exact: true }).click();
  await page.getByRole("status").filter({ hasText: "配置检查通过" }).waitFor();
  await page
    .locator(".plugin-config-card")
    .filter({ hasText: "通用模板" })
    .getByRole("button", { name: "设为停用", exact: true })
    .click();
  await page.getByRole("alert").filter({ hasText: "需要服务" }).waitFor();
  await page.getByText("服务提供方和高级配置", { exact: true }).click();
  await content("启动配置 JSON").fill("{");
  await page
    .getByRole("button", { name: "验证并保存配置", exact: true })
    .click();
  await page
    .getByRole("alert")
    .filter({ hasText: "请先修复启动配置 JSON" })
    .waitFor();
  // Comparing generated histories works independently of the current editor state.
  await page
    .getByRole("button", { name: /执行记录/ })
    .first()
    .click();
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
  const rerun = (await (await rerunResponse).json()).data;
  let replay;
  for (let i = 0; i < 100; i++) {
    replay = (await (await fetch(nc + "/api/v2/runs/" + rerun.id)).json()).data;
    if (!["queued", "running"].includes(replay.status)) break;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  assert.equal(replay.status, "succeeded");
  assert.equal(replay.result.data.text.trim(), original.trim());
  assert.equal(replay.environment.format_version, 1);
  assert.equal(replay.replay.check.status, "matching");
  assert(replay.replay.source_run_id);
  await page.locator('[data-replay-status="matching"]').waitFor();
  // Simulate historical version metadata from an older plugin without modifying the provider.
  const replayPath = path.join(workspace, ".nctool/runs", rerun.id + ".json");
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
  const changedReplayResponse = page.waitForResponse(
    (response) =>
      response.url().endsWith("/rerun") &&
      response.request().method() === "POST",
  );
  await page
    .getByRole("button", { name: "按历史输入重新执行", exact: true })
    .click();
  const changedReplayId = (await (await changedReplayResponse).json()).data.id;
  for (let i = 0; i < 100; i++) {
    const result = (
      await (await fetch(nc + "/api/v2/runs/" + changedReplayId)).json()
    ).data;
    if (!["queued", "running"].includes(result.status)) {
      assert.equal(result.status, "succeeded");
      assert.equal(result.replay.check.status, "changed");
      assert.equal(result.replay.source_run_id, rerun.id);
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
  // Every contributed asset store participates in a complete workspace package.
  const bundle = await (await fetch(nc + "/api/v2/bundle")).json();
  assert(bundle.data.collections.template && bundle.data.collections.nc);
  assert(bundle.data.manifest.some((entry) => entry.collection === "nc"));
  await page.screenshot({
    path: path.join(out, "history.png"),
    fullPage: true,
  });
  const exportDownload = page.waitForEvent("download");
  await page
    .getByRole("button", { name: "导出工作区资产", exact: true })
    .click();
  const bundleFile = path.join(temp, "bundle.json");
  await (await exportDownload).saveAs(bundleFile);
  const recipient = await serve("nc", {
    home: path.join(temp, "recipient-home"),
    workspace: path.join(temp, "recipient-workspace"),
  });
  await page.goto(recipient);
  await page
    .getByRole("button", { name: "导入工作区资产", exact: true })
    .waitFor();
  await page
    .locator('input[type="file"][accept=".json"]')
    .setInputFiles(bundleFile);
  await page.getByRole("dialog", { name: "导入资产集合" }).waitFor();
  assert(
    (await page.getByRole("dialog").innerText()).includes("move-nc → move-nc"),
  );
  await page.getByRole("button", { name: "确认导入", exact: true }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "资产集合已导入" })
    .waitFor();
  assert.equal(
    (await request(recipient, "machine.read", { name: "virtual" })).data.machine
      .config.line_number_prefix,
    "L",
  );
  assert.equal(
    (await request(recipient, "template.read", { name: "move-nc" })).data.asset
      .defaults.x,
    10,
  );
  await page
    .locator('input[type="file"][accept=".json"]')
    .setInputFiles(bundleFile);
  await page.getByRole("alert").filter({ hasText: "写入冲突" }).waitFor();
  await page.getByRole("button", { name: "多工序程序", exact: true }).click();
  await page.getByRole("button", { name: "添加工序", exact: true }).click();
  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
    false,
  );
  await page.screenshot({
    path: path.join(out, "process-mobile.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 1440, height: 900 });
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
  assert.deepEqual(errors, []);
  console.log(
    "PASS: author/operator modes, typed arrays, exact numbers, invalid JSON recovery, drafts, searchable syntax reference and 16 rendered examples, history, real exports, plugin installation and dependency checks, NC/machines/process views, stale results, history diff, replay environment drift and legacy records, asset bundles",
  );
}
main()
  .catch(async (error) => {
    console.error(error);
    if (page) {
      console.error((await page.locator("body").innerText()).slice(-2400));
      fs.mkdirSync(path.join(root, "target/ui22-browser"), { recursive: true });
      await page.screenshot({
        path: path.join(root, "target/ui22-browser/failure.png"),
        fullPage: true,
      });
    }
    process.exitCode = 1;
  })
  .finally(async () => {
    if (browser) await browser.close();
    for (const child of children) child.kill("SIGTERM");
    fs.rmSync(temp, { recursive: true, force: true });
  });
