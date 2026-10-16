// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { newDocument, type Data } from "../../lib/data";
import { previewParameters } from "../../lib/managedParameters";
import { ParametersPane } from "./ParametersPane";

vi.mock("../../lib/managedParameters", async (original) => ({
  ...await original<typeof import("../../lib/managedParameters")>(),
  previewParameters: vi.fn(),
}));
vi.mock("../../components/Form", () => ({
  Form: ({ value, onChange }: Data) => <button onClick={() => onChange({ ...value, width: 30 })}>编辑宽度</button>,
}));
vi.mock("../../plugins/ParameterViews", () => ({ ParameterSelection: () => null }));
vi.mock("../../components/WorkbenchUi", () => ({ Empty: () => null, JsonEditor: () => null }));
vi.mock("./useWorkbenchController", () => ({ normalizeSchema: (asset: Data) => asset.schema }));
afterEach(cleanup);

describe("shared parameter pane", () => {
  it("keeps hook order across document transitions and edits only the changed field", async () => {
    vi.mocked(previewParameters).mockResolvedValue({
      values: { width: 25, inherited: "preset" }, origins: {}, definitions: {},
    });
    const doc = newDocument();
    doc.asset.schema = { type: "object", properties: { width: {}, inherited: {} } };
    const editDoc = vi.fn();
    const controller: Data = {
      errors: {}, paramMode: "form", doc: undefined,
      generation: { id: "template.render.managed" }, editDoc,
    };
    const view = render(<ParametersPane controller={controller} />);
    view.rerender(<ParametersPane controller={{ ...controller, doc }} />);
    await waitFor(() => expect(previewParameters).toHaveBeenCalled());
    await waitFor(() => {
      fireEvent.click(screen.getByText("编辑宽度"));
      expect(editDoc).toHaveBeenLastCalledWith({ context: { width: 30 }, jsonInput: undefined });
    });
    view.rerender(<ParametersPane controller={controller} />);
    expect(screen.queryByText("编辑宽度")).toBeNull();
  });
});
