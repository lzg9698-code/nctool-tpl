// Additional tests. Existing browser_review_fixes.cjs remains intact.
const { suite } = require("./browser_rewrite_support.cjs");
suite("rewrite-plugins", async (t) => {
  const { fs, path, assert, api, action } = t;
  const fixture = path.join(t.temp, "provider");
  fs.cpSync(path.join(t.root, "examples/plugins/python-report"), fixture, {
    recursive: true,
  });
  const file = path.join(fixture, "plugin.json"),
    manifest = JSON.parse(fs.readFileSync(file, "utf8"));
  manifest.command[0] = process.env.NCTOOL_PYTHON || "python3";
  const alternate = structuredClone(manifest.actions[0]);
  alternate.id = "report.alternate";
  alternate.title = "另一份报告";
  for (const entry of [manifest.actions[0], alternate])
    entry.ui_schema = {
      document_input: {
        title: entry.title,
        bindings: { values: "metadata.report.values", title: "metadata.title" },
      },
    };
  manifest.actions.push(alternate);
  manifest.descriptor.panels[0].actions.push(alternate.id);
  fs.writeFileSync(file, JSON.stringify(manifest));
  const provider = path.join(fixture, "plugin.py");
  fs.writeFileSync(
    provider,
    fs
      .readFileSync(provider, "utf8")
      .replace(
        'if action != "report.compute":',
        'if action not in ["report.compute", "report.alternate"]:',
      )
      .replace(
        "    total = sum",
        '    if input_data.get("title") == "slow":\n        import time\n        time.sleep(3)\n    if input_data.get("title") == "fail":\n        raise ValueError("intentional failure")\n    total = sum',
      ),
  );
  await t.boot();
  const page = t.page;
  await page.getByRole("menuitem", { name: "插件与设置", exact: true }).click();
  await page
    .getByRole("textbox", { name: "插件目录", exact: true })
    .fill(fixture);
  await page.getByRole("button", { name: "读取插件信息", exact: true }).click();
  await page.getByRole("button", { name: "安装插件", exact: true }).click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "安装插件", exact: true })
    .click();
  await page
    .getByText("插件已安装，请配置启用并重启服务", { exact: true })
    .waitFor();
  await page
    .getByRole("switch", { name: "启用 python-report", exact: true })
    .click();
  await page
    .getByRole("button", { name: "验证并保存配置", exact: true })
    .click();
  await page.getByText("配置已保存，下次启动生效", { exact: true }).waitFor();
  assert.ok((await api("config")).config.enabled.includes("python-report"));
  t.step("插件检查、安装与下次启动配置保存");
  t.stop(t.children[0]);
  await t.serve();
  await page.goto(t.url);
  await page.getByRole("menuitem", { name: "数据报告", exact: true }).click();
  await page.getByRole("tab", { name: "JSON", exact: true }).click();
  await t.edit("插件输入 JSON", '{"values":');
  let posts = 0;
  page.on("request", (request) => {
    if (request.url().endsWith("/api/v2/runs") && request.method() === "POST")
      posts++;
  });
  assert.equal(
    await page
      .getByRole("button", { name: "执行操作", exact: true })
      .isDisabled(),
    true,
  );
  await page.getByRole("menuitem", { name: "执行记录", exact: true }).click();
  await page.getByRole("menuitem", { name: "数据报告", exact: true }).click();
  await page.getByRole("tab", { name: "JSON", exact: true }).click();
  assert.equal(
    await page.locator('.cm-content[aria-label="插件输入 JSON"]').innerText(),
    '{"values":',
  );
  await page.reload();
  await page.getByRole("tab", { name: "JSON", exact: true }).click();
  assert.equal(
    await page.locator('.cm-content[aria-label="插件输入 JSON"]').innerText(),
    '{"values":',
  );
  assert.equal(posts, 0);
  t.step("插件非法 JSON 禁止执行并跨页、刷新保留");
  for (const raw of ["[]", "null", '{"values":[-]}']) {
    await t.edit("插件输入 JSON", raw);
    assert.equal(
      await page
        .getByRole("button", { name: "执行操作", exact: true })
        .isDisabled(),
      true,
    );
    assert.equal(posts, 0);
  }
  await t.edit("插件输入 JSON", '{"values":[999],"title":"report"}');
  const [response] = await Promise.all([
    page.waitForResponse(
      (res) =>
        res.url().endsWith("/api/v2/runs") && res.request().method() === "POST",
    ),
    page.getByRole("button", { name: "执行操作", exact: true }).click(),
  ]);
  let run = await t.finished((await response.json()).data.id);
  assert.equal(run.status, "succeeded");
  assert.deepEqual(run.input.values, [999]);
  await page.getByRole("button", { name: "导出文件", exact: true }).waitFor();
  await t.edit("插件输入 JSON", '{"values":[1000],"title":"report"}');
  assert.equal(
    await page
      .getByRole("button", { name: "导出文件", exact: true })
      .isDisabled(),
    true,
  );
  t.step("插件实际输入、根对象约束和过期交付门禁");
  await t.select("选择插件操作", "另一份报告");
  await page.getByRole("tab", { name: "JSON", exact: true }).click();
  await t.edit("插件输入 JSON", '{"values":[5],"title":"slow"}');
  const [slowResponse] = await Promise.all([
    page.waitForResponse(
      (res) =>
        res.url().endsWith("/api/v2/runs") && res.request().method() === "POST",
    ),
    page.getByRole("button", { name: "执行操作", exact: true }).click(),
  ]);
  const slowId = (await slowResponse.json()).data.id;
  await page.getByRole("button", { name: "取消任务", exact: true }).click();
  run = await t.finished(slowId);
  assert.equal(run.status, "cancelled");
  assert.equal(run.action, "report.alternate");
  t.step("动态动作切换与真实后台取消");
  await t.edit("插件输入 JSON", '{"values":[5],"title":"fail"}');
  const [failed] = await Promise.all([
    page.waitForResponse(
      (res) =>
        res.url().endsWith("/api/v2/runs") && res.request().method() === "POST",
    ),
    page.getByRole("button", { name: "执行操作", exact: true }).click(),
  ]);
  assert.equal(
    (await t.finished((await failed.json()).data.id)).status,
    "failed",
  );
  t.step("插件失败保留错误且不交付");
  for (const metadata of [
    { nc: null },
    { nc: {} },
    { execution: { action: "nc.generate" } },
  ]) {
    const name = "retired-" + Math.random().toString(16).slice(2);
    await action("template.save", {
      name,
      asset: { source: "x", metadata },
      expected: null,
    });
    const result = await t.request("actions/template.render", {
      input: { template: name, context: {} },
    });
    assert.equal(result.ok, false);
    assert.equal(result.error.code, "domain_action_required");
  }
  for (const raw of ["18446744073709551616", "1e-400"])
    assert.equal(
      (
        await t.request(
          "actions/template.render",
          '{"input":{"source":"{{ n }}","context":{"n":' + raw + "}}}",
        )
      ).ok,
      false,
    );
  assert.equal(
    (
      await t.request(
        "actions/template.render",
        '{"input":{"source":"{{ n }}","context":{"n":9007199254740993}}}',
      )
    ).data.data.text,
    "9007199254740993",
  );
  t.step("退役意图拒绝及真实 HTTP 数值边界");
  await action("template.save", {
    name: "explicit",
    asset: {
      source: "unused",
      metadata: {
        title: "显式插件模板",
        report: { values: [7] },
        execution: { action: "report.alternate" },
      },
    },
    expected: null,
  });
  await page.getByRole("menuitem", { name: "模板工作台", exact: true }).click();
  await page.getByRole("button", { name: "刷新", exact: true }).click();
  await t.open("显式插件模板");
  run = await t.generate();
  assert.equal(run.action, "report.alternate");
  await page.getByRole("button", { name: "保存模板", exact: true }).click();
  await page.getByText("模板已保存", { exact: true }).waitFor();
  assert.equal(
    (await action("template.read", { name: "explicit" })).data.asset.metadata
      .execution.action,
    "report.alternate",
  );
  t.step("显式文档动作执行和保存不降级");
}).catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
