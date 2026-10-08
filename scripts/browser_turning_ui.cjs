// Presentation isolation and keyboard/responsive acceptance with temporary assets.
const { chromium } = require(process.env.NCTOOL_PLAYWRIGHT || "playwright");
const { spawn, execFileSync } = require("node:child_process");
const fs = require("node:fs"),
  path = require("node:path"),
  os = require("node:os"),
  assert = require("node:assert/strict"),
  crypto = require("node:crypto");
const root = path.resolve(__dirname, ".."),
  binary =
    process.env.NCTOOL_BINARY ||
    path.join(root, "target/ui-optimization/build-nc/debug/nctool.exe");
const report =
  process.env.NCTOOL_REPORT ||
  path.join(root, "target/ui-optimization/presentation-browser");
const temp = fs.mkdtempSync(path.join(os.tmpdir(), "turning-ui-")),
  home = path.join(temp, "home"),
  workspace = path.join(temp, "workspace");
const passed = [],
  errors = [],
  children = [];
let browser, page, url;
fs.mkdirSync(report, { recursive: true });
async function serve(profile, selectedBinary = binary) {
  return new Promise((resolve, reject) => {
    const child = spawn(
      selectedBinary,
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
    child.stderr.on("data", (bytes) => {
      log += bytes;
      const match = log.match(/http:\/\/127\.0\.0\.1:\d+/);
      if (match) resolve(match[0]);
    });
    child.on("error", reject);
    child.on("exit", (code) => code && reject(new Error(log)));
  });
}
async function api(route, body) {
  const response = await fetch(url + "/api/v2/" + route, {
    method: body ? "POST" : "GET",
    headers: body ? { "Content-Type": "application/json" } : {},
    body: body && (typeof body === "string" ? body : JSON.stringify(body)),
  });
  const value = await response.json();
  if (!value.ok) throw new Error(value.error.message);
  return value.data;
}
async function side(value) {
  await page
    .getByRole("tab", {
      name: value === "right" ? "右侧" : "左侧",
      exact: true,
    })
    .click();
}
const active = () => page.locator(".turning-side:visible");
async function number(label, value) {
  await page.getByLabel(label, { exact: true }).fill(String(value));
}
async function generate() {
  const response = page.waitForResponse(
    (r) => r.url().endsWith("/api/v2/runs") && r.request().method() === "POST",
  );
  await page.getByRole("button", { name: "生成全部", exact: true }).click();
  const value = await (await response).json();
  assert(value.ok);
  for (let i = 0; i < 200; i++) {
    const run = await api("runs/" + value.data.id);
    if (!["queued", "running"].includes(run.status)) {
      assert.equal(run.status, "succeeded", JSON.stringify(run.error));
      return run;
    }
    await new Promise((r) => setTimeout(r, 50));
  }
  throw new Error("run timeout");
}
async function main() {
  url = await serve("nc");
  const bundle = JSON.parse(
    fs.readFileSync(
      path.join(root, "examples/v3-managed/workspace.json"),
      "utf8",
    ),
  );
  await api(
    "bundle/import",
    '{"bundle":' +
      fs.readFileSync(
        path.join(root, "examples/v3-managed/workspace.json"),
        "utf8",
      ) +
      "}",
  );
  const specialized = bundle.collections.template.records.filter((r) =>
    ["sync.left", "sync.right", "groove"].includes(
      r.asset.metadata?.nc?.operation?.id,
    ),
  );
  const format = specialized.find(
    (r) => r.asset.metadata.nc.operation.id === "sync.left",
  ).asset;
  const custom = {
    ...format,
    metadata: { ...format.metadata, title: "隐蔽自定义格式" },
    tags: ["private-format"],
  };
  await api("actions/template.save", {
    input: { name: "unrelated-custom-name", asset: custom, expected: null },
  });
  const normal = {
    source: "普通模板",
    schema: { type: "object", properties: {}, required: [] },
    defaults: {},
    metadata: { title: "普通同步车示例" },
    tags: ["sync.left"],
  };
  await api("actions/template.save", {
    input: { name: "sync-turning-template", asset: normal, expected: null },
  });
  const total =
    (await api("actions/template.catalog", { input: {} })).data.items.length -
    specialized.length -
    1;
  browser = await chromium.launch({
    headless: true,
    ...(process.env.NCTOOL_CHROMIUM
      ? { executablePath: process.env.NCTOOL_CHROMIUM }
      : {}),
    args: ["--no-sandbox"],
  });
  page = await browser.newPage({ viewport: { width: 1440, height: 950 } });
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(url);
  await page
    .getByRole("button", { name: `模板库 · ${total} 个模板`, exact: true })
    .waitFor();
  assert.equal(await page.locator(".template-item").count(), total);
  for (const entry of specialized)
    assert.equal(
      await page
        .locator(".template-item")
        .filter({ hasText: entry.asset.metadata.title })
        .count(),
      0,
    );
  assert.equal(
    await page
      .getByLabel("按标签筛选模板")
      .locator('option[value="private-format"]')
      .count(),
    0,
  );
  await page.getByLabel("搜索模板").fill("隐蔽自定义格式");
  assert.equal(await page.locator(".template-item").count(), 0);
  await page.getByLabel("搜索模板").fill("普通同步车示例");
  assert.equal(await page.locator(".template-item").count(), 1);
  await page.getByLabel("搜索模板").fill("");
  passed.push(
    "metadata ownership, search, count, tags and name false positives",
  );

  const workspaceId = (await api("workspace")).id;
  const hidden = {
    key: "hidden-draft",
    id: "unrelated-custom-name",
    title: "旧专用编辑草稿",
    asset: { ...custom, source: "hidden-unsaved-source" },
    context: {},
    fingerprint: "old-baseline",
    saved: "",
    mode: "edit",
    tab: "source",
    outputName: "output.txt",
  };
  const visible = {
    ...hidden,
    key: "normal-draft",
    id: null,
    title: "普通旧草稿",
    asset: normal,
    tab: "settings",
  };
  await page.addInitScript(
    ({ workspaceId, hidden, visible }) => {
      const pack = (v) =>
        Array.isArray(v)
          ? ["a", v.map(pack)]
          : v !== null && typeof v === "object"
            ? [
                "o",
                Object.fromEntries(
                  Object.entries(v).map(([k, item]) => [k, pack(item)]),
                ),
              ]
            : ["v", v];
      localStorage.setItem(
        "nctool:workbench:" + workspaceId,
        JSON.stringify({
          version: 1,
          active: hidden.key,
          documents: pack([hidden, visible]),
        }),
      );
    },
    { workspaceId, hidden, visible },
  );
  await page.reload();
  await page.getByRole("button", { name: "普通旧草稿", exact: true }).waitFor();
  assert.equal(await page.locator(".document-tab").count(), 1);
  const references = await page
    .getByLabel("插入其他模板引用")
    .locator("option")
    .evaluateAll((options) => options.map((o) => o.value));
  assert(!references.includes("unrelated-custom-name"));
  for (const entry of specialized) assert(!references.includes(entry.name));
  assert(
    await page.evaluate(
      (id) =>
        localStorage
          .getItem("nctool:workbench:" + id)
          .includes("hidden-unsaved-source"),
      workspaceId,
    ),
  );
  const before = await page.locator(".document-tab").count();
  await page
    .locator('input[type="file"][accept=".json,.j2,.jinja2,.txt"]')
    .setInputFiles({
      name: "custom.json",
      mimeType: "application/json",
      buffer: Buffer.from(JSON.stringify(custom)),
    });
  await page
    .getByRole("alert")
    .filter({ hasText: "专用模板请通过工作区资产包导入" })
    .waitFor();
  assert.equal(await page.locator(".document-tab").count(), before);
  passed.push(
    "hidden drafts preserved, active fallback, include selector, single-file import",
  );

  await page.getByRole("button", { name: "G 代码生成", exact: true }).click();
  assert.equal(
    await page
      .getByLabel("加工模板")
      .locator('option[value="unrelated-custom-name"]')
      .count(),
    0,
  );
  await page.getByRole("button", { name: "多工序程序", exact: true }).click();
  await page.getByRole("button", { name: "添加工序", exact: true }).click();
  assert.equal(
    await page
      .getByLabel("工序模板1")
      .locator('option[value="unrelated-custom-name"]')
      .count(),
    0,
  );
  passed.push("generic NC and process selectors hide specialized formats");

  await page.getByRole("button", { name: "同步粗车", exact: true }).click();
  await active().locator(".turning-format > summary").click();
  await page
    .getByLabel("右侧模板格式")
    .locator("option")
    .filter({ hasText: "内置 V3" })
    .waitFor({ state: "attached" });
  await side("left");
  await active().locator(".turning-format > summary").click();
  await page.getByLabel("左侧模板格式").selectOption("unrelated-custom-name");
  assert.equal(
    await page.getByLabel("左侧模板格式").inputValue(),
    "unrelated-custom-name",
  );
  await page.getByLabel("左侧模板格式").selectOption("");
  for (const name of ["right", "left"]) {
    await side(name);
    const label = name === "right" ? "右侧" : "左侧";
    await number(label + "固定 ap1（直径减量 mm）", 2);
    for (const [key, value] of [
      ["起始 X", 60],
      ["起始 Z", 0],
      ["结束 X", 50],
      ["结束 Z", -10],
      ["ap2", 3],
    ])
      await number(`${label}第1行 ${key} (mm)`, value);
    await page
      .getByRole("button", { name: "计算" + label, exact: true })
      .click();
    await active()
      .getByRole("status")
      .filter({ hasText: "计算预览" })
      .waitFor();
    assert(
      await active()
        .getByRole("button", { name: "导出 SPF", exact: true })
        .first()
        .isDisabled(),
    );
  }
  await side("right");
  assert(
    (await active().locator(".turning-status").innerText()).includes(
      "计算预览",
    ),
  );
  passed.push(
    "independent side previews, preview export blocked, plugin format selection",
  );
  await generate();
  await active()
    .getByRole("button", { name: "导出 SPF", exact: true })
    .first()
    .waitFor();
  await page.waitForFunction(
    () =>
      document.querySelector(".turning-side:not([hidden]) .turning-status")
        .textContent === "生成成功",
  );
  await number("右侧第1行 结束 Z (mm)", -11);
  assert(
    await active()
      .getByRole("button", { name: "导出 SPF", exact: true })
      .first()
      .isDisabled(),
  );
  assert(
    await active()
      .getByRole("button", { name: "右侧 CH1复制", exact: true })
      .isDisabled(),
  );
  assert(
    await active()
      .getByRole("button", { name: "加入组合 · 右侧", exact: true })
      .isDisabled(),
  );
  await side("left");
  assert(
    await active()
      .getByRole("button", { name: "导出 SPF", exact: true })
      .first()
      .isEnabled(),
  );
  await number("同步进给（按程序模式）", 0.3);
  assert(
    await active()
      .getByRole("button", { name: "导出 SPF", exact: true })
      .first()
      .isDisabled(),
  );
  passed.push(
    "side-local invalidation and global invalidation gate all delivery controls",
  );

  await page.setViewportSize({ width: 853, height: 480 });
  await side("right");
  const right = page.getByRole("tab", { name: "右侧", exact: true });
  await right.focus();
  await page.keyboard.press("ArrowRight");
  assert.equal(
    await page
      .getByRole("tab", { name: "左侧", exact: true })
      .getAttribute("aria-selected"),
    "true",
  );
  await page.keyboard.press("Home");
  assert.equal(await right.getAttribute("aria-selected"), "true");
  const code = active().getByRole("tab", { name: "代码", exact: true });
  await code.focus();
  await page.keyboard.press("End");
  assert(
    (await active().getByRole("img", { name: "轨迹坐标示意" }).isVisible()) ||
      (await active().getByText("填入原始参数后计算预览。").isVisible()),
  );
  await page.keyboard.press("Home");
  const ch1 = active().getByRole("tab", { name: "CH1", exact: true });
  await ch1.focus();
  await page.keyboard.press("ArrowRight");
  assert.equal(
    await active()
      .getByRole("tab", { name: "CH2", exact: true })
      .getAttribute("aria-selected"),
    "true",
  );
  assert.equal(
    await active().getByLabel("右侧 CH1程序", { exact: true }).isVisible(),
    false,
  );
  assert(
    await active().getByLabel("右侧 CH2程序", { exact: true }).isVisible(),
  );
  for (let i = 0; i < 10; i++) {
    await page.keyboard.press("Tab");
    assert(
      await page.evaluate(
        () =>
          !document.activeElement.closest("[hidden]") &&
          !document.activeElement.closest(".turning-channel:not(.selected)"),
      ),
    );
  }
  passed.push(
    "keyboard side/view/channel navigation and hidden focus exclusion",
  );
  if (await page.getByLabel("关闭提示", { exact: true }).count())
    await page.getByLabel("关闭提示", { exact: true }).click();
  await side("right");
  await generate();
  await page.waitForFunction(
    () =>
      document.querySelector(".turning-side:not([hidden]) .turning-status")
        .textContent === "生成成功",
  );
  if (
    await active()
      .locator(".turning-format")
      .evaluate((el) => el.open)
  )
    await active().locator(".turning-format > summary").click();
  for (const width of [1600, 1440, 1280, 853, 390]) {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 950 });
    await active().locator(".turning-results").scrollIntoViewIfNeeded();
    assert(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    );
    assert.equal(
      await active().locator(".turning-channel:visible").count(),
      width >= 1280 ? 2 : 1,
    );
    assert.equal(
      await active()
        .locator(".turning-side-body")
        .evaluate(
          (el) => getComputedStyle(el).gridTemplateColumns.split(" ").length,
        ),
      width >= 1600 ? 2 : 1,
    );
    await page.screenshot({
      path: path.join(report, `sync-${width}.png`),
      fullPage: true,
    });
  }
  passed.push("all five responsive breakpoints and independent code scroll");
  execFileSync(binary, ["--home", home, "plugins", "disable", "sync-turning"], {
    cwd: root,
  });
  url = await serve("nc");
  await page.goto(url);
  await page
    .getByRole("button", { name: `模板库 · ${total} 个模板`, exact: true })
    .waitFor();
  assert.equal(
    await page.getByRole("button", { name: "同步粗车", exact: true }).count(),
    0,
  );
  assert.equal(
    await page
      .locator(".template-item")
      .filter({ hasText: "隐蔽自定义格式" })
      .count(),
    0,
  );
  passed.push("hidden ownership independent of plugin availability");
  url = await serve("template", process.env.NCTOOL_TEMPLATE_BINARY || binary);
  await page.goto(url);
  await page
    .getByRole("button", { name: `模板库 · ${total} 个模板`, exact: true })
    .waitFor();
  assert.equal(
    await page.getByRole("button", { name: "越程槽", exact: true }).count(),
    0,
  );
  assert.equal(
    await page
      .locator(".template-item")
      .filter({ hasText: "隐蔽自定义格式" })
      .count(),
    0,
  );
  passed.push(
    "default template build hides specialized assets without NC providers",
  );
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
          playwright: require(
            (process.env.NCTOOL_PLAYWRIGHT || "playwright") + "/package.json",
          ).version,
          chromium: browser?.version(),
          checked_at: new Date().toISOString(),
          passed,
          errors,
        },
        null,
        2,
      ),
    );
    if (browser) await browser.close();
    for (const child of children) if (child.exitCode === null) child.kill();
  });
