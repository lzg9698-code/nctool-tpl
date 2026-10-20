import { describe, expect, it } from "vitest";
import { fieldErrors } from "./data";
import { buildStarterExample, type StarterExampleId } from "./starterExamples";

const ids: StarterExampleId[] = ["plain-text", "parameter-report"];

describe("buildStarterExample", () => {
  it.each(ids)("builds %s as a template-only bundle with local parameters", id => {
    const { example, bundle } = buildStarterExample(id);
    expect(example.id).toBe(id);
    expect(bundle.format).toBe("nctool-workspace");
    expect(bundle.version).toBe(1);
    expect(Object.keys(bundle.collections)).toEqual(["template"]);
    expect(bundle.collections.template.records).toHaveLength(1);
    const record = bundle.collections.template.records[0];
    expect(record.group).toBe("templates");
    expect(record.asset).toEqual({
      source: example.source,
      schema: example.schema,
      defaults: example.defaults,
      metadata: {
        title: example.title,
        output_name: example.outputName,
        execution: { action: "template.render" },
      },
    });
    expect(example.outputName).toBe(`${record.name}.txt`);
    expect(bundle.manifest).toEqual([{
      collection: "template",
      provider: "template",
      version: 1,
      fingerprint: expect.stringMatching(/^fnv1a64:[a-f0-9]{16}$/),
    }]);
    for (const key of ["bindings", "definition", "preset", "executionAction"]) {
      expect(example).not.toHaveProperty(key);
    }
    expect(fieldErrors(example.schema, example.defaults)).toEqual({});
  });

  it("describes the greeting's required local name field and matching output", () => {
    const { example } = buildStarterExample("plain-text");
    expect(example.defaults).toEqual({ user: { name: "Ada" } });
    expect(example.schema.required).toEqual(["user"]);
    const user = example.schema.properties?.user;
    expect(user?.required).toEqual(["name"]);
    expect(user?.properties?.name).toEqual({ type: "string", title: "姓名" });
    expect(example.source.replace("{{ user.name }}", example.defaults.user.name)).toBe(example.expected);
    expect(fieldErrors(example.schema, { user: { name: "" } })).toHaveProperty("/user/name");
  });

  it("describes the report's required local fields and matching output", () => {
    const { example } = buildStarterExample("parameter-report");
    expect(example.defaults).toEqual({ report: { name: "零件 A", unit: "件" } });
    expect(example.schema.required).toEqual(["report"]);
    const report = example.schema.properties?.report;
    expect(report?.required).toEqual(["name", "unit"]);
    expect(report?.properties).toEqual({
      name: { type: "string", title: "名称" },
      unit: { type: "string", title: "单位" },
    });
    expect(example.source
      .replace("{{ report.name }}", example.defaults.report.name)
      .replace("{{ report.unit }}", example.defaults.report.unit)).toBe(example.expected);
    expect(Object.keys(fieldErrors(example.schema, { report: {} }))).toEqual(["/report/name", "/report/unit"]);
  });
});
