// Each suite owns its temporary host, data and browser. No user workspace is used.
const { chromium } = require(process.env.NCTOOL_PLAYWRIGHT || "playwright");
const { spawn, execFileSync } = require("node:child_process");
const fs = require("node:fs"),
  path = require("node:path"),
  os = require("node:os"),
  assert = require("node:assert/strict");
const root = path.resolve(__dirname, "..");
const binary =
  process.env.NCTOOL_BINARY ||
  path.join(
    root,
    "target/debug/nctool" + (process.platform === "win32" ? ".exe" : ""),
  );
async function suite(name, execute) {
  const report =
    process.env.NCTOOL_REPORT ||
    path.join(root, "target/ui-rewrite-20261023", name);
  fs.mkdirSync(report, { recursive: true });
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), "nctool-rewrite-")),
    home = path.join(temp, "home"),
    workspace = path.join(temp, "workspace");
  const children = [],
    errors = [],
    checks = [];
  let browser, page, url;
  const stop = (child) => {
    if (child && child.exitCode === null) {
      try {
        if (process.platform === "win32")
          execFileSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
            stdio: "ignore",
            windowsHide: true,
          });
        else child.kill();
      } catch (error) {
        if (child.exitCode === null)
          console.warn(
            "Cleanup could not confirm process exit:",
            child.pid,
            error.message,
          );
      }
    }
  };
  async function serve() {
    url = undefined;
    const child = spawn(
      binary,
      ["--home", home, "--workspace", workspace, "ui", "--port", "0"],
      { cwd: root, windowsHide: true },
    );
    children.push(child);
    url = await new Promise((resolve, reject) => {
      let log = "";
      const timer = setTimeout(
        () => reject(new Error(log || "startup timeout")),
        20000,
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
      child.on("exit", (code) => {
        clearTimeout(timer);
        if (!url) reject(new Error(log || String(code)));
      });
    });
    return child;
  }
  async function request(route, body, options = {}) {
    const response = await fetch(url + "/api/v2/" + route, {
      method: body === undefined ? "GET" : "POST",
      headers: {
        Origin: url,
        ...(body === undefined ? {} : { "Content-Type": "application/json" }),
        ...options.headers,
      },
      body:
        body === undefined
          ? undefined
          : typeof body === "string"
            ? body
            : JSON.stringify(body),
    });
    const text = await response.text();
    return { ...JSON.parse(text), status: response.status, text };
  }
  async function api(route, body) {
    const result = await request(route, body);
    assert.equal(result.ok, true, result.text);
    return result.data;
  }
  const action = (id, input = {}) =>
    api("actions/" + encodeURIComponent(id), { input });
  async function finished(id) {
    for (let i = 0; i < 300; i++) {
      const run = await api("runs/" + id);
      if (!["queued", "running"].includes(run.status)) return run;
      await new Promise((resolve) => setTimeout(resolve, 30));
    }
    throw new Error("run timeout " + id);
  }
  async function edit(label, text) {
    const target = page
      .locator('.cm-content[aria-label="' + label + '"]')
      .first();
    await target.click();
    await target.press(process.platform === "darwin" ? "Meta+a" : "Control+a");
    await page.keyboard.insertText(text);
    await page.waitForFunction(
      ([label, text]) =>
        [...document.querySelectorAll(".cm-content")].some(
          (el) =>
            el.getAttribute("aria-label") === label && el.innerText === text,
        ),
      [label, text],
    );
  }
  async function open(title) {
    await page
      .locator(".catalog-item")
      .filter({ hasText: title })
      .first()
      .click();
    await page
      .getByRole("textbox", { name: "模板名称", exact: true })
      .waitFor();
  }
  async function generate() {
    const button = page.getByRole("button", { name: "生成文件", exact: true });
    assert.equal(
      await button.isEnabled(),
      true,
      await page.locator("body").innerText(),
    );
    const [response] = await Promise.all([
      page.waitForResponse(
        (res) =>
          res.url().endsWith("/api/v2/runs") &&
          res.request().method() === "POST",
      ),
      button.click(),
    ]);
    return finished((await response.json()).data.id);
  }
  async function select(label, text) {
    await page.getByRole("combobox", { name: label, exact: true }).click();
    await page
      .locator(".ant-select-dropdown:visible .ant-select-item-option")
      .filter({ hasText: text })
      .first()
      .click();
  }
  async function boot() {
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
    page.on("dialog", (dialog) =>
      dialog.type() === "beforeunload" ? dialog.accept() : dialog.dismiss(),
    );
    await page.goto(url);
    await page.getByText("本地服务已连接", { exact: true }).waitFor();
  }
  const t = {
    root,
    binary,
    home,
    workspace,
    temp,
    report,
    fs,
    path,
    assert,
    request,
    api,
    action,
    finished,
    edit,
    open,
    generate,
    select,
    serve,
    stop,
    boot,
    children,
    step: (name) => {
      checks.push(name);
      console.log("PASS", name);
    },
    get page() {
      return page;
    },
    get browser() {
      return browser;
    },
    get url() {
      return url;
    },
  };
  try {
    await serve();
    await execute(t);
    assert.deepEqual(errors, [], "browser errors");
    if (page)
      await page.screenshot({
        path: path.join(report, "final.png"),
        fullPage: true,
      });
    fs.writeFileSync(
      path.join(report, "report.json"),
      JSON.stringify(
        {
          status: "passed",
          checks,
          errors,
          binary,
          platform: process.platform,
        },
        null,
        2,
      ),
    );
    console.log(`PASS ${name}: ${checks.length} checks`);
  } catch (error) {
    if (page) {
      await page
        .screenshot({ path: path.join(report, "failure.png"), fullPage: true })
        .catch(() => {});
      fs.writeFileSync(
        path.join(report, "failure.html"),
        await page.content().catch(() => ""),
      );
    }
    fs.writeFileSync(
      path.join(report, "report.json"),
      JSON.stringify(
        {
          status: "failed",
          checks,
          errors,
          error: String(error.stack || error),
          binary,
        },
        null,
        2,
      ),
    );
    throw error;
  } finally {
    if (browser) await browser.close();
    children.forEach(stop);
  }
}
module.exports = { suite };
