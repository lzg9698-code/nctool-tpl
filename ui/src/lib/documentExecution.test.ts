import { describe, expect, it } from "vitest";
import { newDocument, parseData, encode, DraftNumber } from "./data";
import {
  documentInput,
  selectDocumentAction,
  documentSignature,
} from "./documentExecution";
const text = {
  id: "template.render",
  ui_schema: {
    document_input: {
      default: true,
      bindings: { source: "source", context: "context", defaults: "defaults" },
    },
  },
};
const nc = {
  id: "nc.generate",
  ui_schema: {
    document_input: {
      match_metadata: "nc",
      bindings: {
        source: "source",
        params: "effective_context",
        specs: "metadata.nc.specs",
        trim_blocks: "metadata.nc.render_options.trim_blocks",
      },
    },
  },
};
describe("plugin contributions to document execution", () => {
  it("chooses ordinary rendering for unmarked documents and NC validation for NC assets", () => {
    const doc = newDocument();
    expect(selectDocumentAction(doc, [text, nc]).entry.id).toBe(
      "template.render",
    );
    doc.asset.metadata = { nc: { specs: [] } };
    expect(selectDocumentAction(doc, [text, nc]).entry.id).toBe("nc.generate");
    doc.generationAction = "template.render";
    expect(selectDocumentAction(doc, [text, nc]).entry.id).toBe(
      "template.render",
    );
  });
  it("preserves exact parameters and edited source/specs while omitting unrelated input fields", () => {
    const doc = newDocument();
    doc.asset.source = "unsaved {{ x | nc_fixed(3) }}";
    doc.asset.defaults = { x: 1, other: 0 };
    doc.context = parseData('{"x":9007199254740993}');
    doc.asset.metadata = {
      nc: {
        specs: [{ name: "x", kind: "integer" }],
        render_options: { trim_blocks: true },
      },
    };
    const input = documentInput(doc, nc);
    expect(encode(input.params)).toBe('{"x":9007199254740993,"other":0}');
    expect(input.source).toBe(doc.asset.source);
    expect(input.specs).toEqual(doc.asset.metadata.nc.specs);
    expect(input.trim_blocks).toBe(true);
    expect(input).not.toHaveProperty("context");
    expect(input).not.toHaveProperty("schema");
    doc.context.x = new DraftNumber("-");
    expect(() => documentInput(doc, nc)).toThrow();
  });
  it("supports non-NC providers and blocks unavailable or ambiguous choices instead of falling back", () => {
    const doc = newDocument();
    doc.asset.metadata = { report: { format: "csv" } };
    const report = {
      id: "report.make",
      ui_schema: {
        document_input: {
          match_metadata: "report",
          bindings: {
            payload: "effective_context",
            format: "metadata.report.format",
          },
        },
      },
    };
    expect(selectDocumentAction(doc, [text, report]).entry.id).toBe(
      "report.make",
    );
    expect(documentInput(doc, report)).toEqual({ payload: {}, format: "csv" });
    expect(
      selectDocumentAction(doc, [
        text,
        report,
        { ...report, id: "other.report" },
      ]).error,
    ).toMatch("多个插件");
    doc.generationAction = "report.make";
    expect(selectDocumentAction(doc, [text]).entry).toBeUndefined();
    expect(selectDocumentAction(doc, [text]).error).toMatch("不可用");
  });
  it("blocks unavailable domain metadata on import and draft recovery while permitting explicit text", () => {
    const doc = newDocument();
    doc.asset.metadata = { nc: { specs: [] } };
    expect(selectDocumentAction(doc, [text], [text, nc]).entry).toBeUndefined();
    expect(selectDocumentAction(doc, [text], [text, nc]).error).toMatch(
      "不可用",
    );
    doc.asset.metadata.execution = { action: "nc.generate" };
    expect(selectDocumentAction(doc, [text]).error).toMatch("不可用");
    doc.generationAction = "template.render";
    expect(selectDocumentAction(doc, [text], [text, nc]).entry?.id).toBe(
      "template.render",
    );
  });
  it("invalidates old output when switching providers or editing domain metadata", () => {
    const doc = newDocument();
    const original = documentSignature(doc, text.id);
    expect(documentSignature(doc, nc.id)).not.toBe(original);
    doc.asset.metadata = { nc: { specs: [{ name: "x", min: 5 }] } };
    expect(documentSignature(doc, text.id)).not.toBe(original);
  });
});
