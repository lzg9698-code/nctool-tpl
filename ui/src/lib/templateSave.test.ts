import { describe, expect, it } from "vitest";
import { newDocument, documentValue } from "./data";
import { applyTemplateSaveResult, templateSaveAsset } from "./templateSave";

describe("template save merge", () => {
  it("advances the fingerprint without replacing edits made during the request", () => {
    const submitted = newDocument();
    submitted.title = "Submitted title";
    submitted.asset.source = "submitted source";
    const submittedAsset = templateSaveAsset(submitted, "template.render");
    const editedDuringSave = {
      ...submitted,
      title: "Edited while saving",
      asset: { ...submitted.asset, source: "late source" },
    };

    const result = applyTemplateSaveResult(
      editedDuringSave,
      submitted,
      "tpl-1",
      submittedAsset,
      "fingerprint-after-save",
    );

    expect(result.id).toBe("tpl-1");
    expect(result.fingerprint).toBe("fingerprint-after-save");
    expect(result.title).toBe("Edited while saving");
    expect(result.asset.source).toBe("late source");
    expect(result.saved).toBe(documentValue({
      ...submitted,
      id: "tpl-1",
      asset: submittedAsset,
      fingerprint: "fingerprint-after-save",
    }));
    expect(result.saved).not.toBe(documentValue(result));
  });

  it("marks the submitted version clean when no later edits exist", () => {
    const submitted = newDocument();
    submitted.asset.source = "same source";
    const asset = templateSaveAsset(submitted);
    const result = applyTemplateSaveResult(submitted, submitted, "tpl-2", asset, "fp");
    expect(result.asset).toBe(asset);
    expect(result.saved).toBe(documentValue(result));
  });
});
