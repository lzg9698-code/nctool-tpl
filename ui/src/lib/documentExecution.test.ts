import { describe, expect, it } from "vitest";
import { newDocument, parseData, encode, DraftNumber, type Run } from "./data";
import { decodeDraft, encodeDraft } from "./drafts";
import {
  activePanels,
  documentActions,
  documentInput,
  selectDocumentAction,
  documentSignature,
  historyExecutionReason,
  restoreDocument,
} from "./documentExecution";
const text = {
  id: "template.render",
  ui_schema: {
    document_input: {
      default: true,
      bindings: {
        source: "source",
        context: "context",
        defaults: "defaults",
        schema: "schema",
        extensions: "extensions",
      },
    },
  },
};
const managed = {
  id: "template.render.managed",
  ui_schema: {
    document_input: {
      match_metadata: "parameters",
      bindings: {
        source: "source",
        template: "id",
        context: "context",
        defaults: "defaults",
        schema: "schema",
        bindings: "metadata.parameters.bindings",
        preset_id: "parameter_preset_id",
        extensions: "extensions",
      },
    },
  },
};
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
function run(action = "template.render.managed"): Run {
  return {
    id: "r1",
    title: "History",
    action,
    input: {},
    status: "succeeded",
    started_at: 0,
  };
}
describe("generic document execution and retired intent", () => {
  it("keeps generic metadata ordinary unless an external provider declares it", () => {
    const doc = newDocument();
    doc.asset.metadata = {
      title: "NC machine",
      report: { format: "csv" },
      machine: {},
      process: {},
      turning: {},
    };
    expect(selectDocumentAction(doc, [text]).entry?.id).toBe(text.id);
    expect(selectDocumentAction(doc, [text, report]).entry?.id).toBe(report.id);
    expect(documentInput(doc, report)).toEqual({ payload: {}, format: "csv" });
    expect(
      selectDocumentAction(doc, [
        text,
        report,
        { ...report, id: "other.report" },
      ]).error,
    ).toMatch("多个插件");
    doc.generationAction = report.id;
    expect(selectDocumentAction(doc, [text]).error).toMatch("不可用");
  });
  it("blocks old NC metadata including null even with explicit ordinary render and no known retired provider", () => {
    for (const nc of [null, {}, { operation: { id: "groove" } }, false]) {
      const doc = newDocument();
      doc.asset.metadata = { nc };
      for (const requested of [undefined, text.id]) {
        doc.generationAction = requested;
        expect(selectDocumentAction(doc, [text]).entry).toBeUndefined();
        expect(selectDocumentAction(doc, [text]).error).toMatch("退役");
        expect(() => documentInput(doc, text)).toThrow("退役");
      }
    }
  });
  it("blocks retired execution actions and recovered action selections until intent is explicitly removed", () => {
    for (const id of [
      "nc.generate",
      "process.generate.managed",
      "sync-turning.generate",
      "undercut.generate",
    ]) {
      const doc = newDocument();
      doc.asset.metadata = { execution: { action: id } };
      doc.generationAction = text.id;
      expect(selectDocumentAction(doc, [text]).error).toMatch("退役");
      delete doc.asset.metadata.execution;
      doc.generationAction = id;
      expect(selectDocumentAction(doc, [text]).error).toMatch("退役");
      doc.generationAction = undefined;
      expect(selectDocumentAction(doc, [text]).entry?.id).toBe(text.id);
    }
  });
  it("omits retired contributed actions/panels without removing unknown external schema pages", () => {
    const retired = { ...text, id: "nc.generate" };
    expect(documentActions([text, report, retired])).toEqual([text, report]);
    expect(
      activePanels(
        [
          { id: "templates", view: "template-editor", actions: [text.id] },
          { id: "tools", view: "nc.parameters", actions: [retired.id] },
          { id: "legacy", view: "unknown", actions: [retired.id] },
          { id: "parameters", view: "parameter.library", actions: [] },
          {
            id: "report",
            view: "report.form",
            actions: [report.id, retired.id],
          },
        ],
        [text, report, retired],
      ),
    ).toEqual([
      { id: "parameters", view: "parameter.library", actions: [] },
      { id: "report", view: "report.form", actions: [report.id] },
    ]);
  });
  it("preserves exact and nested JSON input, and rejects incomplete numeric drafts", () => {
    const doc = newDocument();
    doc.asset.metadata = { report: { format: "csv" } };
    doc.asset.defaults = { other: null };
    doc.context = parseData('{"x":9007199254740993,"nested":{"enabled":true}}');
    expect(encode(documentInput(doc, report).payload)).toBe(
      '{"other":null,"x":9007199254740993,"nested":{"enabled":true}}',
    );
    doc.context.x = new DraftNumber("-");
    expect(() => documentInput(doc, report)).toThrow();
  });
  it("forwards only explicit extensions on plain and managed rendering and invalidates changed choices", () => {
    const doc = newDocument();
    expect(documentInput(doc, text).extensions).toEqual([]);
    doc.asset.metadata = {
      parameters: {},
      execution: { extensions: ["math"] },
    };
    expect(selectDocumentAction(doc, [text, managed]).entry?.id).toBe(
      managed.id,
    );
    expect(documentInput(doc, managed).extensions).toEqual(["math"]);
    const original = documentSignature(doc, managed.id);
    doc.extensions = [];
    expect(documentInput(doc, managed).extensions).toEqual([]);
    expect(documentSignature(doc, managed.id)).not.toBe(original);
  });
});
describe("history recovery", () => {
  it("lets an explicit empty selection clear an asset default preset", () => {
    const doc = newDocument();
    doc.asset.metadata = { parameters: { preset_id: "default-preset" } };
    expect(documentInput(doc, managed).preset_id).toBe("default-preset");
    doc.parameterSelection = {};
    expect(documentInput(doc, managed)).not.toHaveProperty("preset_id");
  });
  it("forwards an explicit preset decline instead of dropping it", () => {
    const doc = newDocument();
    doc.asset.metadata = { parameters: { preset_id: "default-preset" } };
    doc.parameterSelection = { preset_id: null };
    // Dropping it would let the backend fall back to the asset's saved preset.
    expect(documentInput(doc, managed).preset_id).toBeNull();
  });
  it("keeps unavailable and retired runs readable but refuses executable restoration", () => {
    for (const id of [
      "nc.generate",
      "process.generate",
      "sync-turning.generate",
      "missing.external",
    ]) {
      const history = run(id);
      expect(historyExecutionReason(history, [text, managed])).not.toBe("");
      expect(() => restoreDocument(history, [text, managed])).toThrow();
    }
    const history = run(text.id);
    history.input = {
      snapshot: { asset: { source: "old", metadata: { nc: null } } },
    };
    expect(historyExecutionReason(history, [text])).toMatch("退役");
    expect(() => restoreDocument(history, [text])).toThrow("退役");
  });
  it("restores the frozen parameter request and referenced sources without live asset resolution or history mutation", () => {
    const history = run();
    history.input = { source: "old live source", context: { x: 0 } };
    history.result = {
      artifacts: [],
      diagnostics: [],
      data: {
        replay_input: parseData(
          '{"snapshot":{"template":"main","templates":{"main":"{% include \'header\' %} {{ x }}","header":"frozen header"},"asset":{"source":"older saved source","metadata":{"parameters":{"preset_id":"live-preset","bindings":{"x":"shared-x"}}}}},"parameter_snapshot":{"version":1,"records":[]},"parameter_request":{"values":{"x":9007199254740993},"defaults":{"y":2},"schema":{"type":"object"},"bindings":{"x":"shared-x"},"preset_id":"frozen-preset"},"extensions":["math"]}',
        ),
      },
    };
    const original = encode(history);
    const doc = restoreDocument(history, [managed]);
    expect(doc.asset.source).toContain("include");
    expect(encode(doc.context)).toBe('{"x":9007199254740993}');
    expect(doc.asset.defaults).toEqual(parseData('{"y":2}'));
    expect(doc.parameterSelection).toEqual({});
    expect(doc.extensions).toEqual(["math"]);
    expect(doc.asset.metadata?.parameters).not.toHaveProperty("preset_id");
    const input = documentInput(doc, managed);
    expect(input.snapshot.templates.header).toBe("frozen header");
    expect(input.parameter_request.preset_id).toBe("frozen-preset");
    expect(encode(input.parameter_request.values)).toBe(
      '{"x":9007199254740993}',
    );
    expect(input).not.toHaveProperty("source");
    expect(input).not.toHaveProperty("context");
    doc.context.x = parseData("42");
    doc.asset.source += " edited";
    const edited = documentInput(doc, managed);
    expect(encode(edited.parameter_request.values)).toBe('{"x":42}');
    expect(edited.snapshot.templates.main).toMatch("edited");
    expect(encode(history)).toBe(original);
    expect(decodeDraft(encodeDraft(doc))).toEqual(doc);
    doc.parameterSelection = { preset_id: "new-live-preset" };
    const live = documentInput(doc, managed);
    expect(live.preset_id).toBe("new-live-preset");
    expect(live).not.toHaveProperty("parameter_snapshot");
    expect(live).not.toHaveProperty("parameter_request");
  });
  it("retains ordinary template snapshot references and original input defaults/extensions", () => {
    const history = run(text.id);
    history.input = {
      snapshot: {
        template: "root",
        templates: { root: "{{ x }}", base: "frozen" },
        asset: null,
      },
      context: { x: null },
      defaults: { y: true },
      extensions: ["math"],
    };
    const doc = restoreDocument(history, [text]);
    expect(doc.context).toEqual({ x: null });
    expect(doc.asset.defaults).toEqual({ y: true });
    const input = documentInput(doc, text);
    expect(input.snapshot.templates.base).toBe("frozen");
    expect(input.extensions).toEqual(["math"]);
  });
});
