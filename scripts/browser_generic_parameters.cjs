// Real-browser acceptance for the generic shared-parameter chain and the optional
// math extension. Runs against a fixed NCTOOL_BINARY with a disposable
// home/workspace on a random loopback port; never reads or writes user assets.
const { chromium } = require(process.env.NCTOOL_PLAYWRIGHT || "playwright");
const { spawn, execFileSync } = require("node:child_process");
const fs = require("node:fs"),
  os = require("node:os"),
  path = require("node:path"),
  assert = require("node:assert/strict");
const root = path.resolve(__dirname, "..");
const reportDir =
  process.env.NCTOOL_REPORT ||
  path.join(root, "target/generic-parameters-browser");
const binary =
  process.env.NCTOOL_BINARY ||
  path.join(
    root,
    "target/debug/nctool" + (process.platform === "win32" ? ".exe" : ""),
  );
const temp = fs.mkdtempSync(path.join(os.tmpdir(), "nctool-generic-params-"));
const home = path.join(temp, "home"),
  workspace = path.join(temp, "workspace");
const children = [];
let browser, page;
const errors = [];
const checks = [];
let runPosts = 0;
const step = (name) => checks.push(name);
const cli = (...args) =>
  JSON.parse(
    execFileSync(binary, ["--home", home, "--workspace", workspace, ...args], {
      cwd: root,
      encoding: "utf8",
    }),
  );
