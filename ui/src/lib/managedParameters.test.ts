import { describe, expect, it } from "vitest";
import { encode, newDocument, parseData, type Run } from "./data";
import {
  applyOverrides,
  managedFields,
  managedFormSchema,
  managedRawInput,
  managedRequest,
  isStructured,
  validatedSelection,
  originLabel,
  pointerEscape,
  pointerTokens,
  pointerValue,
  removePointer,
} from "./managedParameters";
import { restoreDocument } from "./documentExecution";

describe("NC parameter compatibility", () => {
  it("locks only bound managed tool fields and preserves raw numeric input", () => {
    const schema = { type: "object", properties: { "a/b": { type: "number" }, free: { type: "number" } } };
    const bindings = { "/a~1b": { slot: "main" } };
    const selection = { slots: { main: { tool_id: "tool" } } };
    expect(managedFormSchema(schema, bindings, selection).properties["a/b"].readOnly).toBe(true);
    expect(schema.properties["a/b"]).not.toHaveProperty("readOnly");
    expect(encode(managedRawInput(parseData('{"a/b":1,"free":9007199254740993}'), bindings, selection))).toBe('{"free":9007199254740993}');
    expect(() => validatedSelection({ slots: { main: { inputs_draft: "{" } } })).toThrow("JSON");
    expect(isStructured(parseData("9007199254740993"))).toBe(false);
    expect(isStructured({ width: 1 })).toBe(true);
  });
});

describe("shared parameter pointers", () => {
  it("escapes and unescapes RFC 6901 tokens", () => {
    expect(pointerEscape("a/b")).toBe("a~1b");
    expect(pointerEscape("a~b")).toBe("a~0b");
    expect(pointerTokens("/a~1b~0c")).toEqual(["a/b~c"]);
  });
  it("reads nested and array pointers", () => {
    const values = parseData('{"report":{"width":30},"rows":[{"depth":1},{"depth":2}]}');
    expect(encode(pointerValue(values, "/report/width"))).toBe("30");
    expect(encode(pointerValue(values, "/rows/1/depth"))).toBe("2");
    expect(pointerValue(values, "/report/missing")).toBeUndefined();
    expect(pointerValue(values, "/rows/9/depth")).toBeUndefined();
  });
  it("removes a leaf and prunes emptied parents", () => {
    expect(removePointer({ report: { width: 30 } }, "/report/width")).toEqual({});
    expect(
      removePointer({ report: { width: 30, height: 5 } }, "/report/width"),
    ).toEqual({ report: { height: 5 } });
    expect(removePointer({ keep: 1 }, "/gone/deep")).toEqual({ keep: 1 });
    // Array indices are rejected: removing one would shift the rest.
    expect(removePointer({ rows: [{ a: 1 }, { a: 2 }] }, "/rows/0")).toEqual({
      rows: [{ a: 1 }, { a: 2 }],
    });
  });
  it("labels every origin the backend can report", () => {
    expect(originLabel("definition")).toBe("共享定义默认值");
    expect(originLabel("spec_default")).toBe("规格默认值");
    expect(originLabel("template_default")).toBe("模板默认值");
    expect(originLabel("manual")).toBe("本次填写");
    expect(originLabel("preset:compact")).toBe("共享预置：compact");
  });
});

