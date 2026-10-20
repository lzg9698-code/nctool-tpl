import { describe, expect, it } from "vitest";
import { DraftNumber, newDocument } from "./data";
import {
  documentForGeneration,
  initialDocumentContext,
  removeDocumentParameter,
  resetDocumentParameters,
} from "./appDocumentParameters";

const asset = {
  source: "{{ width }}",
  schema: {
    type: "object",
    properties: { width: { type: "number" } },
  },
  defaults: { width: 20, label: "" },
};

describe("template-local parameters", () => {
  it("starts a document from the template's own defaults", () => {
    expect(initialDocumentContext(asset)).toEqual({ width: 20, label: "" });
    expect(resetDocumentParameters({ ...newDocument(), asset }).context).toEqual({
      width: 20,
      label: "",
    });
  });
  it("removes a field from defaults and this run's input together", () => {
    const doc = { ...newDocument(), asset, context: { width: 8, keep: true } };
    const patch = removeDocumentParameter(doc, "width");
    expect(patch.asset?.defaults).toEqual({ label: "" });
    expect(patch.context).toEqual({ keep: true });
    expect(patch.jsonInput).toBeUndefined();
    // The source document is not mutated.
    expect(doc.asset.defaults).toEqual({ width: 20, label: "" });
    expect(doc.context).toEqual({ width: 8, keep: true });
  });
  it("executes the latest raw JSON and blocks incomplete drafts", () => {
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
});
