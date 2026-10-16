// @vitest-environment jsdom
import { useState } from "react";
import { isLosslessNumber } from "lossless-json";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { action } from "../lib/api";
import { DraftNumber, encode, newDocument, numberText, parseData, type Document } from "../lib/data";
import { previewParameters } from "../lib/managedParameters";
import { SharedParameterInputs } from "./SharedParameterInputs";

vi.mock("../lib/api", () => ({ action: vi.fn() }));
vi.mock("../lib/managedParameters", async original => ({
  ...await original<typeof import("../lib/managedParameters")>(), previewParameters: vi.fn(),
}));
vi.mock("./WorkbenchUi", () => ({ JsonEditor: () => <div>JSON editor</div> }));
// Isolate the component's Form path contract; the real Form has its own tests.
vi.mock("./Form", () => ({ Form: ({ value, onFieldChange, disabled }: any) => <>
  <input aria-label="宽度" disabled={disabled} value={numberText(value.width)} onChange={event => onFieldChange("/width", new DraftNumber(event.target.value))} />
  <input aria-label="标题" value={value.title || ""} onChange={event => onFieldChange("/title", event.target.value)} />
</> }));

const preview = (values: any = parseData('{"width":25,"title":"继承标题"}')) => ({ values, origins: { "/width": "definition", "/rows/0": "definition" }, definitions: { "/width": { title: "宽度定义", unit: "mm" } } });
function document(): Document {
  const doc = newDocument();
  doc.asset.metadata = { parameters: { bindings: { "/width": "width" } } };
  doc.asset.schema = { type: "object", properties: { width: { type: "number" }, title: { type: "string" } }, required: ["width"] };
  return doc;
}
function Harness({ initial }: { initial: Document }) {
  const [doc, setDoc] = useState(initial);
  return <><SharedParameterInputs doc={doc} mode="form" onChange={patch => setDoc(previous => ({ ...previous, ...patch }))} /><output aria-label="覆盖">{JSON.stringify(doc.context, (_key, value) => value instanceof DraftNumber || isLosslessNumber(value) ? { draft: numberText(value) } : value)}</output></>;
}
afterEach(cleanup);
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(action).mockResolvedValue({ data: { items: [{ id: "shared", title: "共享预置一" }] } });
  vi.mocked(previewParameters).mockResolvedValue(preview());
});

