// Full Jinja/NC editing inside dedicated plugins, isolated assets and real HTTP.
const { chromium } = require(process.env.NCTOOL_PLAYWRIGHT || "playwright");
const { spawn } = require("node:child_process");
const fs = require("node:fs"),
  path = require("node:path"),
  os = require("node:os"),
  assert = require("node:assert/strict"),
  crypto = require("node:crypto");
const root = path.resolve(__dirname, ".."),
  binary =
    process.env.NCTOOL_BINARY ||
    path.join(root, "target/ap2-auto/build-nc/debug/nctool.exe");
const report =
  process.env.NCTOOL_REPORT ||
  path.join(root, "target/specialized-template-editor/browser");
const temp = fs.mkdtempSync(path.join(os.tmpdir(), "specialized-template-")),
  passed = [],
  errors = [];
let server, browser, page, url;
fs.mkdirSync(report, { recursive: true });
const active = () => page.locator(".turning-side:visible");
const editor = () => active().locator(".turning-template-editor");
async function api(route, body) {
  const r = await fetch(url + "/api/v2/" + route, {
    method: body ? "POST" : "GET",
    headers: body ? { "Content-Type": "application/json" } : {},
    body: body && JSON.stringify(body),
  });
  const result = await r.json();
  assert(result.ok, JSON.stringify(result.error));
  return result.data;
}
const action = async (id, input) =>
  (await api("actions/" + id, { input })).data;
