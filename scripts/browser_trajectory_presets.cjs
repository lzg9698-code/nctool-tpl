// Independent preset editing, isolated assets, existing versioned plugin APIs.
const { chromium } = require(process.env.NCTOOL_PLAYWRIGHT || "playwright");
const { spawn } = require("node:child_process");
const fs = require("node:fs"),
  path = require("node:path"),
  os = require("node:os"),
  assert = require("node:assert/strict"),
  crypto = require("node:crypto");
const root = path.resolve(__dirname, "..");
const binary =
  process.env.NCTOOL_BINARY ||
  path.join(root, "target/ap2-auto/build-nc/debug/nctool.exe");
const report =
  process.env.NCTOOL_REPORT ||
  path.join(root, "target/trajectory-preset-editor/browser");
const temp = fs.mkdtempSync(path.join(os.tmpdir(), "trajectory-editor-"));
const passed = [],
  errors = [];
let server, browser, page, url;
fs.mkdirSync(report, { recursive: true });
const active = () => page.locator(".turning-side:visible");
const editor = () => active().locator(".turning-preset-editor");
async function api(route, input) {
  const response = await fetch(url + "/api/v2/actions/" + route, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ input }),
  });
  const value = await response.json();
  assert(value.ok, JSON.stringify(value.error));
  return value.data.data;
}
async function select(side) {
  await page
    .getByRole("tab", { name: side === "right" ? "右侧" : "左侧", exact: true })
    .click();
}
async function open() {
  if (
    !(await active()
      .locator(".turning-trajectory")
      .evaluate((el) => el.open))
  )
    await active().locator(".turning-trajectory > summary").click();
}
async function fill(label, value) {
  await page.getByLabel(label, { exact: true }).fill(String(value));
}
async function save(kind, success = true) {
  const response = page.waitForResponse((r) =>
    r.url().endsWith("/actions/" + kind + ".asset.save"),
  );
  await editor().getByRole("button", { name: "保存预设", exact: true }).click();
  const value = await (await response).json();
  assert.equal(value.ok, success, JSON.stringify(value.error));
  if (success)
    await editor()
      .getByRole("status")
      .filter({ hasText: "预设已保存" })
      .waitFor();
  else await editor().getByRole("alert").waitFor();
}
async function asset(kind, id) {
  return (
    await api(kind + ".asset.catalog", { group: "trajectories" })
  ).items.find((p) => p.id === id);
}
async function generate(name) {
  const response = page.waitForResponse(
    (r) => r.url().endsWith("/api/v2/runs") && r.request().method() === "POST",
  );
  await page.getByRole("button", { name, exact: true }).click();
  const value = await (await response).json();
  assert(value.ok);
  for (let n = 0; n < 200; n++) {
    const run = (
      await (await fetch(url + "/api/v2/runs/" + value.data.id)).json()
    ).data;
    if (!["queued", "running"].includes(run.status)) {
      assert.equal(run.status, "succeeded", JSON.stringify(run.error));
      await active()
        .locator(".turning-status")
        .filter({ hasText: "生成成功" })
        .waitFor();
      return;
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
  let prepares = 0;
  page.on("request", (r) => {
    if (/actions\/(sync-turning|undercut)\.prepare$/.test(r.url())) prepares++;
  });
  for (const kind of ["sync-turning", "undercut"]) {
    prepares = 0;
    const sync = kind === "sync-turning",
      navigation = sync ? "同步粗车" : "越程槽";
    const parameter = sync ? "X 接近余量 (mm)" : "切削进给";
    const id = "ui-" + kind,
      copyId = id + "-copy",
      original = sync ? 12.125 : 0.25;
    await page.getByRole("button", { name: navigation, exact: true }).click();
    await select("right");
    await open();
    await editor()
      .getByRole("button", { name: "编辑所选预设", exact: true })
      .click();
    await fill("右侧预设 ID", "v3-compatible");
    await editor()
      .getByRole("button", { name: "保存预设", exact: true })
      .click();
    await editor().getByRole("alert").filter({ hasText: "不可覆盖" }).waitFor();
    await fill("右侧预设 ID", id);
    await fill("右侧预设名称", navigation + " UI 轨迹");
    await fill("右侧预设 " + parameter, sync ? 0 : -1);
    await save(kind, false);
    assert.equal(await asset(kind, id), undefined);
    await fill("右侧预设 " + parameter, "1e");
    await editor()
      .getByRole("button", { name: "保存预设", exact: true })
      .click();
    await editor().getByRole("alert").filter({ hasText: "完整" }).waitFor();
    assert.equal(
      await page.getByLabel("右侧预设 " + parameter).inputValue(),
      "1e",
    );
    await fill("右侧预设 " + parameter, original);
    await save(kind);
    assert.equal(
      (await asset(kind, id)).asset.parameters[
        sync ? "clearance_x" : "cut_feed"
      ],
      original,
    );
    assert.equal(
      prepares,
      0,
      "preset editing must work without machining calculation",
    );
    passed.push(
      kind +
        " builtin copy, incomplete/range validation and direct precise save with blank rows",
    );

    await page.getByLabel("右侧轨迹预设", { exact: true }).selectOption(id);
    await editor()
      .getByRole("button", { name: "重新读取所选预设", exact: true })
      .click();
    await fill("右侧预设名称", "待保存的本地名称");
    const existing = await asset(kind, id);
    existing.asset.title = "外部更新";
    existing.asset.extra = { retained: true };
    await api(kind + ".asset.save", {
      group: "trajectories",
      name: id,
      asset: existing.asset,
      expected: existing.fingerprint,
    });
    await save(kind, false);
    assert.equal(
      await page.getByLabel("右侧预设名称").inputValue(),
      "待保存的本地名称",
    );
    await page.reload();
    await page.getByRole("button", { name: navigation, exact: true }).click();
    await open();
    assert.equal(
      await page.getByLabel("右侧预设名称").inputValue(),
      "待保存的本地名称",
    );
    await save(kind, false); // Persisted draft keeps its original fingerprint.
    await editor()
      .getByRole("button", { name: "重新读取所选预设", exact: true })
      .click();
    await page.getByLabel("右侧预设名称").filter({ visible: true }).waitFor();
    await page.waitForFunction(
      () =>
        document.querySelector('[aria-label="右侧预设名称"]').value ===
        "外部更新",
    );
    await fill("右侧预设名称", "重新读取后更新");
    await save(kind);
    assert.deepEqual((await asset(kind, id)).asset.extra, { retained: true });
    passed.push(
      kind +
        " conflict preserves draft and baseline through reload; explicit reread preserves metadata",
    );

    await fill("右侧预设 ID", copyId);
    await fill("右侧预设名称", "另存 UI 轨迹");
    await save(kind);
    assert.equal((await asset(kind, id)).asset.title, "重新读取后更新");
    assert.equal((await asset(kind, copyId)).asset.title, "另存 UI 轨迹");
    await select("left");
    await open();
    await editor()
      .getByRole("button", { name: "编辑所选预设", exact: true })
      .click();
    await fill("左侧预设 ID", "left-draft-" + kind);
    await select("right");
    assert.equal(await page.getByLabel("右侧预设 ID").inputValue(), copyId);
    await page.reload();
    await page.getByRole("button", { name: navigation, exact: true }).click();
    await select("left");
    await open();
    assert.equal(
      await page.getByLabel("左侧预设 ID").inputValue(),
      "left-draft-" + kind,
    );
    await editor()
      .getByRole("button", { name: "放弃预设编辑", exact: true })
      .click();
    assert.equal(await editor().locator("input").count(), 0);
    passed.push(
      kind +
        " copy leaves original intact; independent side drafts survive navigation and refresh",
    );

    // Two sides reference one custom preset, then editing it invalidates both immediately.
    for (const side of ["right", "left"]) {
      await select(side);
      await open();
      const label = side === "right" ? "右侧" : "左侧";
      await page
        .getByLabel(label + "轨迹预设", { exact: true })
        .selectOption(id);
      if (sync) {
        await fill(label + "固定 ap1（直径减量 mm）", 2);
        for (const [key, value] of [
          ["起始 X", 60],
          ["起始 Z", 0],
          ["结束 X", 50],
          ["结束 Z", -10],
          ["ap2", 3],
        ])
          await fill(label + "第1行 " + key + " (mm)", value);
      } else {
        await fill("毛坯直径 (mm)", 100);
        await page
          .getByLabel(label + "第1行标准")
          .selectOption({ label: "DIN509-E 0,2 x 0,1 · r=0.2" });
        for (const [key, value] of [
          ["D1", 80],
          ["Z", 30],
          ["D2", 85],
          ["C", 0.5],
        ])
          await fill(label + "第1行 " + key + " (mm)", value);
        await fill(label + "第1行 角度 (°)", 45);
      }
    }
    await select("right");
    await generate("生成全部");
    await editor()
      .getByRole("button", { name: "重新读取所选预设", exact: true })
      .click();
    await page.waitForFunction(
      () =>
        document.querySelector('[aria-label="右侧预设名称"]').value ===
        "重新读取后更新",
    );
    await fill("右侧 " + parameter, sync ? 17.5 : 0.4);
    await fill("右侧预设 " + parameter, sync ? 13.125 : 0.3);
    await save(kind);
    assert.equal(
      Number(
        await page
          .getByLabel("右侧 " + parameter, { exact: true })
          .inputValue(),
      ),
      sync ? 17.5 : 0.4,
    );
    for (const side of ["right", "left"]) {
      await select(side);
      assert(
        await active()
          .getByRole("button", { name: "导出 SPF", exact: true })
          .first()
          .isDisabled(),
      );
      assert(
        await active()
          .getByRole("button", {
            name: "加入组合 · " + (side === "right" ? "右侧" : "左侧"),
            exact: true,
          })
          .isDisabled(),
      );
      assert.equal(
        await active()
          .getByRole("button", {
            name: "确认" + (side === "right" ? "右侧" : "左侧") + "轨迹调整",
            exact: true,
          })
          .isDisabled(),
        true,
      );
    }
    await select("right");
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 950 });
      await editor().scrollIntoViewIfNeeded();
      assert(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      );
      await editor().screenshot({
        path: path.join(report, kind + `-editor-${width}.png`),
      });
    }
    await page.setViewportSize({ width: 1280, height: 950 });
    await active()
      .getByRole("button", { name: "恢复预设值", exact: true })
      .click();
    await generate("生成右侧"); // Saved preset is consumed by existing generation API.
    await select("left");
    await page.getByLabel("左侧轨迹预设", { exact: true }).selectOption(copyId);
    await generate("生成左侧");
    await select("right");
    await fill("右侧预设 " + parameter, sync ? 14.125 : 0.35);
    await save(kind);
    await select("left");
    assert(
      await active()
        .getByRole("button", { name: "导出 SPF", exact: true })
        .first()
        .isEnabled(),
    );
    passed.push(
      kind +
        " saving referenced preset immediately invalidates both results and confirmations; unrelated side preserved",
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
