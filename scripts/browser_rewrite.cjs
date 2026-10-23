const { suite } = require("./browser_rewrite_support.cjs");
suite("browser-v2", async (t) => {
  const { assert, action, api } = t;
  await action("template.save", {
    name: "welcome",
    asset: {
      source: "你好，{{ name }}！{% for n in items %} {{ n }}{% endfor %}",
      schema: {
        type: "object",
        properties: {
          name: { type: "string", title: "姓名" },
          items: {
            type: "array",
            title: "数据",
            items: { type: "integer", title: "整数项" },
          },
        },
        required: ["name"],
      },
      defaults: { name: "Ada", items: [1] },
      metadata: { title: "验收模板" },
    },
    expected: null,
  });
  await t.boot();
  const page = t.page;
  await page.waitForLoadState("networkidle");
  const resourceMetrics = () => page.evaluate(() => performance.getEntriesByType("resource")
    .filter((entry)=>/\/assets\/.*\.(js|css)$/.test(entry.name))
    .map((entry)=>({name:entry.name.split("/").pop(),transfer:entry.transferSize,encoded:entry.encodedBodySize})));
  const coldResources = await resourceMetrics();
  await page.reload();
  await page.getByText("本地服务已连接",{exact:true}).waitFor();
  await page.waitForLoadState("networkidle");
  const warmResources = await resourceMetrics();
  assert.ok(coldResources.reduce((n,e)=>n+e.transfer,0)>0);
  assert.ok(warmResources.length>0 && warmResources.every((e)=>e.transfer===0),"hashed resources should be reused from browser cache");
  const indexHeaders = await fetch(t.url).then((r)=>r.headers);
  assert.equal(indexHeaders.get("cache-control"),"no-store");
  const apiHeaders = await fetch(t.url+"/api/v2/workspace").then((r)=>r.headers);
  assert.equal(apiHeaders.get("cache-control"),"no-store");
  t.fs.writeFileSync(t.path.join(t.report,"resource-transfer.json"),JSON.stringify({cold:coldResources,warm:warmResources},null,2));
  t.step("哈希资源复用浏览器缓存，HTML 与 API 保持 no-store");
  await t.open("验收模板");
  const widthSlider = page.getByRole("slider", { name: "输入区域宽度" });
  await widthSlider.focus();
  const priorWidth = Number(await widthSlider.getAttribute("aria-valuenow"));
  await widthSlider.press("ArrowRight");
  assert.equal(
    Number(await widthSlider.getAttribute("aria-valuenow")),
    priorWidth + 1,
  );
  assert.equal(
    await page.getByRole("textbox", { name: "姓名", exact: true }).inputValue(),
    "Ada",
  );
  let run = await t.generate();
  assert.equal(run.status, "succeeded");
  assert.equal(run.result.artifacts[0].text, "你好，Ada！ 1");
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "导出文件", exact: true }).click();
  assert.ok((await download).suggestedFilename());
  t.step("真实模板读取、递归数组表单、生成与下载");
  await page
    .getByRole("textbox", { name: "整数项", exact: true })
    .fill("9007199254740993");
  run = await t.generate();
  assert.equal(run.status, "succeeded");
  assert.ok(run.result.artifacts[0].text.includes("9007199254740993"));
  await page.getByText("JSON", { exact: true }).first().click();
  await t.edit("参数 JSON", '{"name":"Grace","items":[9007199254740993]}');
  await page.getByText("表单", { exact: true }).first().click();
  assert.equal(
    await page
      .getByRole("textbox", { name: "整数项", exact: true })
      .inputValue(),
    "9007199254740993",
  );
  t.step("大整数通过表单、JSON与后端往返不失真");
  await page
    .getByRole("textbox", { name: "姓名", exact: true })
    .fill("新版草稿");
  await page.reload();
  await page.getByRole("textbox", { name: "姓名", exact: true }).waitFor();
  assert.equal(
    await page.getByRole("textbox", { name: "姓名", exact: true }).inputValue(),
    "新版草稿",
  );
  const keys = await page.evaluate(() => Object.keys(localStorage));
  assert.ok(keys.some((key) => key.startsWith("nctool:rewrite:v1:")));
  assert.ok(!keys.some((key) => key.startsWith("nctool:workbench:")));
  t.step("新版草稿恢复且不创建旧存储");
  await page.getByText("编辑模板", { exact: true }).click();
  await t.edit("模板源码", '{% set n = 12 %}{{ "sample" }}{# comment #}');
  const editor = page.locator('.cm-content[aria-label="模板源码"]');
  const colors = await editor
    .locator("span")
    .evaluateAll((elements) => [
      ...new Set(elements.map((el) => getComputedStyle(el).color)),
    ]);
  assert.ok(colors.length >= 4, JSON.stringify(colors));
  await editor.press("Control+f");
  const search = page.locator(".cm-search");
  await search.waitFor();
  await search.locator('input[name="search"]').fill("sample");
  const style = await search.evaluate((el) => ({
    panel: getComputedStyle(el.closest(".cm-panels")).backgroundColor,
    input: getComputedStyle(el.querySelector("input")).backgroundColor,
  }));
  assert.notEqual(style.panel, "rgba(0, 0, 0, 0)");
  assert.notEqual(style.input, "rgba(0, 0, 0, 0)");
  await search.locator('input[name="search"]').press("Escape");
  t.step("编辑器高亮颜色、搜索和不透明背景");
  await t.edit("模板源码", "已更新 {{ name }}");
  await page.getByRole("button", { name: "保存模板", exact: true }).click();
  await page.getByText("模板已保存", { exact: true }).waitFor();
  const disk = (await action("template.read", { name: "welcome" })).data;
  await action("template.save", {
    name: "welcome",
    expected: disk.fingerprint,
    asset: { ...disk.asset, source: "磁盘版本" },
  });
  await t.edit("模板源码", "本地冲突草稿");
  await page.getByRole("button", { name: "保存模板", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog.waitFor();
  assert.ok((await dialog.innerText()).includes("模板已被其他编辑修改"));
  assert.ok(await dialog.locator(".diff-output").innerText());
  await dialog.getByRole("button", { name: "继续编辑", exact: true }).click();
  assert.equal(await editor.innerText(), "本地冲突草稿");
  t.step("指纹冲突展示差异并保留草稿");
  for (const [width, height] of [
    [1280, 720],
    [1440, 900],
    [1920, 1080],
    [900, 700],
  ]) {
    await page.setViewportSize({ width, height });
    assert.equal(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
      false,
      `${width}x${height} overflow`,
    );
    await page.screenshot({
      path: t.path.join(t.report, `layout-${width}.png`),
      fullPage: true,
    });
  }
  await page.setViewportSize({ width: 1440, height: 900 });
  t.step("四种桌面和窄屏布局无水平溢出");
  await page.getByRole("menuitem", { name: "参数管理", exact: true }).click();
  await page
    .getByRole("textbox", { name: "稳定资产 ID", exact: true })
    .fill("created-in-ui");
  await page
    .getByRole("textbox", { name: "参数显示名称", exact: true })
    .fill("界面定义");
  await page.getByRole("button", { name: "保存参数资产", exact: true }).click();
  await page.getByText("参数资产已保存", { exact: true }).waitFor();
  assert.equal(
    (
      await action("parameter.read", {
        group: "definitions",
        name: "created-in-ui",
      })
    ).data.asset.title,
    "界面定义",
  );
  t.step("参数管理结构化创建与保存");
  await page.getByRole("menuitem", { name: "模板语法", exact: true }).click();
  await page
    .getByRole("button", { name: "保存共享参数示例", exact: true })
    .click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "确认保存整个示例", exact: true })
    .click();
  await page.getByText("示例已保存，请在模板库打开", { exact: true }).waitFor();
  assert.ok(
    (await action("template.catalog")).data.items.some(
      (item) => item.title === "共享参数示例",
    ),
  );
  t.step("示例预检查及确认后整体保存");
  const bundle = await api("bundle");
  assert.ok(bundle.manifest.length >= 2);
  const mixed = {
    ...bundle,
    collections: { ...bundle.collections, nc: { version: 1, records: [] } },
  };
  for (const route of ["bundle/validate", "bundle/import"])
    assert.equal((await t.request(route, { bundle: mixed })).ok, false);
  t.step("集合导出和混合退役集合整体拒绝");
  const first = await api("runs", {
    action: "template.render",
    input: { source: "历史 {{ n }}", context: { n: 1 } },
    title: "历史比较样本",
  });
  const completed = await t.finished(first.id);
  assert.equal(completed.status, "succeeded");
  const original = JSON.stringify(await api("runs/" + first.id));
  const rerun = await api("runs/" + first.id + "/rerun", {});
  const repeated = await t.finished(rerun.id);
  assert.equal(
    repeated.result.artifacts[0].text,
    completed.result.artifacts[0].text,
  );
  assert.equal(repeated.replay.source_run_id, first.id);
  assert.equal(JSON.stringify(await api("runs/" + first.id)), original);
  await page.getByRole("menuitem", { name: "执行记录", exact: true }).click();
  await page.getByLabel("搜索执行记录", { exact: true }).fill("历史比较样本");
  await page
    .getByRole("button", { name: "历史比较样本", exact: true })
    .first()
    .click();
  await page
    .getByRole("button", { name: "恢复输入为新草稿", exact: true })
    .click();
  await page.getByRole("textbox", { name: "模板名称", exact: true }).waitFor();
  t.step("历史检索、冻结重执行不改原记录及恢复草稿");
  const recordPath = t.path.join(t.workspace, ".nctool/runs", first.id + ".json");
  const recordedBytes = t.fs.readFileSync(recordPath);
  const changedEnvironment = JSON.parse(recordedBytes);
  changedEnvironment.environment.host_version = "0.0.0-test";
  t.fs.writeFileSync(recordPath, JSON.stringify(changedEnvironment));
  await page.goto(t.url + "#/history/" + first.id);
  await page.getByText("执行环境已变化", {exact:true}).waitFor();
  assert.equal((await api("runs/" + first.id + "/replay-check")).status, "changed");
  delete changedEnvironment.environment;
  t.fs.writeFileSync(recordPath, JSON.stringify(changedEnvironment));
  await page.reload();
  await page.getByText("执行环境信息不完整", {exact:true}).waitFor();
  assert.equal((await api("runs/" + first.id + "/replay-check")).status, "unknown");
  t.fs.writeFileSync(recordPath, recordedBytes);
  await page.reload();
  await page.getByText("执行环境一致", {exact:true}).waitFor();
  await page.getByRole("button", {name:"恢复输入为新草稿",exact:true}).click();
  await page.getByRole("textbox", {name:"模板名称",exact:true}).waitFor();
  t.step("历史页面区分环境一致、已变化与信息不完整");
  await page.getByText("编辑模板", { exact: true }).click();
  await t.edit("模板源码", "Hello\n{% if %}");
  const invalid = await t.generate();
  assert.equal(invalid.status, "failed");
  assert.equal(invalid.error.code, "template_parse");
  const diagnostic = invalid.error.diagnostics[0];
  assert.equal(diagnostic.line, 2);
  assert.ok(diagnostic.column > 0);
  assert.equal(typeof diagnostic.template, "string");
  await page.getByRole("tab", {name: "检查信息 (1)", exact: true}).click();
  await page.getByRole("button", {name: "定位问题", exact: true}).click();
  await page.waitForFunction(() => {
    const selection = window.getSelection();
    return document.activeElement?.getAttribute("aria-label") === "模板源码" &&
      selection?.anchorNode?.parentElement?.closest(".cm-line")?.textContent?.includes("{% if %}");
  });
  const direct = await t.request("actions/template.render", {input:{source:"Hello\n{% if %}",context:{}}});
  assert.equal(direct.ok, false);
  assert.equal(direct.error.code, "template_parse");
  assert.equal(direct.error.diagnostics[0].line, 2);
  t.step("任务与 HTTP 保留结构化语法诊断，页面按行列定位");
}).catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