async function select(side) {
  await page
    .getByRole("tab", { name: side === "right" ? "右侧" : "左侧", exact: true })
    .click();
}
async function open() {
  if (
    !(await active()
      .locator(".turning-format")
      .evaluate((el) => el.open))
  )
    await active().locator(".turning-format > summary").click();
}
async function source(text) {
  const code = editor().getByRole("textbox", {
    name: "右侧专用模板源码",
    exact: true,
  });
  await code.fill(text);
}
async function fill(label, value) {
  await page.getByLabel(label, { exact: true }).fill(String(value));
}
async function save(success = true) {
  const response = page.waitForResponse((r) =>
    r.url().endsWith("/actions/template.save"),
  );
  await editor().getByRole("button", { name: "保存模板", exact: true }).click();
  const value = await (await response).json();
  assert.equal(value.ok, success, JSON.stringify(value.error));
  if (success)
    await editor()
      .getByRole("status")
      .filter({ hasText: "模板已保存" })
      .waitFor();
  else await editor().getByRole("alert").waitFor();
}
async function generate(name, success = true) {
  const response = page.waitForResponse(
    (r) => r.url().endsWith("/api/v2/runs") && r.request().method() === "POST",
  );
  await page.getByRole("button", { name, exact: true }).click();
  const value = await (await response).json();
  assert(value.ok);
  for (let n = 0; n < 200; n++) {
    const run = await api("runs/" + value.data.id);
    if (!["queued", "running"].includes(run.status)) {
      assert.equal(
        run.status,
        success ? "succeeded" : "failed",
        JSON.stringify(run.error),
      );
      await active()
        .locator(".turning-status")
        .filter({ hasText: success ? "生成成功" : "生成失败" })
        .waitFor();
      return run;
    }
    await new Promise((r) => setTimeout(r, 50));
  }
  throw new Error("generation timeout");
}
async function main() {
  url = await new Promise((resolve, reject) => {
    server = spawn(
      binary,
      [
        "--home",
        path.join(temp, "home"),
        "--workspace",
        path.join(temp, "workspace"),
        "--profile",
        "nc",
        "ui",
        "--port",
        "0",
      ],
      { cwd: root },
    );
    let log = "";
    server.stderr.on("data", (b) => {
      log += b;
      const match = log.match(/http:\/\/127\.0\.0\.1:\d+/);
      if (match) resolve(match[0]);
    });
    server.on("error", reject);
    server.on("exit", (c) => c && reject(new Error(log)));
  });
  browser = await chromium.launch({
    headless: true,
    executablePath: process.env.NCTOOL_CHROMIUM,
    args: ["--no-sandbox"],
  });
  page = await browser.newPage({ viewport: { width: 1280, height: 950 } });
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(url);
  for (const kind of ["sync-turning", "undercut"]) {
    const sync = kind === "sync-turning",
      navigation = sync ? "同步粗车" : "越程槽",
      id = "ui-template-" + kind;
    await page.getByRole("button", { name: navigation, exact: true }).click();
    await select("right");
    await open();
    await editor()
      .getByRole("button", { name: "新建自定义模板", exact: true })
      .click();
    await fill("右侧模板 ID", id);
    await fill("右侧模板名称", navigation + "源码");
    await source("{% if broken %}");
    await editor()
      .getByRole("button", { name: "校验模板", exact: true })
      .click();
    await editor().getByRole("alert").filter({ hasText: "校验失败" }).waitFor();
    assert.equal(
      (await action("template.catalog", {})).items.length,
      sync ? 0 : 2,
    );
    const templateSource = sync
      ? "; CUSTOM {{ cut_num }}\nG0 X{{ actual_sta_x }}\n{{ trajectory_program }}"
      : "; CUSTOM GROOVE\nG1 X{{ D1_CUT }} F0.25";
    await source(templateSource);
    await editor()
      .getByRole("button", { name: "校验模板", exact: true })
      .click();
    await editor()
      .getByRole("status")
      .filter({ hasText: "语法校验通过" })
      .waitFor();
    await save();
    assert.equal(
      (await action("template.read", { name: id })).asset.source,
      templateSource,
    );
    passed.push(
      kind +
        " editable full Jinja source, syntax diagnostics, create with blank processing rows",
    );
    await page.getByLabel("右侧模板格式", { exact: true }).selectOption(id);
    await editor()
      .getByRole("button", { name: "重新读取所选模板", exact: true })
      .click();
    await source(templateSource + "\n; local draft");
    const disk = await action("template.read", { name: id });
    disk.asset.source = templateSource + "\n; external change";
    disk.asset.extra = { preserved: true };
    await action("template.save", {
      name: id,
      asset: disk.asset,
      expected: disk.fingerprint,
    });
    await save(false);
    await page.reload();
    await page.getByRole("button", { name: navigation, exact: true }).click();
    await open();
    assert(
      (
        await editor()
          .getByRole("textbox", { name: "右侧专用模板源码", exact: true })
          .innerText()
      ).includes("; local draft"),
    );
    await save(false);
    await editor()
      .getByRole("button", { name: "重新读取所选模板", exact: true })
      .click();
    await page.waitForFunction(() =>
      document
        .querySelector('[aria-label="右侧专用模板源码"]')
        .textContent.includes("; external change"),
    );
    await source(templateSource);
    await save();
    assert.deepEqual(
      (await action("template.read", { name: id })).asset.extra,
      { preserved: true },
    );
    await fill("右侧模板 ID", id + "-copy");
    await save();
    assert.equal(
      (await action("template.read", { name: id })).asset.source,
      templateSource,
    );
    passed.push(
      kind +
        " baseline conflict, draft refresh recovery, explicit reread, metadata and copy preservation",
    );
    await page.reload();
    await page.getByRole("button", { name: navigation, exact: true }).click();
    await open();
    assert.equal(await page.locator(".template-item").count(), 0);
    if (sync) {
      await fill("右侧固定 ap1（直径减量 mm）", 2);
      await fill("同步进给（按程序模式）", 0.2);
      for (const [key, value] of [
        ["起始 X", 60],
        ["起始 Z", 0],
        ["结束 X", 50],
        ["结束 Z", -10],
        ["ap2", 3],
      ])
        await fill("右侧第1行 " + key + " (mm)", value);
    } else {
      await fill("毛坯直径 (mm)", 100);
      await page
        .getByLabel("右侧第1行标准")
        .selectOption({ label: "DIN509-E 0,2 x 0,1 · r=0.2" });
      for (const [key, value] of [
        ["D1", 80],
        ["Z", 30],
        ["D2", 85],
        ["C", 0.5],
      ])
        await fill("右侧第1行 " + key + " (mm)", value);
      await fill("右侧第1行 角度 (°)", 45);
    }
    let run = await generate("生成右侧");
    assert(run.result.artifacts.every((a) => a.text.includes("; CUSTOM")));
    assert(run.result.artifacts.every((a) => !a.text.includes("{{")));
    const before = run;
    await editor()
      .getByRole("button", { name: "重新读取所选模板", exact: true })
      .click();
    await page.waitForFunction(
      (id) => document.querySelector('[aria-label="右侧模板 ID"]').value === id,
      id,
    );
    await source(templateSource + "\n; UI SAVED");
    await save();
    assert(
      await active()
        .getByRole("button", { name: "导出 SPF", exact: true })
        .first()
        .isDisabled(),
    );
    run = await generate("生成右侧");
    assert(run.result.artifacts.every((a) => a.text.includes("; UI SAVED")));
    const replay = await api("actions/" + kind + ".generate", {
      input: before.result.data.replay_input,
    });
    assert.deepEqual(replay.artifacts, before.result.artifacts);
    passed.push(
      kind +
        " saved source consumed by actual generation, immediate stale export gate, immutable snapshot replay",
    );
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 950 });
      await editor().scrollIntoViewIfNeeded();
      assert(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      );
      await editor().screenshot({
        path: path.join(report, kind + "-template-" + width + ".png"),
      });
    }
    await page.setViewportSize({ width: 1280, height: 950 });
    if (!sync) {
      await select("left");
      await open();
      await page.getByLabel("左侧模板格式", { exact: true }).selectOption(id);
      await page
        .getByLabel("左侧第1行标准")
        .selectOption({ label: "DIN509-E 0,2 x 0,1 · r=0.2" });
      for (const [key, value] of [
        ["D1", 80],
        ["Z", 30],
        ["D2", 85],
        ["C", 0.5],
      ])
        await fill("左侧第1行 " + key + " (mm)", value);
      await fill("左侧第1行 角度 (°)", 45);
      await generate("生成全部");
      await select("right");
      await source(templateSource + "\n; both expired");
      await save();
      await select("left");
      assert(
        await active()
          .getByRole("button", { name: "导出 SPF", exact: true })
          .first()
          .isDisabled(),
      );
      await select("right");
    }
    // A runtime error in a later row must discard the entire batch.
    await page
      .getByRole("button", { name: "右侧复制第1行", exact: true })
      .click();
    if (sync)
      await source(
        "{% if sync_num_start > 1 %}{{ 1 // 0 }}{% endif %}{{ trajectory_program }}",
      );
    else {
      await fill("右侧第2行 Z (mm)", 40);
      await source(
        "{% if ES_Z > 30 %}{{ 1 // 0 }}{% endif %}{{ trajectory_program }}",
      );
    }
    await save();
    run = await generate("生成右侧", false);
    assert(!run.result?.artifacts?.length);
    assert(run.error.message.includes("第 2 行"), run.error.message);
    assert(
      await active()
        .getByRole("button", { name: "导出 SPF", exact: true })
        .first()
        .isDisabled(),
    );
    await page.getByLabel("右侧模板支持专用工作台").uncheck();
    await source("{{ trajectory_program }}");
    await save();
    await generate("生成右侧", false);
    passed.push(
      kind +
        " render failure never delivers files; legacy marker rejection; desktop/mobile editor layout",
    );
  }
  assert.deepEqual(errors, []);
}
main()
  .then(() => console.log(JSON.stringify({ passed, errors }, null, 2)))
  .catch(async (e) => {
    console.error(e);
    errors.push(e.stack);
    process.exitCode = 1;
    if (page)
      await page
        .screenshot({ path: path.join(report, "failure.png"), fullPage: true })
        .catch(() => {});
  })
  .finally(async () => {
    fs.writeFileSync(
      path.join(report, "report.json"),
      JSON.stringify(
        {
          platform: process.platform,
          binary,
          binary_sha256: crypto
            .createHash("sha256")
            .update(fs.readFileSync(binary))
            .digest("hex"),
          passed,
          errors,
        },
        null,
        2,
      ),
    );
    if (browser) await browser.close();
    if (server) server.kill();
  });
