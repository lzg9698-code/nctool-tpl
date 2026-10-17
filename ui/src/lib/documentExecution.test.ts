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
// A retired machining action descriptor. It must never become selectable again.
const nc = {
  id: "nc.generate",
  ui_schema: {
    document_input: {
      match_metadata: "nc",
      bindings: {
        source: "source",
        template: "id",
        params: "effective_context",
        schema: "schema",
        defaults: "defaults",
        specs: "metadata.nc.specs",
        options: "metadata.nc.options",
        machine_id: "metadata.nc.machine_id",
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
describe("generic document execution", () => {
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
  it("refuses retired NC metadata including null on every ordinary action", () => {
    for (const value of [null, {}, { operation: { id: "groove" } }, false]) {
      const doc = newDocument();
      doc.asset.metadata = { nc: value };
      for (const requested of [undefined, text.id, managed.id]) {
        doc.generationAction = requested;
        const result = selectDocumentAction(doc, [text, managed]);
        expect(result.entry).toBeUndefined();
        expect(result.error).toMatch("已移除的加工意图");
        expect(() => documentInput(doc, text)).toThrow("已移除的加工意图");
        expect(() => documentInput(doc, managed)).toThrow("已移除的加工意图");
      }
      doc.generationAction = undefined;
      // A declared matching provider cannot make retired intent renderable.
      const result = selectDocumentAction(doc, [text, nc]);
      expect(result.entry).toBeUndefined();
      expect(result.error).toMatch("已移除的加工意图");
      // Raw metadata stays intact.
      expect(doc.asset.metadata.nc).toBe(value);
    }
  });
  it("filters retired machining actions and refuses explicit machining intent", () => {
    // A retired action in capabilities never becomes a selectable choice.
    expect(documentActions([text, report, nc, { id: "machine.list" }])).toEqual([
      text,
      report,
    ]);
    const doc = newDocument();
    doc.asset.metadata = { nc: {}, parameters: { bindings: { x: "shared-x" } } };
    expect(selectDocumentAction(doc, [text, managed, nc]).entry).toBeUndefined();
    expect(selectDocumentAction(doc, [text, managed, nc]).error).toMatch(
      "已移除的加工意图",
    );
    for (const id of [
      nc.id,
      "process.generate.managed",
      "sync-turning.generate",
      "undercut.generate",
    ]) {
      const provider = { ...nc, id };
      doc.asset.metadata = { execution: { action: id } };
      expect(selectDocumentAction(doc, [text, provider]).entry).toBeUndefined();
      expect(selectDocumentAction(doc, [text, provider]).error).toMatch(
        "已移除的加工意图",
      );
      doc.generationAction = id;
      expect(selectDocumentAction(doc, [text]).error).toMatch(
        "已移除的加工意图",
      );
      delete doc.asset.metadata.execution;
      doc.generationAction = undefined;
    }
  });
  it("keeps only generic contributed actions and panels", () => {
    const panels = [
      { id: "templates", view: "template-editor", actions: [text.id] },
      { id: "tools", view: "nc.parameters", actions: [nc.id] },
      { id: "process", view: "nc.process", actions: ["process.generate"] },
      { id: "parameters", view: "parameter.library", actions: [] },
      { id: "report", view: "report.form", actions: [report.id, nc.id] },
    ];
    expect(documentActions([text, report, nc, { id: "machine.list" }])).toEqual([
      text,
      report,
    ]);
    expect(activePanels(panels, [text, report])).toEqual([
      { id: "parameters", view: "parameter.library", actions: [] },
      { id: "report", view: "report.form", actions: [report.id] },
    ]);
    // Even an explicitly provided retired action never re-enables its panel.
    expect(activePanels(panels, [text, report, nc])).toEqual([
      { id: "parameters", view: "parameter.library", actions: [] },
      { id: "report", view: "report.form", actions: [report.id] },
    ]);
  });
  it("refuses to build a machining input even when bindings and values are exact", () => {
    const doc = newDocument();
    doc.id = "nc-template";
    doc.asset.defaults = { depth: 2 };
    doc.asset.metadata = { nc: { specs: { x: { type: "integer" } } } };
    doc.context = parseData('{"x":9007199254740993}');
    const original = encode(doc);
    expect(() => documentInput(doc, nc)).toThrow("已移除的加工意图");
    expect(encode(doc)).toBe(original);
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
  it("inherits an asset default when the selection has no preset choice", () => {
    const doc = newDocument();
    doc.asset.metadata = { parameters: { preset_id: "default-preset" } };
    expect(documentInput(doc, managed).preset_id).toBe("default-preset");
    doc.parameterSelection = {};
    expect(documentInput(doc, managed).preset_id).toBe("default-preset");
  });
  it("forwards an explicit preset decline instead of dropping it", () => {
    const doc = newDocument();
    doc.asset.metadata = { parameters: { preset_id: "default-preset" } };
    doc.parameterSelection = { preset_id: null };
    // Dropping it would let the backend fall back to the asset's saved preset.
    expect(documentInput(doc, managed).preset_id).toBeNull();
  });
  it("keeps unavailable and retired runs readable but refuses execution and restoration", () => {
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
  });
  it("keeps retired machining history read-only even when its action is available", () => {
    for (const id of [
      nc.id,
      "process.generate",
      "sync-turning.generate",
      "undercut.generate",
    ]) {
      const history = run(id);
      history.input = {
        snapshot: { asset: { source: "old", metadata: { nc: {} } } },
      };
      const provider = { ...nc, id };
      expect(historyExecutionReason(history, [text, provider])).toMatch(
        "已移除的加工意图",
      );
      expect(() => restoreDocument(history, [text, provider])).toThrow();
    }
  });
  it("blocks ordinary history rendering of retired NC snapshots in either saved input", () => {
    for (const provider of [text, managed]) {
      for (const useReplay of [false, true]) {
        const history = run(provider.id);
        const input = { snapshot: { asset: { source: "old", metadata: { nc: null } } } };
        history.input = useReplay ? {} : input;
        history.result = {
          artifacts: [],
          diagnostics: [],
          data: { replay_input: useReplay ? input : {} },
        };
        expect(historyExecutionReason(history, [provider, nc])).toMatch(
          "已移除的加工意图",
        );
        expect(() => restoreDocument(history, [provider, nc])).toThrow(
          "已移除的加工意图",
        );
      }
    }
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
