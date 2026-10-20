import { describe, expect, it } from "vitest";
import { newDocument, type Asset } from "./data";
import {
  cloneDocumentForCopy,
  mergeInspectedParameters,
  normalizeSchema,
  prepareDocument,
} from "./documentModel";

const asset: Asset = {
  source: "{{ a }}",
  schema: {
    type: "object",
    properties: { a: { type: "number", title: "甲" } },
    required: [],
  },
  defaults: { a: 1 },
  metadata: { title: "甲模板", output_name: "a.txt" },
};

describe("shared document model", () => {
  it("normalizes a missing schema and keeps an existing one by reference", () => {
    expect(normalizeSchema({ source: "x" })).toEqual({
      type: "object",
      properties: {},
      required: [],
    });
    const schema = { type: "object", properties: {} };
    expect(normalizeSchema({ source: "x", schema })).toBe(schema);
  });

  it("starts every document from the template's own defaults", () => {
    expect(prepareDocument("tpl-a", asset, "fp").context).toEqual({ a: 1 });
    expect(prepareDocument("tpl-c", asset, "fp").saved).toBeTruthy();
  });

  it("drops the run binding when a document is duplicated", () => {
    const doc = newDocument();
    doc.lastRunId = "run-1";
    doc.lastRunSignature = "sig";
    const copy = cloneDocumentForCopy(doc);
    expect(copy.key).not.toBe(doc.key);
    expect(copy.id).toBeNull();
    expect(copy.fingerprint).toBeNull();
    expect(copy.title).toBe(doc.title + " 副本");
    expect(copy.saved).toBe("");
    expect(copy.lastRunId).toBeUndefined();
    expect(copy.lastRunSignature).toBeUndefined();
  });

  it("merges inspected variables without dropping existing definitions", () => {
    const current: Asset = {
      source: "x",
      schema: {
        type: "object",
        properties: { a: { type: "string" } },
        required: ["a"],
      },
      defaults: {},
      metadata: {},
    };
    const merged = mergeInspectedParameters(current, [
      { name: "a" },
      { name: "b" },
      { name: "c", optional: true },
    ]);
    expect(merged.added).toBe(2);
    expect(merged.changed).toBe(true);
    expect(merged.asset.schema?.properties?.a).toEqual({ type: "string" });
    expect(merged.asset.schema?.properties?.b).toEqual({
      title: "b",
      "x-untyped": true,
    });
    expect(merged.asset.schema?.required).toEqual(["a", "b"]);
    // The input asset must never be mutated in place.
    expect(current.schema?.properties?.b).toBeUndefined();
  });

  it("reports no change when the source references the same variables again", () => {
    const current: Asset = {
      source: "x",
      schema: {
        type: "object",
        properties: { a: { title: "a", "x-untyped": true } },
        required: ["a"],
      },
      defaults: {},
      metadata: {},
    };
    const merged = mergeInspectedParameters(current, [{ name: "a" }]);
    expect(merged.added).toBe(0);
    expect(merged.changed).toBe(false);
  });
});
