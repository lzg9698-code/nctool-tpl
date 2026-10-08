// Isolated visual and interaction acceptance for the template workbench.
const { chromium } = require(process.env.NCTOOL_PLAYWRIGHT || "playwright");
const { spawn, execFileSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const assert = require("node:assert/strict");
const root = path.resolve(__dirname, "..");
const baseline = process.argv.includes("--baseline");
const out = process.env.NCTOOL_REPORT || path.join(
  root,
  "target/ui-optimization",
  baseline ? "before" : "after",
);
const temp = fs.mkdtempSync(path.join(os.tmpdir(), "nctool-ui-"));
const binary =
  process.env.NCTOOL_BINARY ||
  path.join(
    root,
    "target/debug/nctool" + (process.platform === "win32" ? ".exe" : ""),
  );
let browser, child;
const errors = [];
async function start() {
  return new Promise((resolve, reject) => {
    child = spawn(
      binary,
      [
        "--home",
        path.join(temp, "home"),
        "--workspace",
        path.join(temp, "ws"),
        "--profile",
        "template",
        "ui",
        "--port",
        "0",
      ],
      { cwd: root },
    );
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
    child.on("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`server exited ${code}: ${log}`));
    });
  });
}
async function main() {
  fs.mkdirSync(out, { recursive: true });
  const url = await start();
  for (let i = 0; i < 3; i++) {
    const response = await fetch(url + "/api/v2/actions/template.save", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        input: {
          name: "report-" + i,
          asset: {
            tags: [i ? "其他" : "报告"],
            source: "报告 {{ name }}：{{ count }}",
            schema: {
              type: "object",
              properties: {
                name: {
                  type: "string",
                  title: "姓名",
                  description: "报告接收人",
                },
                count: { type: "integer", title: "数量", minimum: 0 },
              },
              required: ["name", "count"],
            },
            defaults: { name: "Ada", count: 12 },
            metadata: {
              title: i
                ? "报告 " + i
                : "生产报告模板：用于验证长名称的展示及完整名称提示",
              description: "生成可重复使用的文本报告",
            },
          },
        },
      }),
    });
    assert.equal((await response.json()).ok, true);
  }
  browser = await chromium.launch({
    headless: true,
    ...(process.env.NCTOOL_CHROMIUM
      ? { executablePath: process.env.NCTOOL_CHROMIUM }
      : {}),
  });
  const page = await browser.newPage({ acceptDownloads: true });
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(url);
  await page.locator(".template-item").first().click();
  await page.getByLabel("模板名称", { exact: true }).waitFor();
  for (const [width, height] of [
    [1280, 720],
    [1440, 900],
    [1920, 1080],
  ]) {
    await page.setViewportSize({ width, height });
    await page.screenshot({ path: path.join(out, `use-${width}.png`) });
    await page.getByRole("button", { name: "编辑模板", exact: true }).click();
    if (!baseline)
      assert.equal(
        await page
          .getByRole("button", { name: "生成文件", exact: true })
          .count(),
        1,
      );
    await page.screenshot({ path: path.join(out, `edit-${width}.png`) });
    await page.getByRole("button", { name: "使用模板", exact: true }).click();
    if (!baseline) {
      assert.equal(
        await page.evaluate(
          () => document.documentElement.scrollWidth > innerWidth,
        ),
        false,
      );
      assert.equal(
        await page
          .getByRole("button", { name: "生成文件", exact: true })
          .count(),
        1,
      );
      for (const name of ["保存", "生成文件", "导出文件"]) {
        const bounds = await page
          .getByRole("button", { name, exact: true })
          .boundingBox();
        assert(
          bounds &&
            bounds.y >= 0 &&
            bounds.y + bounds.height <= height &&
            bounds.x + bounds.width <= width,
          name,
        );
      }
    }
  }
  if (!baseline) {
    await contrast(page);
    await interactions(page, url);
  }
  assert.deepEqual(errors, []);
  fs.writeFileSync(
    path.join(out, "result.json"),
    JSON.stringify(
      {
        platform: process.platform,
        baseline,
        viewports: [1280, 1440, 1920],
        errors,
        status: "passed",
      },
      null,
      2,
    ),
  );
  console.log(
    "PASS: " +
      (baseline
        ? "isolated baseline screenshots"
        : "workbench layout and interactions"),
  );
}
async function interactions(page, url) {
  await page.getByLabel("按标签筛选模板", { exact: true }).selectOption("报告");
  assert.equal(await page.locator(".template-item").count(), 1);
  await page.getByRole("button", { name: "清除筛选", exact: true }).click();
  await page.getByLabel("搜索模板", { exact: true }).fill("没有这个模板");
  await page.getByText("没有匹配的模板", { exact: true }).waitFor();
  await page.getByRole("button", { name: "清除筛选", exact: true }).click();
  assert.equal(await page.locator(".template-item").count(), 3);
  await page.getByRole("button", { name: "折叠侧栏", exact: true }).click();
  await page.reload();
  await page.getByRole("button", { name: "展开侧栏", exact: true }).waitFor();
  await page.getByRole("button", { name: "展开侧栏", exact: true }).click();
  await page.getByLabel("模板名称", { exact: true }).waitFor();
  await page.getByRole("button", { name: "生成文件", exact: true }).click();
  await page.locator(".result-pane .run-status.succeeded").waitFor();
  await page.screenshot({ path: path.join(out, "generated.png") });
  assert.equal(
    await page
      .getByRole("button", { name: "导出文件", exact: true })
      .isEnabled(),
    true,
  );
  const downloaded = page.waitForEvent("download");
  await page.getByRole("button", { name: "导出文件", exact: true }).click();
  await (await downloaded).saveAs(path.join(out, "report.txt"));
  assert.match(fs.readFileSync(path.join(out, "report.txt"), "utf8"), /Ada/);
  await page.locator('[data-field-path="/name"] input').fill("Grace");
  assert.equal(
    await page
      .getByRole("button", { name: "导出文件", exact: true })
      .isDisabled(),
    true,
  );
  await page.getByText("结果已过期", { exact: true }).waitFor();
  await page.getByRole("button", { name: "JSON", exact: true }).click();
  await page
    .locator('.cm-content[aria-label="本次参数 JSON"]')
    .fill('{"name":');
  const started = [];
  page.on("request", (request) => {
    if (request.method() === "POST" && request.url().endsWith("/runs"))
      started.push(request.url());
  });
  await page.getByRole("button", { name: "生成文件", exact: true }).click();
  await page
    .getByRole("button", { name: "导出文件", exact: true })
    .isDisabled()
    .then((value) => assert.equal(value, true));
  assert.equal(started.length, 0);
  await page.getByRole("button", { name: "表单", exact: true }).click();
  await page.getByRole("button", { name: "生成文件", exact: true }).click();
  assert.equal(
    started.length,
    0,
    "invalid raw JSON also blocks generation after switching to form",
  );
  await page.getByRole("button", { name: "保存预设", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog.waitFor();
  assert.equal(
    await dialog.evaluate((el) => el.contains(document.activeElement)),
    true,
  );
  for (let i = 0; i < 8; i++) {
    await page.keyboard.press("Tab");
    assert.equal(
      await dialog.evaluate((el) => el.contains(document.activeElement)),
      true,
    );
  }
  await page.keyboard.press("Escape");
  await dialog.waitFor({ state: "hidden" });
  assert.equal(
    await page
      .getByRole("button", { name: "保存预设", exact: true })
      .evaluate((el) => el === document.activeElement),
    true,
  );
  await page.locator('[data-field-path="/name"] input').fill("Lin");
  await page.getByRole("button", { name: "保存预设", exact: true }).click();
  await page.getByLabel("预设名称", { exact: true }).fill("验收参数");
  await dialog.getByRole("button", { name: "保存预设", exact: true }).click();
  await dialog.waitFor({ state: "hidden" });
  await page.locator('[data-field-path="/name"] input').fill("Hopper");
  await page.getByRole("button", { name: "加载预设", exact: true }).click();
  await dialog.getByRole("button", { name: "验收参数", exact: true }).click();
  assert.equal(
    await page.locator('[data-field-path="/name"] input').inputValue(),
    "Lin",
  );
  await page.locator('[data-field-path="/count"] input').fill("-");
  await page.getByRole("button", { name: "生成文件", exact: true }).click();
  assert.equal(started.length, 0, "incomplete numbers never dispatch");
  assert.equal(
    await page
      .getByRole("button", { name: "导出文件", exact: true })
      .isDisabled(),
    true,
  );
  await page.locator('[data-field-path="/count"] input').fill("12");
  const menu = page.getByLabel("工作区资产", { exact: true });
  await menu.click();
  await page.keyboard.press("Tab");
  assert.equal(
    await page
      .getByRole("button", { name: "导出工作区资产", exact: true })
      .evaluate((el) => el === document.activeElement),
    true,
  );
  await page.keyboard.press("Escape");
  assert.equal(
    await menu.evaluate((el) => el === document.activeElement),
    true,
  );
  // Restore each recorded task state, including terminal records with no error payload.
  const records = (await (await fetch(url + "/api/v2/runs")).json()).data.runs;
  const run = (
    await (await fetch(url + "/api/v2/runs/" + records[0].id)).json()
  ).data;
  const endpoint = "**/api/v2/runs/" + run.id;
  for (const [status, label] of [
    ["queued", "等待执行"],
    ["running", "执行中"],
    ["cancelled", "已取消"],
    ["interrupted", "已中断"],
    ["failed", "生成失败"],
  ]) {
    await page.route(endpoint, (route) =>
      route.fulfill({
        contentType: "application/json",
        body: JSON.stringify({
          ok: true,
          data: { ...run, status, result: undefined, error: undefined },
        }),
      }),
    );
    await page.reload();
    await page.locator(".result-pane .run-status." + status).waitFor();
    assert.equal(
      await page.locator(".result-pane .run-status").innerText(),
      label,
    );
    assert.equal(
      await page
        .getByRole("button", { name: "导出文件", exact: true })
        .isDisabled(),
      true,
    );
    if (["queued", "running"].includes(status)) {
      await page.getByRole("button", { name: "检查信息", exact: true }).click();
      assert.equal(
        await page
          .getByRole("button", { name: "取消任务", exact: true })
          .isVisible(),
        true,
      );
    }
    await page.screenshot({ path: path.join(out, status + ".png") });
    await page.unroute(endpoint);
  }
  await page.route("**/api/v2/actions/template.catalog", (route) =>
    route.fulfill({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({
        ok: false,
        error: { code: "test", message: "模板列表读取失败" },
      }),
    }),
  );
  await page.reload();
  await page.locator(".catalog-error").waitFor();
  await page.unroute("**/api/v2/actions/template.catalog");
  await page.getByRole("button", { name: "重新读取", exact: true }).click();
  await page.locator(".template-item").first().waitFor();
  assert.equal(await page.locator(".catalog-error").count(), 0);
  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
    false,
  );
}
async function contrast(page) {
  const issues = await page.evaluate(() => {
    const rgb = (value) => value.match(/[\d.]+/g).map(Number);
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
      ".nav-item, .template-item strong, .template-item small, .sidebar-section-title, .filter-summary span, .document-state, .field-heading label, .field-description, .text-button, .pane-tabs button, .empty-state strong, .empty-state p, .export-hint, .action-hint, .filename-field span, .run-status, .workspace-indicator";
    for (const el of document.querySelectorAll(selectors)) {
      if (!el.getClientRects().length || el.closest("button:disabled"))
        continue;
      const style = getComputedStyle(el);
      let background = [255, 255, 255, 1];
      const ancestors = [];
      for (let node = el; node; node = node.parentElement)
        ancestors.unshift(node);
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
        (Math.max(foreground, back) + 0.05) /
        (Math.min(foreground, back) + 0.05);
      if (ratio < 4.5 || parseFloat(style.fontSize) < 12)
        problems.push({
          text: el.textContent.trim(),
          ratio,
          fontSize: style.fontSize,
        });
    }
    return problems;
  });
  fs.writeFileSync(
    path.join(out, "contrast.json"),
    JSON.stringify(issues, null, 2),
  );
  assert.deepEqual(issues, [], "ordinary workbench text contrast/font size");
}
main()
  .catch((error) => {
    console.error(error);
    process.exitCode = 1;
  })
  .finally(async () => {
    if (browser) await browser.close();
    if (child && child.exitCode === null) {
      const stopped = new Promise((resolve) => child.once("exit", resolve));
      if (process.platform === "win32")
        execFileSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
          stdio: "ignore",
        });
      else child.kill();
      await stopped;
    }
    // temp is created by this invocation with a fixed prefix under os.tmpdir().
    if (
      path.dirname(temp) !== path.resolve(os.tmpdir()) ||
      !path.basename(temp).startsWith("nctool-ui-")
    )
      throw new Error("unexpected cleanup path");
    fs.rmSync(temp, {
      recursive: true,
      force: true,
      maxRetries: 10,
      retryDelay: 100,
    });
  });