describe("override layering", () => {
  // Values are compared as JSON text: the form carries LosslessNumber tokens.
  const json = (value: unknown) => encode(value);
  it("records only the leaves the user changed", () => {
    const shown = parseData('{"report":{"width":25,"title":"A"},"count":3}');
    const edited = parseData('{"report":{"width":30,"title":"A"},"count":3}');
    // The inherited width and title must not become manual overrides.
    expect(json(applyOverrides(shown, edited, {}))).toBe(
      '{"report":{"width":30}}',
    );
  });
  it("keeps a value the user deliberately set to the inherited one", () => {
    const shown = parseData('{"width":25}');
    const edited = parseData('{"width":25}');
    expect(applyOverrides(shown, edited, {})).toEqual({});
  });
  it("keeps existing overrides while adding a new edit", () => {
    const shown = parseData('{"width":25,"depth":4}');
    const edited = parseData('{"width":30,"depth":4}');
    expect(json(applyOverrides(shown, edited, { depth: 9 }))).toBe(
      '{"depth":9,"width":30}',
    );
  });
  it("clears an override when the field is emptied", () => {
    const shown = parseData('{"report":{"width":30}}');
    const edited = parseData('{"report":{}}');
    expect(json(applyOverrides(shown, edited, { report: { width: 30 } }))).toBe(
      '{"report":{}}',
    );
  });
  it("treats a replaced object or array as a single change", () => {
    expect(
      json(
        applyOverrides(
          { report: { width: 1, title: "A" } },
          { report: { title: "B" } },
          {},
        ),
      ),
    ).toBe('{"report":{"title":"B"}}');
    expect(json(applyOverrides({ rows: [1, 2] }, { rows: [1] }, {}))).toBe(
      '{"rows":[1]}',
    );
  });
});

describe("managed request construction", () => {
  it("sends the same layers the backend resolves", () => {
    const doc = newDocument();
    doc.asset = {
      source: "{{ report.width }}",
      defaults: { report: { width: 25 } },
      schema: { type: "object" },
      metadata: { parameters: { bindings: { "/report/width": "report-width" } } },
    };
    doc.context = { report: { width: 30 } };
    const request = managedRequest(doc);
    expect(request.bindings).toEqual({ "/report/width": "report-width" });
    expect(encode(request.defaults)).toBe('{"report":{"width":25}}');
    expect(encode(request.values)).toBe('{"report":{"width":30}}');
    expect(request).not.toHaveProperty("preset_id");
  });
  it("keeps an explicit decline and omits an absent choice", () => {
    const doc = newDocument();
    doc.asset = { source: "", metadata: { parameters: { preset_id: "rough" } } };
    doc.parameterSelection = { preset_id: null };
    expect(managedRequest(doc).preset_id).toBeNull();
    doc.parameterSelection = { preset_id: "finish" };
    expect(managedRequest(doc).preset_id).toBe("finish");
    doc.parameterSelection = undefined;
    expect(managedRequest(doc).preset_id).toBe("rough");
    doc.parameterSelection = {};
    expect(managedRequest(doc).preset_id).toBe("rough");
    doc.asset.metadata = { parameters: { selection: { preset_id: null }, preset_id: "rough" } };
    doc.parameterSelection = undefined;
    expect(managedRequest(doc).preset_id).toBeNull();
    doc.asset.metadata = {};
    expect(managedRequest(doc)).not.toHaveProperty("preset_id");
  });
  it("lists only schema-backed fields", () => {
    expect(
      managedFields({ schema: { type: "object", properties: { a: {}, b: {} } } }),
    ).toEqual(["a", "b"]);
    expect(managedFields({ schema: true })).toEqual([]);
    expect(managedFields(undefined)).toEqual([]);
  });
});

describe("history restoration keeps the frozen preset choice", () => {
  const managed = {
    id: "template.render.managed",
    ui_schema: {
      document_input: {
        bindings: { source: "source", template: "id", context: "context" },
      },
    },
  };
  function history(presetId: unknown): Run {
    return {
      id: "r1",
      title: "History",
      action: "template.render.managed",
      input: {},
      status: "succeeded",
      started_at: 0,
      result: {
        data: {
          replay_input: {
            snapshot: { template: "t", templates: { t: "src" }, asset: {} },
            parameter_request: { bindings: {}, values: { width: 30 } },
            preset_id: presetId,
          },
        },
        artifacts: [],
        diagnostics: [],
      },
    };
  }
  it("restores a declined preset as an explicit decline", () => {
    expect(restoreDocument(history(null), [managed]).parameterSelection).toEqual(
      { preset_id: null },
    );
  });
  it("restores a chosen preset and an absent one distinctly", () => {
    expect(
      restoreDocument(history("finish"), [managed]).parameterSelection,
    ).toEqual({ preset_id: "finish" });
    expect(
      restoreDocument(history(undefined), [managed]).parameterSelection,
    ).toEqual({});
  });
});