describe("SharedParameterInputs", () => {
  it("records an equal explicit edit only at the touched field and preserves incomplete numbers", async () => {
    render(<Harness initial={document()} />);
    await waitFor(() => expect((screen.getByLabelText("宽度") as HTMLInputElement).value).toBe("25"));
    // React change events on a number-equivalent token are still explicit edits.
    fireEvent.change(screen.getByLabelText("宽度"), { target: { value: "25.0" } });
    expect(screen.getByLabelText("覆盖").textContent).toBe('{"width":{"draft":"25.0"}}');
    fireEvent.change(screen.getByLabelText("宽度"), { target: { value: "1e" } });
    expect((screen.getByLabelText("宽度") as HTMLInputElement).value).toBe("1e");
    expect(screen.getByText(/当前参数校验失败/).textContent).toContain("1e");
    expect(screen.queryByText("当前参数已通过解析校验")).toBeNull();
    expect((screen.getByLabelText("标题") as HTMLInputElement).value).toBe("继承标题");
    fireEvent.click(screen.getByText("恢复跟随"));
    expect(screen.getByLabelText("覆盖").textContent).toBe("{}");
    expect((screen.getByLabelText("宽度") as HTMLInputElement).value).toBe("25");
  });

  it("keeps missing required fields editable while surfacing the strict schema failure", async () => {
    vi.mocked(previewParameters).mockImplementation(async request => {
      if (request.schema) throw new Error("/width required");
      return preview({ title: "继承标题" });
    });
    render(<Harness initial={document()} />);
    await screen.findByText(/当前参数校验失败：\/width required/);
    fireEvent.change(screen.getByLabelText("宽度"), { target: { value: "12" } });
    expect(screen.getByLabelText("覆盖").textContent).toBe('{"width":{"draft":"12"}}');
    expect(screen.queryByText("当前参数已通过解析校验")).toBeNull();
    expect(vi.mocked(previewParameters).mock.calls.some(([request]) => !request.schema && encode(request.values) === "{}")).toBe(true);
  });

  it("drops responses from another document and never adopts its baseline", async () => {
    let finish!: (value: ReturnType<typeof preview>) => void;
    vi.mocked(previewParameters).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    const doc = document();
    const view = render(<SharedParameterInputs doc={doc} mode="form" onChange={vi.fn()} />);
    const next = document();
    vi.mocked(previewParameters).mockResolvedValue(preview({ width: 99 }));
    view.rerender(<SharedParameterInputs doc={next} mode="form" onChange={vi.fn()} />);
    await waitFor(() => expect((screen.getByLabelText("宽度") as HTMLInputElement).value).toBe("99"));
    finish(preview({ width: 1 }));
    await waitFor(() => expect((screen.getByLabelText("宽度") as HTMLInputElement).value).toBe("99"));
  });

  it("ignores a delayed success after current input becomes invalid", async () => {
    let finish!: (value: ReturnType<typeof preview>) => void;
    vi.mocked(previewParameters).mockImplementation(request => request.schema
      ? new Promise(resolve => { finish = resolve; }) : Promise.resolve(preview()));
    render(<Harness initial={document()} />);
    await waitFor(() => expect(finish).toBeTypeOf("function"));
    fireEvent.change(screen.getByLabelText("宽度"), { target: { value: "-" } });
    finish(preview({ width: 25 }));
    await waitFor(() => expect((screen.getByLabelText("宽度") as HTMLInputElement).value).toBe("-"));
    expect(screen.queryByText("当前参数已通过解析校验")).toBeNull();
    expect(screen.getByText(/当前参数校验失败/)).toBeTruthy();
  });

  it("does not reuse the previous preset baseline while a new choice is loading", async () => {
    const doc = document();
    doc.asset.defaults = { width: 4 };
    doc.parameterSelection = { preset_id: "first" };
    vi.mocked(previewParameters).mockResolvedValue(preview({ width: 25 }));
    const view = render(<SharedParameterInputs doc={doc} mode="form" onChange={vi.fn()} />);
    await waitFor(() => expect((screen.getByLabelText("宽度") as HTMLInputElement).value).toBe("25"));
    let finish!: (value: ReturnType<typeof preview>) => void;
    vi.mocked(previewParameters).mockImplementation(request => request.schema
      ? Promise.resolve(preview({ width: 99 })) : new Promise(resolve => { finish = resolve; }));
    view.rerender(<SharedParameterInputs doc={{ ...doc, parameterSelection: { preset_id: "second" } }} mode="form" onChange={vi.fn()} />);
    expect((screen.getByLabelText("宽度") as HTMLInputElement).value).toBe("4");
    finish(preview({ width: 99 }));
    await waitFor(() => expect((screen.getByLabelText("宽度") as HTMLInputElement).value).toBe("99"));
  });

  it("shows presets in JSON mode and clears all overrides with a patch", async () => {
    const doc = document();
    doc.context = parseData('{"width":9007199254740993}');
    const change = vi.fn();
    render(<SharedParameterInputs doc={doc} mode="json" onChange={change} />);
    await screen.findByText("共享预置一 (shared)");
    fireEvent.change(screen.getByLabelText("共享预置"), { target: { value: "shared" } });
    expect(change).toHaveBeenLastCalledWith({ parameterSelection: { preset_id: "shared" } });
    fireEvent.click(screen.getByText("清除全部本次覆盖"));
    expect(change).toHaveBeenLastCalledWith({ context: {}, jsonInput: undefined });
    expect(screen.getByText("JSON editor")).toBeTruthy();
  });

  it("restores a bound array index by removing the entire array layer", async () => {
    const doc = document();
    doc.asset.metadata = { parameters: { bindings: { "/rows/0": "row" } } };
    doc.context = parseData('{"rows":[2,3],"keep":9007199254740993}');
    const change = vi.fn();
    render(<SharedParameterInputs doc={doc} mode="form" onChange={change} />);
    fireEvent.click(screen.getByText("恢复整个数组跟随（/rows）"));
    expect(encode(change.mock.lastCall![0].context)).toBe('{"keep":9007199254740993}');
  });
});