function serve(profile) {
  return new Promise((resolve, reject) => {
    const child = spawn(
      binary,
      [
        "--home",
        home,
        "--workspace",
        workspace,
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
        resolve({ url: match[0], child });
      }
    });
    child.on("error", reject);
    child.once("exit", (code) => {
      clearTimeout(timer);
      if (code) reject(new Error(log));
    });
  });
}
async function stopServer(child) {
  if (!child || child.exitCode !== null) return;
  await new Promise((resolve) => {
    child.once("exit", resolve);
    if (process.platform === "win32")
      execFileSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
        stdio: "ignore",
      });
    else child.kill();
  });
}
// invoke an action and return its ActionResult ({data, artifacts, diagnostics}).
async function action(url, id, input) {
  const response = await fetch(url + "/api/v2/actions/" + id, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ input }),
  });
  const result = await response.json();
  if (!result.ok) throw new Error(result.error.message);
  return result.data;
}
// raw host-API call; returns the {ok,data} envelope so failures stay inspectable.
async function send(url, route, body) {
  return fetch(url + "/api/v2/" + route, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body ?? {}),
  }).then((response) => response.json());
}
async function task(url, id) {
  for (let i = 0; i < 200; i++) {
    const run = (await fetch(url + "/api/v2/runs/" + id).then((r) => r.json()))
      .data;
    if (!["queued", "running"].includes(run.status)) return run;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error("task timeout");
}
const content = (label) =>
  page.locator('.cm-content[aria-label="' + label + '"]');
const field = (name) =>
  page.locator('[data-field-path="/' + name + '"] > input');
async function expectField(name, value) {
  await page.waitForFunction(
    ([name, value]) => {
      const input = document.querySelector(
        '[data-field-path="/' + name + '"] > input',
      );
      return !!input && input.value === value;
    },
    [name, value],
  );
}
async function expectOrigin(pointer, label) {
  await page.waitForFunction(
    ([pointer, label]) =>
      [...document.querySelectorAll(".shared-parameter-origins li")].some(
        (li) =>
          li.textContent.includes(pointer) && li.textContent.includes(label),
      ),
    [pointer, label],
  );
}
async function waitExportEnabled() {
  await page.waitForFunction(() =>
    [...document.querySelectorAll("button")].some(
      (button) =>
        button.textContent.trim() === "导出文件" && !button.disabled,
    ),
  );
}
async function waitExportDisabled() {
  await page.waitForFunction(() =>
    [...document.querySelectorAll("button")].some(
      (button) => button.textContent.trim() === "导出文件" && button.disabled,
    ),
  );
}
function openSyntax() {
  return page
    .getByRole("button", { name: "模板语法", exact: true })
    .click()
    .then(() => page.getByRole("heading", { name: "模板语法", exact: true }).waitFor());
}
async function mathCard() {
  await page.getByRole("button", { name: "插件语法", exact: true }).click();
  const card = page
    .locator(".syntax-card")
    .filter({ hasText: "数学扩展与角度单位" });
  await card.waitFor();
  return card;
}
async function main() {
  // The math chain needs the optional extension; otherwise only the shared chain runs.
  const inventory = cli("plugins", "list");
  const mathCapable = inventory.math_available === true;
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
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    if (
      request.method() === "POST" &&
      request.url().endsWith("/api/v2/runs")
    )
      runPosts++;
  });
  fs.mkdirSync(reportDir, { recursive: true });

  // ---- Shared parameters: seed definitions, a preset and a bound template ----
  const server = await serve("template");
  const url = server.url;
  await action(url, "parameter.save", {
    group: "definitions",
    name: "part-width",
    expected: null,
    asset: {
      format_version: 1,
      id: "part-width",
      title: "宽度",
      unit: "mm",
      schema: { type: "number", exclusiveMinimum: 0, maximum: 100 },
      default: 20,
    },
  });
  await action(url, "parameter.save", {
    group: "definitions",
    name: "part-depth",
    expected: null,
    asset: {
      format_version: 1,
      id: "part-depth",
      title: "深度",
      unit: "mm",
      schema: { type: "number", exclusiveMinimum: 0 },
      default: 6,
    },
  });
  await action(url, "parameter.save", {
    group: "presets",
    name: "rough",
    expected: null,
    asset: {
      format_version: 1,
      id: "rough",
      values: { "part-width": 5, "part-depth": 9 },
    },
  });
  await action(url, "template.save", {
    name: "shared-demo",
    expected: null,
    asset: {
      source: "宽度 {{ width }} mm，深度 {{ depth }} mm",
      schema: {
        type: "object",
        properties: {
          width: { type: "number", title: "宽度", "x-unit": "mm" },
          depth: { type: "number", title: "深度", "x-unit": "mm" },
        },
      },
      defaults: { width: 30 },
      metadata: {
        title: "共享参数示例",
        parameters: {
          bindings: {
            "/width": { id: "part-width", unit: "mm" },
            "/depth": { id: "part-depth", unit: "mm" },
          },
        },
      },
      tags: [],
    },
  });
  const catalog = await action(url, "template.catalog", {});
  assert(
    catalog.data.items.some((item) => item.id === "shared-demo"),
    "共享参数模板未保存",
  );

  await page.goto(url);
  await page
    .locator(".template-item")
    .filter({ hasText: "共享参数示例" })
    .click();
  // The document routes to the real shared-parameter inputs, not the plain form.
  await page.locator('[data-field-path="/width"] > input').waitFor();
  await page.locator(".shared-parameter-origins").waitFor();
  await expectField("width", "30");
  await expectField("depth", "6");
  await expectOrigin("/width", "模板默认值");
  await expectOrigin("/depth", "共享定义默认值");
  await page.getByText("当前参数已通过解析校验", { exact: true }).waitFor();
  step("继承值与来源（模板默认值/共享定义默认值）");

  // A manual edit overrides one bound field only.
  await field("width").fill("12");
  await expectField("width", "12");
  await expectOrigin("/width", "本次填写");
  const widthRow = page
    .locator(".shared-parameter-origins li")
    .filter({ hasText: "/width" });
  await widthRow.getByRole("button", { name: "恢复跟随", exact: true }).waitFor();
  step("手改字段并保留本次覆盖");

  // Selecting a shared preset moves the untouched bound field but keeps the override.
  await page.getByLabel("共享预置", { exact: true }).selectOption("rough");
  await expectField("depth", "9");
  await expectOrigin("/depth", "共享预置：rough");
  await expectField("width", "12");
  await expectOrigin("/width", "本次填写");
  step("切换共享预置且保留本次覆盖");
  await page.locator(".parameters-pane").screenshot({
    path: path.join(reportDir, "shared-parameters.png"),
  });

  // Restoring follow drops the override and falls back to the preset value.
  await widthRow.getByRole("button", { name: "恢复跟随", exact: true }).click();
  await expectField("width", "5");
  await expectOrigin("/width", "共享预置：rough");
  step("恢复共享值（恢复跟随）");

  // Declining the preset returns both fields to inherited defaults.
  await page.getByLabel("共享预置", { exact: true }).selectOption("");
  await expectField("width", "30");
  await expectField("depth", "6");
  await expectOrigin("/width", "模板默认值");
  await expectOrigin("/depth", "共享定义默认值");
  step("取消共享预置");

  // An incomplete number blocks execution and keeps the export disabled.
  let before = runPosts;
  await field("width").fill("1e");
  await page
    .getByRole("button", { name: "生成文件", exact: true })
    .first()
    .click();
  await page
    .getByRole("alert")
    .filter({ hasText: "请先填写完整、有效的数值" })
    .waitFor();
  assert.equal(runPosts, before, "未完成数值不应提交任务");
  await waitExportDisabled();
  step("未完成数值阻止执行");

  // Malformed raw JSON is retained and never dispatched.
  await page.getByRole("button", { name: "JSON", exact: true }).click();
  await content("本次参数 JSON").fill('{"width":');
  await page
    .getByRole("button", { name: "生成文件", exact: true })
    .first()
    .click();
  await page
    .getByRole("alert")
    .filter({ hasText: "请先修正 JSON 输入" })
    .waitFor();
  assert.equal(runPosts, before, "无效 JSON 不应提交任务");
  step("无效 JSON 阻止执行");

  // A corrected value generates successfully; only the override is sent.
  await page.getByRole("button", { name: "表单", exact: true }).click();
  await field("width").fill("8");
  const successPost = page.waitForRequest(
    (request) =>
      request.method() === "POST" && request.url().endsWith("/api/v2/runs"),
  );
  await page
    .getByRole("button", { name: "生成文件", exact: true })
    .first()
    .click();
  const successBody = (await successPost).postDataJSON();
  await page.locator(".result-pane .run-status.succeeded").waitFor();
  assert(
    (await content("生成结果").innerText()).includes("宽度 8 mm，深度 6 mm"),
    "生成结果与解析值不一致",
  );
  assert.equal(successBody.action, "template.render.managed");
  assert.deepEqual(successBody.input.context, { width: 8 });
  assert.equal(successBody.input.preset_id, null);
  assert.deepEqual(successBody.input.bindings, {
    "/width": { id: "part-width", unit: "mm" },
    "/depth": { id: "part-depth", unit: "mm" },
  });
  step("修正后生成成功且只发送本次覆盖");

  // Editing a bound definition invalidates the frozen result and blocks export.
  await waitExportEnabled();
  const definition = (
    await action(url, "parameter.read", {
      group: "definitions",
      name: "part-width",
    })
  ).data;
  await action(url, "parameter.save", {
    group: "definitions",
    name: "part-width",
    expected: definition.fingerprint,
    asset: { ...definition.asset, default: 21 },
  });
  await page.locator(".result-pane .run-status.stale").waitFor();
  await waitExportDisabled();
  assert(
    (await page.locator(".stale-banner").innerText()).includes(
      "结果对应旧输入，请重新生成以应用当前修改。",
    ),
    "过期提示缺失",
  );
  step("修改依赖使旧结果过期且禁止导出");

  // ---- Math extension is off by default: the syntax example cannot run yet ----
  await openSyntax();
  let card = await mathCard();
  const runExample = card.getByRole("button", { name: "试跑示例", exact: true });
  assert.equal(await runExample.isDisabled(), true, "未启用 math 时试跑应禁用");
  assert(
    (await card.innerText()).includes(
      "当前未启用所需插件，请在插件与设置中启用并重启。",
    ),
    "缺少未启用提示",
  );
  step("math 未启用时语法示例试跑禁用");

  const summary = `PASS (shared parameters only): ${checks.join("、")}；math 链因二进制未包含扩展而跳过`;
  if (!mathCapable) {
    console.log(
      "LIMITATION: 固定二进制未包含 math 扩展（需 --features math 构建）；已跳过 math 链，仅验证共享参数链。",
    );
  } else {
    // ---- Enable math, restart our own temporary server (applies next start) ----
    await stopServer(server.child);
    await cli("plugins", "enable", "math");
    const restarted = await serve("template");
    const mathUrl = restarted.url;
    await action(mathUrl, "template.save", {
      name: "math-demo",
      expected: null,
      asset: {
        source: "平方根：{{ 9 | sqrt }}",
        schema: { type: "object", properties: {} },
        defaults: {},
        metadata: {
          title: "数学模板",
          execution: { action: "template.render", extensions: ["math"] },
        },
        tags: [],
      },
    });

    // The syntax page can now run the math example, and the POST carries the extension.
    await page.goto(mathUrl);
    await openSyntax();
    card = await mathCard();
    const enabledExample = card.getByRole("button", {
      name: "试跑示例",
      exact: true,
    });
    assert.equal(await enabledExample.isDisabled(), false, "启用 math 后试跑应可用");
    const examplePost = page.waitForRequest(
      (request) =>
        request.method() === "POST" &&
        request.url().endsWith("/api/v2/actions/template.render"),
    );
    await enabledExample.click();
    const exampleBody = (await examplePost).postDataJSON();
    assert.deepEqual(exampleBody.input.extensions, ["math"]);
    await card.locator(".syntax-result").waitFor();
    assert.equal(await card.getByRole("alert").count(), 0);
    const syntaxOutput = await card.locator(".syntax-result pre").innerText();
    assert(syntaxOutput.includes("平方根：3"), "math 示例输出缺少开方结果");
    assert(syntaxOutput.includes("0.5"), "math 示例输出缺少角度正弦结果");
    await card.screenshot({ path: path.join(reportDir, "math-syntax.png") });
    step("语法页试跑 math 示例成功且携带 extensions");

    // Generating a math template posts action + extensions to the runs API.
    await page
      .getByRole("button", { name: "返回工作区", exact: true })
      .click();
    await page.locator(".template-item").filter({ hasText: "数学模板" }).click();
    const runsPost = page.waitForRequest(
      (request) =>
        request.method() === "POST" && request.url().endsWith("/api/v2/runs"),
    );
    await page
      .getByRole("button", { name: "生成文件", exact: true })
      .first()
      .click();
    const runsBody = (await runsPost).postDataJSON();
    assert.equal(runsBody.action, "template.render");
    assert.deepEqual(runsBody.input.extensions, ["math"]);
    await page.locator(".result-pane .run-status.succeeded").waitFor();
    assert((await content("生成结果").innerText()).includes("平方根：3"));
    step("工作台生成 math 模板 POST runs 携带 extensions ['math']");

    // Host HTTP API: without extensions the filter is unknown; with it, it renders.
    const withoutExtensions = await send(mathUrl, "actions/template.render", {
      input: { source: "{{ 9 | sqrt }}", context: {} },
    });
    assert.equal(withoutExtensions.ok, false);
    assert.equal(withoutExtensions.error.code, "template_error");
    assert(
      /unknown filter|sqrt/.test(withoutExtensions.error.message),
      "未知过滤器错误信息缺失",
    );
    const withExtensions = await action(mathUrl, "template.render", {
      source: "{{ 9 | sqrt }}",
      context: {},
      extensions: ["math"],
    });
    assert.equal(withExtensions.data.text, "3.0");
    step("HTTP template.render 无 extensions 报未知过滤器、有 extensions 成功");

    // A background template.render task records the extension; rerunning keeps it
    // and never rewrites the original record.
    const created = await send(mathUrl, "runs", {
      action: "template.render",
      title: "数学后台任务",
      input: { source: "{{ 9 | sqrt }}", context: {}, extensions: ["math"] },
    });
    const runId = created.data.id;
    const original = await task(mathUrl, runId);
    assert.equal(original.status, "succeeded");
    assert.deepEqual(original.input.extensions, ["math"]);
    assert.equal(original.result.data.text, "3.0");
    const runPath = path.join(workspace, ".nctool", "runs", runId + ".json");
    const originalBytes = fs.readFileSync(runPath, "utf8");
    const rerun = await send(mathUrl, "runs/" + runId + "/rerun", {});
    const rerunRun = await task(mathUrl, rerun.data.id);
    assert.equal(rerunRun.status, "succeeded");
    assert.deepEqual(rerunRun.input.extensions, ["math"]);
    assert.equal(rerunRun.result.data.text, "3.0");
    assert.equal(
      fs.readFileSync(runPath, "utf8"),
      originalBytes,
      "重执行改写了原历史文件",
    );
    step("后台 math 任务记录扩展、重执行仍扩展且旧文件不变");
  }

  assert.deepEqual(errors, []);
  fs.writeFileSync(
    path.join(reportDir, "report.json"),
    JSON.stringify({ math: mathCapable, checks }, null, 2),
  );
  console.log(mathCapable ? "PASS: " + checks.join("；") : summary);
}
main()
  .catch(async (error) => {
    console.error(error);
    if (page) {
      try {
        console.error((await page.locator("body").innerText()).slice(-2400));
      } catch {}
      fs.mkdirSync(reportDir, { recursive: true });
      await page
        .screenshot({ path: path.join(reportDir, "failure.png"), fullPage: true })
        .catch(() => {});
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
          if (process.platform === "win32") {
            try {
              execFileSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
                stdio: "ignore",
              });
            } catch {
              resolve();
            }
          } else child.kill();
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
