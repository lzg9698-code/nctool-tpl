import { describe, expect, it } from "vitest";
import { validPluginInput, signature } from "./execution";
import { DraftNumber } from "./data";
import { encodeDraft, decodeDraft } from "./drafts";
describe("plugin raw drafts and submitted input", () => {
  const schema = {
    type: "object",
    properties: { values: { type: "array" } },
    required: ["values"],
  };
  it("never substitutes the last valid value for invalid or wrong-top-level JSON", () => {
    const previous = { values: [10, 20] };
    for (const raw of ["{", "[]", "null", "1", '{"values":[-]}'])
      expect(() => validPluginInput(schema, previous, raw)).toThrow();
    expect(
      validPluginInput(
        schema,
        previous,
        '{"values":[999]}',
      ).values[0].toString(),
    ).toBe("999");
    expect(() =>
      validPluginInput({ type: "object" }, { x: new DraftNumber("-") }),
    ).toThrow();
  });
  it("tracks changed actions, forms and raw drafts and restores invalid text across navigation", () => {
    const original = signature("report.compute", { values: [10] });
    expect(signature("report.compute", { values: [11] })).not.toBe(original);
    expect(signature("report.other", { values: [10] })).not.toBe(original);
    const draft = {
      selected: "report.compute",
      value: { values: [10] },
      raw: "{",
      lastSignature: original,
    };
    const restored = decodeDraft<typeof draft>(encodeDraft(draft));
    expect(restored.raw).toBe("{");
    expect(signature(restored.selected, restored.value, restored.raw)).not.toBe(
      original,
    );
    expect(() =>
      validPluginInput(schema, restored.value, restored.raw),
    ).toThrow();
  });
});
