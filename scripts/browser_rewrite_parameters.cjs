// Additive tests for the new workbench; the existing three suites are retained.
const { suite } = require("./browser_rewrite_support.cjs");
suite("rewrite-parameters", async (t) => {
  const { assert, action, api } = t;
  await action("parameter.save", {
    group: "definitions",
    name: "width",
    asset: {
      format_version: 1,
      id: "width",
      title: "宽度",
      schema: { type: "number" },
      default: 4,
    },
    expected: null,
  });
  await action("parameter.save", {
    group: "presets",
    name: "wide",
    asset: {
      format_version: 1,
      id: "wide",
      title: "宽预置",
      values: { width: 12 },
    },
    expected: null,
  });
  await action("template.save", {
    name: "shared",
    asset: {
      source: "{{ width }} / {{ label }}",
      schema: {
        type: "object",
        properties: {
          width: { type: "number", title: "宽度" },
          label: { type: "string", title: "说明" },
        },
        required: ["width"],
      },
      defaults: { label: "模板默认" },
      metadata: {
        title: "共享参数验收",
        execution: { action: "template.render.managed" },
        parameters: { bindings: { "/width": "width" } },
      },
    },
    expected: null,
  });
  await t.boot();
  const page = t.page;
  await t.open("共享参数验收");
  const width = page.getByRole("textbox", { name: "宽度", exact: true });
  await page.waitForFunction(
    () => document.querySelector('input[aria-label="宽度"]')?.value === "4",
  );
  assert.equal(
    await page.getByRole("textbox", { name: "说明", exact: true }).inputValue(),
    "模板默认",
  );
  t.step("共享定义与模板默认值继承");
  await width.fill("8");
  await t.select("本次共享预置", "宽预置");
  assert.equal(await width.inputValue(), "8");
  await page
    .locator('[data-field-path="/width"]')
    .getByRole("button", { name: "恢复跟随", exact: true })
    .click();
  await page.waitForFunction(
    () => document.querySelector('input[aria-label="宽度"]')?.value === "12",
  );
  await t.select("本次共享预置", "不使用共享预置");
  await page.waitForFunction(
    () => document.querySelector('input[aria-label="宽度"]')?.value === "4",
  );
  t.step("覆盖跨预置保留、恢复跟随与取消预置");
  let posts = 0;
  page.on("request", (request) => {
    if (request.url().endsWith("/api/v2/runs") && request.method() === "POST")
      posts++;
  });
  await width.fill("1e");
  assert.equal(
    await page
      .getByRole("button", { name: "生成文件", exact: true })
      .isDisabled(),
    true,
  );
  await page.reload();
  await width.waitFor();
  assert.equal(await width.inputValue(), "1e");
  assert.equal(posts, 0);
  t.step("未完成数字阻止提交并逐字恢复");
  await width.fill("8");
  await page.getByText("JSON", { exact: true }).first().click();
  await t.edit("参数 JSON", '{"width":');
  assert.equal(
    await page
      .getByRole("button", { name: "生成文件", exact: true })
      .isDisabled(),
    true,
  );
  assert.equal(posts, 0);
  await t.edit("参数 JSON", '{"width":8}');
  const run = await t.generate();
  assert.equal(run.status, "succeeded");
  assert.deepEqual(run.input.context, { width: 8 });
  assert.equal(run.input.preset_id, null);
  assert.equal(run.result.artifacts[0].text, "8 / 模板默认");
  t.step("无效 JSON 阻止执行，修正后仅发送本次覆盖");
  const definition = (
    await action("parameter.read", { group: "definitions", name: "width" })
  ).data;
  await action("parameter.save", {
    group: "definitions",
    name: "width",
    asset: { ...definition.asset, default: 6 },
    expected: definition.fingerprint,
  });
  await page
    .getByText("模板或参数依赖尚未确认，或已变化，请重新生成", { exact: true })
    .waitFor();
  assert.equal(
    await page
      .getByRole("button", { name: "导出文件", exact: true })
      .isDisabled(),
    true,
  );
  t.step("共享依赖变化使旧结果不可交付");
  await page.getByRole("menuitem", { name: "模板语法", exact: true }).click();
  const mathCard = page
    .locator(".ant-card")
    .filter({ hasText: "数学扩展与角度单位" });
  assert.equal(
    await mathCard
      .getByRole("button", { name: "试跑示例", exact: true })
      .isDisabled(),
    true,
  );
  t.step("未启用 math 时禁止扩展示例");
  const inventory = await api("plugins");
  if (!inventory.plugins.find((plugin) => plugin.id === "math")?.available) {
    console.log("LIMIT: math not compiled; extension cases not run");
    return;
  }
  const config = await api("config");
  await api("config", {
    config: {
      ...config.config,
      enabled: [...new Set([...config.config.enabled, "math"])],
    },
    expected: config.fingerprint,
  });
  t.stop(t.children[0]);
  await t.serve();
  await page.goto(t.url);
  await page.getByRole("menuitem", { name: "模板语法", exact: true }).click();
  const [sampleResponse] = await Promise.all([
    page.waitForResponse(
      (res) =>
        res.url().endsWith("/api/v2/runs") && res.request().method() === "POST",
    ),
    mathCard.getByRole("button", { name: "试跑示例", exact: true }).click(),
  ]);
  const sample = await t.finished((await sampleResponse.json()).data.id);
  assert.equal(sample.status, "succeeded");
  assert.deepEqual(sample.input.extensions, ["math"]);
  assert.equal(sample.result.data.text, "3.0");
  t.step("启用 math 后示例携带显式 extensions");
  assert.equal(
    (
      await t.request("actions/template.render", {
        input: { source: "{{ 9 | sqrt }}", context: {} },
      })
    ).ok,
    false,
  );
  assert.equal(
    (
      await action("template.render", {
        source: "{{ 9 | sqrt }}",
        context: {},
        extensions: ["math"],
      })
    ).data.text,
    "3.0",
  );
  await action("template.save", {
    name: "math-test",
    asset: {
      source: "{{ 9 | sqrt }}",
      metadata: {
        title: "数学模板",
        execution: { action: "template.render", extensions: ["math"] },
      },
    },
    expected: null,
  });
  await page.getByRole("menuitem", { name: "模板工作台", exact: true }).click();
  await page.getByRole("button", { name: "刷新", exact: true }).click();
  await t.open("数学模板");
  const mathRun = await t.generate();
  assert.equal(mathRun.status, "succeeded");
  assert.deepEqual(mathRun.input.extensions, ["math"]);
  t.step("HTTP 与工作台均要求显式选择扩展");
  const original = JSON.stringify(await api("runs/" + mathRun.id));
  const repeated = await t.finished(
    (await api(`runs/${mathRun.id}/rerun`, {})).id,
  );
  assert.equal(repeated.status, "succeeded");
  assert.deepEqual(repeated.input.extensions, ["math"]);
  assert.equal(JSON.stringify(await api("runs/" + mathRun.id)), original);
  t.step("math 历史重执行保留扩展且不改写原历史");
}).catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
