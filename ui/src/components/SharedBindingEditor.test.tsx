// @vitest-environment jsdom
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { action } from "../lib/api";
import { newDocument, type Asset } from "../lib/data";
import { previewParameters } from "../lib/managedParameters";
import { SharedBindingEditor } from "./SharedBindingEditor";

vi.mock("../lib/api", () => ({ action: vi.fn() }));
vi.mock("../lib/managedParameters", async (original) => ({
  ...await original<typeof import("../lib/managedParameters")>(),
  previewParameters: vi.fn(),
}));

const definitions = [
  { id: "width", title: "宽度", asset: { schema: { type: "number" }, unit: "mm", description: "零件宽度", default: 25 } },
  { id: "height", title: "高度", asset: { schema: { type: "integer" }, default: 8 } },
];
const presets = [{ id: "rough", title: "粗加工" }];
function asset(): Asset {
  return {
    source: "{{ width }}",
    defaults: { width: 20 },
    schema: { type: "object", properties: { width: { type: "number" } } },
    metadata: {
      note: "keep",
      extension: { flag: true },
      parameters: { bindings: { "/width": "width" }, extension: "keep", selection: { preset_id: null, extra: 7 } },
    },
  };
}
function Harness({ initial }: { initial: Asset }) {
  const [value, setValue] = useState(initial);
  return <><SharedBindingEditor asset={value} onChange={setValue} /><output aria-label="资产快照">{JSON.stringify(value)}</output></>;
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(action).mockImplementation(async (_id, input: any) => ({ data: { items: input.group === "definitions" ? definitions : presets } } as any));
  vi.mocked(previewParameters).mockResolvedValue({ values: { width: 20 }, origins: { "/width": "template_default" }, definitions: { "/width": definitions[0].asset } });
});
afterEach(cleanup);

