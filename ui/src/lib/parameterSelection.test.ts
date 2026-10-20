import { describe, expect, it } from "vitest";
import { newDocument } from "./data";
import { documentActions } from "./documentExecution";

describe("template-local generation choices", () => {
  it("does not offer the retired shared render action as a document action", () => {
    const entry = (id: string) => ({
      id,
      ui_schema: { document_input: { bindings: { source: "source" } } },
    });
    expect(
      documentActions([entry("template.render"), entry("template.render.managed")]).map(
        (item) => item.id,
      ),
    ).toEqual(["template.render"]);
  });
  it("new documents carry no shared parameter selection", () => {
    expect(newDocument()).not.toHaveProperty("parameterSelection");
    expect(newDocument()).not.toHaveProperty("replayInput");
  });
});
