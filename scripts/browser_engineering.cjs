// End-to-end native workbench and independent plugin acceptance.
const { suite } = require("./browser_rewrite_support.cjs");
const { execFileSync } = require("node:child_process");
suite("engineering", async (t) => {
  const { fs, path, assert, api, action, request } = t;
  const source =
    process.env.NCTOOL_FTU_PACKAGE || path.join(t.root, "target/ftu-package");
  assert.ok(
    fs.existsSync(path.join(source, "plugin.json")),
    "build independent FTU package first",
  );
  const demo = path.join(t.temp, "demo");
  fs.cpSync(path.join(t.root, "examples/plugins/engineering-demo"), demo, {
    recursive: true,
  });
  const manifest = JSON.parse(
    fs.readFileSync(path.join(demo, "plugin.json"), "utf8"),
  );
  manifest.command[0] = process.env.NCTOOL_PYTHON || "python3";
  fs.writeFileSync(path.join(demo, "plugin.json"), JSON.stringify(manifest));
  for (const dir of [source, demo]) {
    const installed = await api("plugins/install", { source: dir });
    await api(`plugins/${installed.installed}/enable`, {});
  }
  t.stop(t.children[0]);
  await t.serve();
  await t.boot();
  const caps = await api("capabilities");
  assert.equal(caps.workbenches.length, 2);
  const view = caps.workbenches.find((v) => v.plugin === "ftu"),
    project = view.example;
  const analysis = await action("ftu.analyze", { project });
  assert.equal(analysis.data.result.valid, true);
  assert.ok(analysis.data.scene.items.length > 5);
  const prepared = await action("ftu.prepare", {
    project,
    formats: ["spf", "json", "csv", "svg"],
  });
  const generated = await action("ftu.generate", prepared.data.input);
  assert.equal(generated.artifacts.length, 4);
  const spf = generated.artifacts.find((a) => a.name.endsWith(".SPF")).text;
  assert.ok(spf.endsWith("M17\r\n"));
  assert.ok(spf.includes("G95\r\nG96"));
  assert.ok(!/[^\x00-\x7F]/.test(spf));
  t.step(
    "independent plugin, service callbacks, four artifacts and numeric formatting",
  );
  const post = (await action("ftu.template")).data.asset;
  const fp = (
    await action("template.save", {
      name: "user-post",
      asset: { source: post.source + "\n; USER COPY" },
    })
  ).data.fingerprint;
  const frozen = (
    await action("ftu.prepare", {
      project,
      formats: ["spf"],
      template: "user-post",
    })
  ).data.input;
  await action("template.save", {
    name: "user-post",
    expected: fp,
    asset: { source: post.source.replace("G95\n", "") },
  });
  assert.equal((await action("ftu.generate", frozen)).artifacts.length, 1);
  const rejected = await request("actions/ftu.generate", {
    input: { project, formats: ["spf"], template: "user-post" },
  });
  assert.equal(rejected.ok, false);
  assert.equal(rejected.error.code, "nc_output_mismatch");
  t.step(
    "frozen template survives edits; changed modal semantics are rejected",
  );
  const doc = {
    format_version: 1,
    raw: JSON.stringify(project),
    invalid: { "/startZ": "-" },
    formats: ["spf"],
    template: "",
  };
  const endpoint = "engineering/ftu/finishing/";
  const first = await api(endpoint + "save", {
    id: "invalid-draft",
    document: doc,
    expected: null,
  });
  assert.equal(
    (
      await request(endpoint + "save", {
        id: "invalid-draft",
        document: doc,
        expected: null,
      })
    ).error.code,
    "conflict",
  );
  assert.equal(
    (await api(endpoint + "read", { id: "invalid-draft" })).document.invalid[
      "/startZ"
    ],
    "-",
  );
  assert.equal(
    (await request(endpoint + "read", { id: "../../escape" })).ok,
    false,
  );
  assert.equal(
    (await request("engineering/missing/finishing/list", {})).ok,
    false,
  );
  const bundle = await api("bundle");
  assert.ok(bundle.collections["documents.ftu.project"]);
  await api(endpoint + "remove", {
    id: "invalid-draft",
    expected: first.fingerprint,
  });
  t.step(
    "draft persistence, create-only conflict, path boundaries and bundle contribution",
  );
  // Differential evidence uses copies transpiled into the disposable fixture only.
  const oldRoot = path.join(t.root, "..", "FTU", "src");
  if (fs.existsSync(path.join(oldRoot, "core.ts"))) {
    const ts = require(path.join(t.root, "ui/node_modules/typescript")),
      comparison = path.join(t.temp, "reference");
    fs.mkdirSync(comparison);
    fs.writeFileSync(
      path.join(comparison, "package.json"),
      '{"type":"commonjs"}',
    );
    for (const name of ["core", "machining", "examples", "sinumerik"]) {
      const text = fs.readFileSync(path.join(oldRoot, name + ".ts"), "utf8");
      fs.writeFileSync(
        path.join(comparison, name + ".js"),
        ts.transpileModule(text, {
          compilerOptions: {
            target: ts.ScriptTarget.ES2022,
            module: ts.ModuleKind.CommonJS,
          },
        }).outputText,
      );
    }
    const old = require(path.join(comparison, "core.js")),
      examples = require(path.join(comparison, "examples.js"));
    let cases = 0;
    for (const mode of ["outside", "inside"])
      for (const direction of [-1, 1])
        for (const reverse of [false, true])
          for (const lead of ["line", "arc"]) {
            const p = examples.example(mode);
            Object.assign(p, { direction, reverse, stock: null });
            p.entry.type = lead;
            p.exit.type = lead;
            const expected = old.generateTrajectory(p),
              actual = (await action("ftu.analyze", { project: p })).data
                .result;
            assert.equal(
              actual.valid,
              expected.valid,
              JSON.stringify({
                mode,
                direction,
                reverse,
                lead,
                diagnostics: actual.diagnostics,
              }),
            );
            assert.equal(actual.segments.length, expected.segments.length);
            for (let i = 0; i < actual.segments.length; i++) {
              const a = actual.segments[i],
                b = expected.segments[i];
              for (const end of ["start", "end"])
                for (const axis of ["z", "r"])
                  assert.ok(Math.abs(a[end][axis] - b[end][axis]) < 1e-7);
              assert.equal(a.feedPerRev, b.feedPerRev);
              assert.equal(a.featureId, b.featureId);
            }
            cases++;
          }
    t.step(`differential geometry against original FTU: ${cases} combinations`);
  }
  const page = t.page;
  await page.getByRole("menuitem", { name: "精车轨迹", exact: true }).click();
  const generate = page.getByRole("button", { name: "正式生成", exact: true });
  await generate.waitFor();
  await page.waitForFunction(() =>
    [...document.querySelectorAll("button")].some(
      (b) => b.textContent.includes("正式生成") && !b.disabled,
    ),
  );
  await page.locator(".engineering-scene svg").scrollIntoViewIfNeeded();
  const hit = await page.locator(".engineering-scene svg").evaluate((svg) => {
    const p = svg.querySelector("path"),
      point = p.getPointAtLength(p.getTotalLength() / 2),
      screen = point.matrixTransform(svg.getScreenCTM());
    return { x: screen.x, y: screen.y };
  });
  await page.mouse.click(hit.x, hit.y);
  assert.ok((await page.locator(".engineering-selected").count()) > 0);
  await page.getByRole("button", { name: "播放", exact: true }).click();
  await page.waitForTimeout(160);
  await page.getByRole("button", { name: "暂停", exact: true }).click();
  await page
    .getByRole("textbox", { name: "项目标识", exact: true })
    .fill("browser-project");
  await page.getByRole("button", { name: "保存项目", exact: true }).click();
  await page.getByText("项目已保存", { exact: true }).waitFor();
  const field = page.locator('[id="field-/features/0/length"]');
  let releaseSave, saveArrived;
  const gate = new Promise((resolve) => {
      releaseSave = resolve;
    }),
    arrived = new Promise((resolve) => {
      saveArrived = resolve;
    });
  const saveRoute = "**/api/v2/engineering/ftu/finishing/save";
  await page.route(saveRoute, async (route) => {
    const response = await route.fetch();
    saveArrived();
    await gate;
    await route.fulfill({ response });
  });
  await page.getByRole("button", { name: "保存项目", exact: true }).click();
  await arrived;
  await field.fill("19");
  releaseSave();
  await page.waitForFunction(
    () => !document.querySelector(".engineering-page .ant-btn-loading"),
  );
  assert.equal(await field.inputValue(), "19");
  await page.unroute(saveRoute);
  await field.fill("18");
  t.step("editing during a pending save preserves the newer draft");
  await field.fill("-");
  assert.equal(await generate.isDisabled(), true);
  await page.reload();
  assert.equal(
    await page
      .getByRole("textbox", { name: "项目标识", exact: true })
      .inputValue(),
    "browser-project",
  );
  await page.locator('[id="field-/features/0/length"]').waitFor();
  assert.equal(
    await page.locator('[id="field-/features/0/length"]').inputValue(),
    "-",
  );
  await page.locator('[id="field-/features/0/length"]').fill("18");
  await page.waitForFunction(() =>
    [...document.querySelectorAll("button")].some(
      (b) => b.textContent.includes("正式生成") && !b.disabled,
    ),
  );
  t.step(
    "native table, graphic selection, playback and invalid numeric draft after reload",
  );
  await page.getByRole("tab", { name: "项目 JSON", exact: true }).click();
  const malformed = structuredClone(project);
  malformed.features = [null];
  await t.edit("工程项目 JSON", JSON.stringify(malformed));
  await page
    .getByText("记录表必须是对象数组，请在项目 JSON 中修正", { exact: true })
    .waitFor();
  assert.equal(await generate.isDisabled(), true);
  await t.edit("工程项目 JSON", JSON.stringify(project, null, 2));
  await page.getByRole("tab", { name: "表格与参数", exact: true }).click();
  await page.waitForFunction(() =>
    [...document.querySelectorAll("button")].some(
      (b) => b.textContent.includes("正式生成") && !b.disabled,
    ),
  );
  t.step(
    "malformed table JSON stays editable without crashing or executing old input",
  );
  await page.getByRole("button", { name: "编辑模板副本", exact: true }).click();
  await page.getByRole("dialog").waitFor();
  await page
    .getByRole("textbox", { name: "模板副本标识", exact: true })
    .fill("ui-post");
  await page.getByRole("dialog").getByRole("button", { name: "确 定" }).click();
  await page
    .getByText("模板已保存；正式生成仍会检查输出语义", { exact: true })
    .waitFor();
  await page.waitForFunction(() =>
    [...document.querySelectorAll("button")].some(
      (b) => b.textContent.includes("正式生成") && !b.disabled,
    ),
  );
  const [response] = await Promise.all([
    page.waitForResponse(
      (r) =>
        r.url().endsWith("/api/v2/runs") && r.request().method() === "POST",
    ),
    generate.click(),
  ]);
  const runId = (await response.json()).data.id;
  const run = await t.finished(runId);
  assert.equal(run.status, "succeeded", JSON.stringify(run.error));
  assert.equal(run.result.artifacts.length, 4);
  const allFiles = page.getByRole("button", {
    name: "导出全部文件",
    exact: true,
  });
  await page.waitForFunction(() =>
    [...document.querySelectorAll("button")].some(
      (b) => b.textContent.includes("导出全部文件") && !b.disabled,
    ),
  );
  const [download] = await Promise.all([
    page.waitForEvent("download"),
    allFiles.click(),
  ]);
  const zip = path.join(t.temp, "artifacts.zip");
  await download.saveAs(zip);
  execFileSync(
    process.env.NCTOOL_PYTHON || "python3",
    [
      "-c",
      "import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); assert z.testzip() is None; assert set(z.namelist())=={'FTU_PATH.SPF','trajectory.json','trajectory.csv','trajectory.svg'}; assert z.read('FTU_PATH.SPF').endswith(b'M17\\r\\n')",
      zip,
    ],
    { windowsHide: true },
  );
  t.step(
    "single ZIP download independently decoded with all four exact text artifacts",
  );
  await field.fill("19");
  await page.getByText("输入已修改，请重新生成", { exact: true }).waitFor();
  t.step(
    "template copy editor, frozen official run and stale output after edit",
  );
  await page
    .getByRole("menuitem", { name: "折线绘图示例", exact: true })
    .click();
  await page.locator(".engineering-scene svg path").first().waitFor();
  assert.equal(await page.locator(".ant-table-row").count(), 3);
  assert.equal(
    await page.locator('[id="field-/points/1/x"]').inputValue(),
    "10",
  );
  await page.getByRole("button", { name: "正式生成", exact: true }).click();
  await page.getByText("已完成", { exact: true }).first().waitFor();
  t.step(
    "second plugin reuses workbench without manufacturing-specific host branches",
  );
  await page.setViewportSize({ width: 390, height: 844 });
  assert.ok(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth + 1,
    ),
    "mobile document must not overflow horizontally",
  );
  await page.screenshot({
    path: path.join(t.report, "mobile.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  const large = structuredClone(project);
  large.features = Array.from({ length: 500 }, (_, i) => ({
    id: "f" + i,
    type: "cylinder",
    length: 1,
    diameter: 30,
    feedPerRev: 0.1,
    corner: { type: "none", value: 1, feedPerRev: 0.1 },
  }));
  large.entry.type = "line";
  large.exit.type = "line";
  large.forbidden = Array.from({ length: 100 }, (_, i) => ({
    zMin: 1000 + i * 2,
    zMax: 1001 + i * 2,
  }));
  const started = performance.now(),
    big = await action("ftu.analyze", { project: large });
  assert.equal(big.data.result.valid, true);
  assert.equal(big.data.result.profile.length, 500);
  const analysisMs = performance.now() - started;
  const bigOutput = await action("ftu.generate", {
    project: large,
    formats: ["json", "csv", "svg", "spf"],
  });
  assert.equal(bigOutput.artifacts.length, 4);
  await page.getByRole("menuitem", { name: "精车轨迹", exact: true }).click();
  const renderStart = performance.now();
  const largeFile = path.join(t.temp, "max-project.json");
  fs.writeFileSync(
    largeFile,
    JSON.stringify({
      format_version: 1,
      raw: JSON.stringify(large, null, 2),
      invalid: {},
      formats: ["json", "csv", "svg", "spf"],
      template: "",
    }),
  );
  await page
    .locator('.engineering-page input[type="file"]')
    .setInputFiles(largeFile);
  await page
    .getByRole("dialog")
    .getByRole("button", { name: /^(确\s*定|OK)$/ })
    .click();
  await page.getByRole("tab", { name: "表格与参数", exact: true }).click();
  await page.waitForFunction(
    () =>
      document.querySelectorAll(".engineering-scene svg path").length === 1004,
  );
  assert.equal(await page.locator(".ant-table-row").count(), 40);
  fs.writeFileSync(
    path.join(t.report, "performance.json"),
    JSON.stringify(
      {
        features: 500,
        forbidden: 100,
        analysis_ms: analysisMs,
        file_import_and_render_ms: performance.now() - renderStart,
        artifact_bytes: bigOutput.artifacts.reduce(
          (sum, a) => sum + Buffer.byteLength(a.text),
          0,
        ),
        scene_items: big.data.scene.items.length,
      },
      null,
      2,
    ),
  );
  t.step("narrow viewport and maximum declared project scale");
  await page.evaluate(() => window.scrollTo(0, 0));
});
