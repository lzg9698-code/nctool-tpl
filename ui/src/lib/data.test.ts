import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  encode,
  parseData,
  DraftNumber,
  fieldErrors,
  newDocument,
  isDirty,
  documentValue,
  type Schema,
} from "./data";
import { saveDrafts, loadDrafts } from "./drafts";
describe("lossless parameters", () => {
  it("retains integers outside the IEEE safe range and nonzero tiny decimals", () => {
    const text =
      '{"integer":9007199254740993,"tiny":1e-400,"zero":0,"flag":false,"empty":null}';
    expect(encode(parseData(text))).toBe(text);
  });
  it("serializes numeric inputs as numeric literals rather than strings or rounded doubles", () => {
    expect(
      encode({
        value: new DraftNumber("1e-400"),
        id: new DraftNumber("9007199254740993"),
      }),
    ).toBe('{"value":1e-400,"id":9007199254740993}');
    expect(() => encode({ value: new DraftNumber("-") })).toThrow();
  });
  it("does not treat false, zero or explicitly nullable values as absent", () => {
    const schema: Schema = {
      type: "object",
      properties: {
        flag: { type: "boolean" },
        count: { type: "integer" },
        optional: { type: ["string", "null"] },
      },
      required: ["flag", "count", "optional"],
    };
    expect(
      fieldErrors(schema, {
        flag: false,
        count: parseData("0"),
        optional: null,
      }),
    ).toEqual({});
  });
  it("validates nested array fields without collapsing object data to JSON text", () => {
    const schema: Schema = {
      type: "object",
      properties: {
        parts: {
          type: "array",
          items: {
            type: "object",
            properties: { name: { type: "string" }, depth: { type: "number" } },
            required: ["name", "depth"],
          },
        },
      },
    };
    expect(
      fieldErrors(schema, {
        parts: [{ name: "", depth: new DraftNumber(".") }],
      }),
    ).toEqual({
      "/parts/0/name": "请填写name",
      "/parts/0/depth": "请输入有效数字",
    });
  });
  it("detects unresolved parameter types", () => {
    expect(
      fieldErrors(
        { type: "object", properties: { x: { "x-untyped": true } } },
        {},
      ),
    ).toHaveProperty("/x");
  });
  it("tracks changes to the actual template and output filename independently of run parameters", () => {
    const doc = newDocument();
    doc.saved = documentValue(doc);
    expect(isDirty(doc)).toBe(false);
    doc.context = { foo: "runtime" };
    expect(isDirty(doc)).toBe(false);
    doc.outputName = "result.csv";
    expect(isDirty(doc)).toBe(true);
  });
});
describe("workspace draft recovery", () => {
  beforeEach(() => {
    const map = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => map.get(key) || null,
      setItem: (key: string, value: string) => map.set(key, value),
    });
  });
  it("preserves unfinished numeric input, source edits, nulls and false across refresh", () => {
    const doc = newDocument();
    doc.asset.source = "draft {{ x }}";
    doc.context = { x: new DraftNumber("-"), flag: false, nullable: null };
    expect(saveDrafts("a", [doc], doc.key)).toBe(true);
    const restored = loadDrafts("a");
    expect(restored.documents[0].context.x.raw).toBe("-");
    expect(restored.documents[0].context.flag).toBe(false);
    expect(restored.documents[0].asset.source).toBe(doc.asset.source);
    expect(restored.active).toBe(doc.key);
  });
  it("does not confuse user objects with tagged numeric storage", () => {
    const doc = newDocument();
    doc.context = {
      user: { __nctool_raw_number: "literal", raw: "-" },
      safe: parseData("9007199254740993"),
    };
    saveDrafts("a", [doc], doc.key);
    expect(encode(loadDrafts("a").documents[0].context)).toBe(
      encode(doc.context),
    );
    expect(loadDrafts("b").documents).toEqual([]);
  });
});
it("tagged plugin draft storage retains unfinished numeric tokens", async () => {
  const { encodeDraft, decodeDraft } = await import("./drafts");
  const restored = decodeDraft<{ value: DraftNumber }>(
    encodeDraft({ value: new DraftNumber("1e-") }),
  );
  expect(restored.value.raw).toBe("1e-");
});
