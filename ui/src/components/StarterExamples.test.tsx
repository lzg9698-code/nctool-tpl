// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { api } from "../lib/api";
import { buildStarterExample, type StarterExampleId } from "../lib/starterExamples";
import { Form } from "./Form";
import { StarterExamples } from "./StarterExamples";

vi.mock("../lib/api", () => ({ api: vi.fn() }));
vi.mock("./WorkbenchUi", () => ({ Modal: ({ title, children, buttons, onClose }: any) => <section role="dialog" aria-label={title}><button aria-label="关闭对话框" onClick={onClose}>关闭</button>{children}<footer>{buttons}</footer></section> }));

afterEach(cleanup);
beforeEach(() => { vi.resetAllMocks(); vi.mocked(api).mockResolvedValue({ reports: [], valid: true }); });

describe("StarterExamples", () => {
  it.each([
    { id: "plain-text", fields: { 姓名: "Ada" } },
    { id: "parameter-report", fields: { 名称: "零件 A", 单位: "件" } },
  ] as { id: StarterExampleId; fields: Record<string, string> }[])("provides editable local parameter fields for $id", ({ id, fields }) => {
    const { example } = buildStarterExample(id);
    const onChange = vi.fn();
    render(<Form schema={example.schema} value={example.defaults} onChange={onChange} />);
    for (const [label, value] of Object.entries(fields)) {
      expect((screen.getByRole("textbox", { name: new RegExp(label) }) as HTMLInputElement).value).toBe(value);
    }
    fireEvent.change(screen.getByRole("textbox", { name: new RegExp(Object.keys(fields)[0]) }), { target: { value: "修改后的名称" } });
    expect(onChange).toHaveBeenCalledOnce();
    expect(api).not.toHaveBeenCalled();
  });

  it("previews without writes and imports only after explicit confirmation", async () => {
    render(<StarterExamples onSaved={vi.fn().mockResolvedValue(undefined)} />);
    fireEvent.click(screen.getByRole("button", { name: "查看入门示例" }));
    expect(screen.getByText("你好，{{ user.name }}！")).toBeTruthy();
    expect(screen.getByText("你好，Ada！")).toBeTruthy();
    expect(api).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "校验示例" }));
    await screen.findByText(/示例校验通过/);
    expect(api).toHaveBeenCalledTimes(1);
    expect(api).toHaveBeenCalledWith("bundle/validate", expect.any(Object));
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(api).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "确认保存整个示例" }));
    await waitFor(() => expect(api).toHaveBeenCalledTimes(2));
    expect(vi.mocked(api).mock.calls[1][0]).toBe("bundle/import");
  });

  it("prevents duplicate validation and save requests while busy", async () => {
    let resolve!: (value: any) => void;
    vi.mocked(api).mockImplementationOnce(() => new Promise(done => { resolve = done; }));
    render(<StarterExamples onSaved={vi.fn().mockResolvedValue(undefined)} />);
    fireEvent.click(screen.getByRole("button", { name: "查看入门示例" }));
    const validate = screen.getByRole("button", { name: "校验示例" });
    fireEvent.click(validate);
    fireEvent.click(validate);
    expect(api).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "关闭对话框" }));
    expect(screen.getByRole("dialog")).toBeTruthy();
    resolve({ valid: true, reports: [] });
    await screen.findByText(/示例校验通过/);
    fireEvent.click(screen.getByRole("button", { name: "确认保存整个示例" }));
    expect(api).toHaveBeenCalledTimes(2);
  });

  it("retains the preview after validation/import failures and permits retry", async () => {
    vi.mocked(api)
      .mockRejectedValueOnce(new Error("校验失败"))
      .mockResolvedValueOnce({ valid: true, reports: [] })
      .mockRejectedValueOnce(new Error("标识冲突"))
      .mockResolvedValueOnce({ imported: true, reports: [] });
    const onSaved = vi.fn().mockResolvedValue(undefined);
    render(<StarterExamples onSaved={onSaved} />);
    fireEvent.click(screen.getByRole("button", { name: "查看入门示例" }));
    fireEvent.click(screen.getByRole("button", { name: "参数报告" }));
    expect(screen.getByRole("dialog", { name: "入门示例：参数报告" })).toBeTruthy();
    expect(screen.getByText("报告：{{ report.name }}（{{ report.unit }}）")).toBeTruthy();
    expect(screen.getByText("报告：零件 A（件）")).toBeTruthy();
    expect(screen.getByText("保存后可在参数表单修改本地参数；默认值如下。")).toBeTruthy();
    expect(screen.getByText("template.render")).toBeTruthy();
    expect(screen.queryByText(/共享/)).toBeNull();
    expect(api).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "校验示例" }));
    await screen.findByRole("alert");
    expect(screen.getByRole("dialog")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "校验示例" }));
    await screen.findByText(/示例校验通过/);
    fireEvent.click(screen.getByRole("button", { name: "确认保存整个示例" }));
    await screen.findByText(/预览已保留/);
    expect(screen.getByRole("dialog")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "确认保存整个示例" }));
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(vi.mocked(api).mock.calls.map(([path]) => path)).toEqual([
      "bundle/validate", "bundle/validate", "bundle/import", "bundle/import",
    ]);
    const bundles = vi.mocked(api).mock.calls.map(([, input]) => (input as any).bundle);
    for (const bundle of bundles) {
      expect(bundle).toBe(bundles[0]);
      expect(Object.keys(bundle.collections)).toEqual(["template"]);
      expect(bundle.collections.template.records[0].asset.metadata).not.toHaveProperty("parameters");
    }
  });
});