describe("SharedBindingEditor", () => {
  it("preserves metadata and parameter extension fields while adding a saved binding", async () => {
    render(<Harness initial={asset()} />);
    await screen.findByLabelText("共享定义 /width");
    fireEvent.change(screen.getByLabelText("新增绑定 JSON Pointer"), { target: { value: "/height" } });
    expect(screen.getByText("尚未添加")).toBeTruthy();
    expect(JSON.parse(screen.getByLabelText("资产快照").textContent!).metadata.parameters.bindings).toEqual({ "/width": "width" });
    fireEvent.change(screen.getByLabelText("新增绑定共享定义"), { target: { value: "height" } });
    fireEvent.click(screen.getByText("添加绑定"));
    await waitFor(() => expect(JSON.parse(screen.getByLabelText("资产快照").textContent!).metadata.parameters.bindings["/height"]).toBe("height"));
    const saved = JSON.parse(screen.getByLabelText("资产快照").textContent!);
    expect(saved.metadata).toMatchObject({ note: "keep", extension: { flag: true } });
    expect(saved.metadata.parameters).toMatchObject({ extension: "keep", selection: { preset_id: null, extra: 7 } });
    // The summary is now a definition list, but it must still expose the same
    // facts about the bound definition, scoped to the /width card.
    const card = screen.getByLabelText("共享定义 /width").closest(".binding-card") as HTMLElement;
    expect(card).toBeTruthy();
    const summary = card.querySelector(".binding-summary") as HTMLElement;
    expect(summary).toBeTruthy();
    expect(summary.textContent).toContain("number");
    expect(summary.textContent).toContain("mm");
    expect(summary.textContent).toContain("零件宽度");
    expect(summary.textContent).toContain("25");
  });

  it("does not print the identifier twice when the catalog falls back to it as the title", async () => {
    // The parameters catalog serves the asset id as the title when the asset has
    // no title, so a naive `title (id)` label would show the same string twice.
    const id = "example-name-k3f9qz";
    vi.mocked(action).mockImplementation(async (_id, input: any) => ({
      data: { items: input.group === "definitions"
        ? [{ id, title: id, asset: { schema: { type: "string" } } }]
        : [] },
    } as any));
    const initial = asset();
    initial.metadata!.parameters.bindings = { "/width": id };
    render(<Harness initial={initial} />);
    const definition = await screen.findByLabelText("共享定义 /width");
    const labels = [...(definition as HTMLSelectElement).options].map((option) => option.textContent);
    expect(labels).toContain(id);
    expect(labels.some((label) => label === `${id} (${id})`)).toBe(false);
  });

  it("persists the template default preset in the field read by managed rendering", async () => {
    render(<Harness initial={asset()} />);
    await screen.findByLabelText("模板默认共享预置");
    fireEvent.change(screen.getByLabelText("模板默认共享预置"), { target: { value: "rough" } });
    await waitFor(() => {
      const saved = JSON.parse(screen.getByLabelText("资产快照").textContent!);
      expect(saved.metadata.parameters.preset_id).toBe("rough");
    });
  });

  it("retains unknown current definitions and presets, and explicit preset null", async () => {
    const initial = asset();
    initial.metadata!.parameters.bindings = { "/width": "removed-definition" };
    initial.metadata!.parameters.selection.preset_id = "removed-preset";
    delete initial.metadata!.parameters.preset_id;
    render(<Harness initial={initial} />);
    expect(await screen.findByText("removed-definition（未知定义，保留当前值）")).toBeTruthy();
    expect(screen.getByText("removed-preset（当前选择，目录中未知）")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("模板默认共享预置"), { target: { value: "" } });
    await waitFor(() => expect(JSON.parse(screen.getByLabelText("资产快照").textContent!).metadata.parameters.preset_id).toBeNull());
  });

  it("reports catalog failure and keeps editing unavailable where needed", async () => {
    vi.mocked(action).mockRejectedValue(new Error("目录离线"));
    render(<Harness initial={asset()} />);
    expect(await screen.findByText("参数目录读取失败：目录离线")).toBeTruthy();
    expect((screen.getByLabelText("新增绑定共享定义") as HTMLSelectElement).options).toHaveLength(1);
  });

  it("reports invalid and overlapping JSON Pointers before adding", async () => {
    render(<Harness initial={asset()} />);
    await screen.findByLabelText("共享定义 /width");
    const pointer = screen.getByLabelText("新增绑定 JSON Pointer");
    fireEvent.change(pointer, { target: { value: "/width/child" } });
    expect(screen.getByText("这个 JSON Pointer 与现有绑定路径重叠。")).toBeTruthy();
    fireEvent.change(pointer, { target: { value: "/bad~2token" } });
    expect(screen.getByText("JSON Pointer token 不能为空，且 ~ 只能写作 ~0 或 ~1。")).toBeTruthy();
    fireEvent.change(pointer, { target: { value: "/constructor/x" } });
    expect(screen.getByText("此 JSON Pointer 包含禁止的路径 token。")).toBeTruthy();
  });

  it("does not show a late check result after asset edits", async () => {
    let finish!: (result: any) => void;
    vi.mocked(previewParameters).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    const initial = asset();
    const onChange = vi.fn();
    function Controlled() {
      const [value, setValue] = useState(initial);
      return <><SharedBindingEditor asset={value} onChange={(next) => { onChange(next); setValue(next); }} /><button onClick={() => setValue({ ...value, defaults: { width: 21 } })}>编辑资产</button></>;
    }
    render(<Controlled />);
    await screen.findByLabelText("共享定义 /width");
    fireEvent.click(screen.getByText("检查共享绑定"));
    await waitFor(() => expect(finish).toBeTypeOf("function"));
    fireEvent.click(screen.getByText("编辑资产"));
    finish({ values: { width: 20 }, origins: {}, definitions: {} });
    await waitFor(() => expect(screen.queryByLabelText("共享绑定检查结果")).toBeNull());
  });

  it("uses managed resolve request and surfaces backend validation errors", async () => {
    vi.mocked(previewParameters).mockRejectedValue(new Error("绑定路径重叠"));
    render(<Harness initial={asset()} />);
    await screen.findByLabelText("共享定义 /width");
    fireEvent.click(screen.getByText("检查共享绑定"));
    expect(await screen.findByText("共享绑定检查失败：绑定路径重叠")).toBeTruthy();
    const [request] = vi.mocked(previewParameters).mock.calls[0];
    expect(request).toMatchObject({ values: {}, defaults: { width: 20 }, schema: asset().schema, bindings: { "/width": "width" }, preset_id: null });
  });

  it("enables managed bindings and explicitly describes safe disable behavior", async () => {
    const initial = newDocument().asset;
    initial.metadata = { other: "preserve", execution: { action: "template.render", extra: true } };
    render(<Harness initial={initial} />);
    fireEvent.click(screen.getByText("启用共享参数绑定"));
    const saved = JSON.parse(screen.getByLabelText("资产快照").textContent!);
    expect(saved.metadata).toMatchObject({ other: "preserve", execution: { action: "template.render.managed", extra: true }, parameters: { bindings: {} } });
    fireEvent.click(screen.getByText("确认移除共享参数绑定"));
    const disabled = JSON.parse(screen.getByLabelText("资产快照").textContent!);
    expect(disabled.metadata).toMatchObject({ other: "preserve", execution: { action: "template.render", extra: true } });
    expect(disabled.metadata).not.toHaveProperty("parameters");
  });

  it("does not offer enable or disable for retired intent assets", () => {
    const initial = newDocument().asset;
    initial.metadata = { nc: {}, execution: { action: "nc.generate" } };
    const { container } = render(<Harness initial={initial} />);
    expect(container.querySelector("button")).toBeNull();
  });

  it("preserves object binding extension fields when its definition changes", async () => {
    const initial = asset();
    initial.metadata!.parameters.bindings["/width"] = { id: "width", unit: "mm", custom: "keep" };
    render(<Harness initial={initial} />);
    await screen.findByLabelText("共享定义 /width");
    fireEvent.change(screen.getByLabelText("共享定义 /width"), { target: { value: "height" } });
    await waitFor(() => expect(JSON.parse(screen.getByLabelText("资产快照").textContent!).metadata.parameters.bindings["/width"]).toEqual({ id: "height", custom: "keep" }));
  });

  it("surfaces malformed parameter shapes without rewriting them", () => {
    const initial = asset();
    initial.metadata!.parameters.bindings = "bad";
    render(<Harness initial={initial} />);
    expect(screen.getByRole("alert").textContent).toContain("metadata.parameters.bindings 必须是对象");
    expect(JSON.parse(screen.getByLabelText("资产快照").textContent!).metadata.parameters.bindings).toBe("bad");
  });
});
