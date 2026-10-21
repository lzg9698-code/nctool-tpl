import { describe, expect, it } from "vitest";
import { DraftNumber, newDocument } from "./data";
import { documentInput } from "./documentExecution";
import {
  initialDocumentContext,
  resetDocumentParameters,
  sharedParameterDocument,
  documentForGeneration,
  templatePresetParameters,
  loadTemplatePresetParameters,
} from "./appDocumentParameters";

const asset = {
  source: "{{ width }}",
  schema: {
    type: "object",
    properties: { width: { type: "number", default: 10 } },
  },
  defaults: { width: 20 },
  metadata: { parameters: { selection: { preset_id: "shared" } } },
};
describe("App shared parameter lifecycle", () => {
  it("starts managed open/import with overrides empty and retains ordinary defaults", () => {
    expect(initialDocumentContext(asset, "template.render.managed")).toEqual(
      {},
    );
    expect(String(initialDocumentContext(asset, "template.render").width)).toBe(
      "20",
    );
    expect(
      initialDocumentContext({ ...asset, metadata: {} }, "template.render"),
    ).toEqual({ width: 20 });
    expect(
      initialDocumentContext(
        { ...asset, metadata: { nc: {} } },
        "template.render.managed",
      ),
    ).toEqual({ width: 20 });
    expect(sharedParameterDocument(asset, "template.render.managed")).toBe(
      true,
    );
    expect(
      sharedParameterDocument(
        { ...asset, metadata: { nc: {} } },
        "template.render.managed",
      ),
    ).toBe(false);
  });
  it("clears overrides explicitly without guessing or mutating old drafts", () => {
    const doc = {
      ...newDocument(),
      asset,
      context: { width: 20 },
      parameterSelection: { preset_id: null },
      jsonInput: "bad",
    };
    expect(resetDocumentParameters(doc, "template.render.managed")).toEqual({
      context: {},
      jsonInput: undefined,
    });
    expect(doc.context).toEqual({ width: 20 });
    expect(doc.parameterSelection).toEqual({ preset_id: null });
    expect(resetDocumentParameters(doc, "template.render").context).toEqual({
      width: 20,
    });
  });
  it("executes the latest raw JSON and blocks incomplete raw/form drafts", () => {
    const doc = {
      ...newDocument(),
      context: { width: 5 },
      jsonInput: '{"width":9}',
    };
    expect(String(documentForGeneration(doc).context.width)).toBe("9");
    expect(() =>
      documentForGeneration({ ...doc, jsonInput: '{"width":' }),
    ).toThrow();
    expect(() =>
      documentForGeneration({
        ...doc,
        jsonInput: undefined,
        context: { width: new DraftNumber("-") },
      }),
    ).toThrow();
  });
  it("preserves shared selection and explicit inherited-preset decline in presets and generation", () => {
    const doc = {
      ...newDocument(),
      asset,
      parameterSelection: { preset_id: null },
      context: { custom: null },
    };
    const saved = templatePresetParameters(doc);
    expect(saved).toEqual({
      context: { custom: null },
      parameter_selection: { preset_id: null },
    });
    expect(loadTemplatePresetParameters(saved)).toEqual({
      context: { custom: null },
      parameterSelection: { preset_id: null },
      jsonInput: undefined,
    });
    expect(loadTemplatePresetParameters({ context: { legacy: true } })).toEqual(
      { context: { legacy: true }, jsonInput: undefined },
    );
    const input = documentInput(doc, {
      id: "template.render.managed",
      ui_schema: {
        document_input: {
          bindings: { context: "context", preset_id: "parameter_preset_id" },
        },
      },
    });
    expect(input.preset_id).toBeNull();
    expect(input.context.custom).toBeNull();
  });
});
