// End-to-end regressions for the 2026-10-02 review; uses isolated home/workspace.
const { chromium } = require(process.env.NCTOOL_PLAYWRIGHT || "playwright");
const { spawn, execFileSync } = require("node:child_process");
const fs = require("node:fs"),
  os = require("node:os"),
  path = require("node:path"),
  assert = require("node:assert/strict");
const root = path.resolve(__dirname, ".."),
  binary =
    process.env.NCTOOL_BINARY ||
    path.join(
      root,
      "target/debug/nctool" + (process.platform === "win32" ? ".exe" : ""),
    );
const temp = fs.mkdtempSync(path.join(os.tmpdir(), "nctool-review-fixes-")),
  home = path.join(temp, "home"),
  workspace = path.join(temp, "ws");
const children = [];
let browser, page;
const cli = (...args) =>
  JSON.parse(
    execFileSync(binary, ["--home", home, "--workspace", workspace, ...args], {
      cwd: root,
      encoding: "utf8",
    }),
  );
async function serve(profile) {
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
    child.stderr.on("data", (d) => {
      log += d;
      const m = log.match(/http:\/\/127\.0\.0\.1:\d+/);
      if (m) {
        clearTimeout(timer);
        resolve(m[0]);
      }
    });
    child.on("error", reject);
    child.on("exit", (code) => {
      clearTimeout(timer);
      if (code) reject(new Error(log));
    });
  });
}
async function api(url, route, body) {
  const r = await fetch(url + "/api/v2/" + route, {
    method: body === undefined ? "GET" : "POST",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  return r.json();
}
async function action(url, id, input) {
  const r = await api(url, "actions/" + id, { input });
  assert.equal(r.ok, true, JSON.stringify(r));
  return r.data;
}
async function editJson(text) {
  const target = content("插件输入 JSON");
  await target.click();
  await target.press("ControlOrMeta+a");
  await target.press("Backspace");
  await page.keyboard.insertText(text);
  await target.waitFor();
  assert.equal(await target.innerText(), text);
}
const content = (label) =>
  page.locator('.cm-content[aria-label="' + label + '"]');
async function taskAfter(button) {
  const started = page.waitForResponse(
    (r) => r.url().endsWith("/api/v2/runs") && r.request().method() === "POST",
  );
  await button.click();
  return (await (await started).json()).data.id;
}
async function finished(url, id) {
  for (let i = 0; i < 200; i++) {
    const r = (await api(url, "runs/" + id)).data;
    if (!["queued", "running"].includes(r.status)) return r;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error("task timeout");
}
async function main() {
  // A disposable provider adds a second action and a cancellable slow invocation.
  const fixture = path.join(temp, "provider");
  fs.cpSync(path.join(root, "examples/plugins/python-report"), fixture, {
    recursive: true,
  });
  const manifestPath = path.join(fixture, "plugin.json");
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  const alternative = structuredClone(manifest.actions[0]);
  alternative.id = "report.alternate";
  alternative.title = "生成另一份报告";
  manifest.actions.push(alternative);
  manifest.descriptor.panels[0].actions.push(alternative.id);
  fs.writeFileSync(manifestPath, JSON.stringify(manifest));
  const providerPath = path.join(fixture, "plugin.py");
  fs.writeFileSync(
    providerPath,
    fs
      .readFileSync(providerPath, "utf8")
      .replace(
        'if action != "report.compute":',
        'if action not in ["report.compute", "report.alternate"]:',
      )
      .replace(
        "    total = sum",
        '    if input_data.get("title") == "slow":\n        import time\n        time.sleep(2)\n    total = sum',
      ),
  );
  cli("plugins", "install", fixture);
  cli("plugins", "enable", "python-report");
  browser = await chromium.launch({
    headless: true,
    ...(process.env.NCTOOL_CHROMIUM
      ? { executablePath: process.env.NCTOOL_CHROMIUM }
      : {}),
    args: ["--no-sandbox"],
  });
  page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const url = await serve("template");
  await action(url, "template.save", {
    name: "nc-marker",
    asset: {
      source: "G1 X{{ x }}",
      schema: {
        type: "object",
        properties: { x: { type: "number" } },
        required: ["x"],
      },
      defaults: { x: -10 },
      metadata: {
        title: "NC marker",
        nc: { specs: [{ name: "x", kind: "number", min: 0 }] },
      },
    },
  });
  await page.goto(url);
  await page
    .locator(".recent-cards button")
    .filter({ hasText: "NC marker" })
    .click();
  assert.equal(
    await page.getByLabel("模板生成方式", { exact: true }).inputValue(),
    "",
  );
  let submitted = 0;
  const observe = (request) => {
    if (request.url().endsWith("/api/v2/runs") && request.method() === "POST")
      submitted++;
  };
  page.on("request", observe);
  await page
    .getByRole("button", { name: "生成文件", exact: true })
    .first()
    .click();
  await page.getByRole("alert").filter({ hasText: "不可用" }).first().waitFor();
  assert.equal(submitted, 0);
  // Explicit text selection is persisted, never implicitly inferred after plugin removal.
  await page
    .getByLabel("模板生成方式", { exact: true })
    .selectOption("template.render");
  const save = page.waitForResponse((r) =>
    r.url().endsWith("/api/v2/actions/template.save"),
  );
  await page.getByRole("button", { name: "保存", exact: true }).first().click();
  await save;
  assert.equal(
    (await action(url, "template.read", { name: "nc-marker" })).data.asset
      .metadata.execution.action,
    "template.render",
  );
  await page.getByRole("button", { name: "数据报告", exact: true }).click();
  await page.getByText("高级 JSON 输入", { exact: true }).click();
  await editJson('{"values":');
  await page.getByRole("button", { name: "执行任务", exact: true }).click();
  await page
    .getByRole("alert")
    .filter({ hasText: "修复插件输入" })
    .first()
    .waitFor();
  assert.equal(submitted, 0);
  // Page navigation and reload both preserve the invalid raw draft.
  await page
    .getByRole("button", { name: /执行记录/ })
    .first()
    .click();
  await page.getByRole("button", { name: "数据报告", exact: true }).click();
  await page.getByText("高级 JSON 输入", { exact: true }).click();
  assert.equal(await content("插件输入 JSON").innerText(), '{"values":');
  await page.reload();
  await page.getByRole("button", { name: "数据报告", exact: true }).click();
  await page.getByText("高级 JSON 输入", { exact: true }).click();
  assert.equal(await content("插件输入 JSON").innerText(), '{"values":');
  for (const raw of ["[]", "null", '{"values":[-]}']) {
    await editJson(raw);
    await page.getByRole("button", { name: "执行任务", exact: true }).click();
    await page
      .getByRole("alert")
      .filter({ hasText: /修复插件输入|JSON 对象/ })
      .first()
      .waitFor();
    assert.equal(submitted, 0);
  }
  await editJson('{"values":[999],"title":"new input"}');
  let id = await taskAfter(
    page.getByRole("button", { name: "执行任务", exact: true }),
  );
  let run = await finished(url, id);
  assert.equal(run.status, "succeeded");
  assert.deepEqual(run.input.values, [999]);
  await page.locator(".plugin-columns .run-status.succeeded").waitFor();
  assert.equal(
    await page
      .getByRole("button", { name: "导出历史文件", exact: true })
      .isEnabled(),
    true,
  );
  await editJson('{"values":[1],"title":"edited input"}');
  assert.equal(
    await page
      .getByRole("button", { name: "导出历史文件", exact: true })
      .isDisabled(),
    true,
  );
  id = await taskAfter(
    page.getByRole("button", { name: "执行任务", exact: true }),
  );
  assert.equal((await finished(url, id)).status, "succeeded");
  await page.locator(".plugin-columns .run-status.succeeded").waitFor();
  assert.equal(
    await page
      .getByRole("button", { name: "导出历史文件", exact: true })
      .isEnabled(),
    true,
  );
  await page
    .locator(".plugin-columns .panel-header select")
    .selectOption("report.alternate");
  assert.equal(
    await page
      .getByRole("button", { name: "导出历史文件", exact: true })
      .count(),
    0,
  );
  id = await taskAfter(
    page.getByRole("button", { name: "执行任务", exact: true }),
  );
  run = await finished(url, id);
  assert.equal(run.action, "report.alternate");
  assert.equal(run.status, "succeeded");
  await page.locator(".plugin-columns .run-status.succeeded").waitFor();
  await editJson('{"values":[1],"title":"slow"}');
  id = await taskAfter(
    page.getByRole("button", { name: "执行任务", exact: true }),
  );
  assert.equal(
    (await api(url, "runs/" + id + "/cancel", {})).data.cancelled,
    true,
  );
  assert.equal((await finished(url, id)).status, "cancelled");
  await page.locator(".plugin-columns .run-status.cancelled").waitFor();
  assert.equal(
    await page
      .getByRole("button", { name: "导出历史文件", exact: true })
      .count(),
    0,
  );
  await editJson('{"values":[1e308,1e308]}');
  id = await taskAfter(
    page.getByRole("button", { name: "执行任务", exact: true }),
  );
  run = await finished(url, id);
  assert.equal(run.status, "failed");
  await page.locator(".plugin-columns .run-status.failed").waitFor();
  assert.equal(
    await page
      .getByRole("button", { name: "导出历史文件", exact: true })
      .count(),
    0,
  );
  const out = path.join(root, "target/fixes-browser");
  fs.mkdirSync(out, { recursive: true });
  await page.screenshot({ path: path.join(out, "plugin.png"), fullPage: true });
  const nc = await serve("nc");
  await action(nc, "template.save", {
    name: "schema-nc",
    asset: {
      source: "G1 X{{ x | nc_fixed(3) }}",
      schema: {
        type: "object",
        properties: { x: { type: "number", minimum: 0 } },
        required: ["x"],
        additionalProperties: false,
      },
      defaults: { x: 10 },
      metadata: { title: "Schema NC", nc: {} },
    },
  });
  await page.goto(nc);
  await page
    .locator(".recent-cards button")
    .filter({ hasText: "Schema NC" })
    .click();
  await page.locator('[data-field-path="/x"] input').fill("-10");
  id = await taskAfter(
    page.getByRole("button", { name: "生成文件", exact: true }).first(),
  );
  run = await finished(nc, id);
  assert.equal(run.status, "failed");
  assert.equal(run.result, null);
  await page.locator(".result-pane .run-status.failed").waitFor();
  await page.locator('[data-field-path="/x"] input').fill("10");
  id = await taskAfter(
    page.getByRole("button", { name: "生成文件", exact: true }).first(),
  );
  run = await finished(nc, id);
  assert.equal(run.status, "succeeded");
  assert.equal(run.result.data.text, "G1 X10.000\n");
  await page.locator(".result-pane .run-status.succeeded").waitFor();
  const replay = (await api(nc, "runs/" + id + "/rerun", {})).data.id;
  assert.equal((await finished(nc, replay)).result.data.text, "G1 X10.000\n");
  await page.screenshot({
    path: path.join(out, "schema-nc.png"),
    fullPage: true,
  });
  // Real HTTP numeric ingress rejects both underflow and oversized integer literals.
  for (const literal of ["18446744073709551617", "-9223372036854775809"]) {
    const r = await fetch(nc + "/api/v2/actions/template.render", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: '{"input":{"source":"{{ x }}","context":{"x":' + literal + "}}}",
    });
    const data = await r.json();
    assert.equal(data.ok, false);
    assert.equal(data.error.code, "numeric_range");
  }
  assert.deepEqual(errors, []);
  console.log(
    "PASS: missing NC capability blocks fallback; explicit intent persists; invalid/wrong-type raw JSON never dispatches and survives navigation/reload; submitted values are current; stale/failed plugin output cannot export; Schema constraints and replay agree; HTTP integer ingress rejects silent rounding",
  );
}
main()
  .catch(async (error) => {
    console.error(error);
    if (page) {
      try {
        console.error((await page.locator("body").innerText()).slice(-2500));
      } catch {}
    }
    process.exitCode = 1;
  })
  .finally(async () => {
    if (browser) await browser.close();
    for (const child of children) child.kill("SIGINT");
    fs.rmSync(temp, { recursive: true, force: true });
  });
