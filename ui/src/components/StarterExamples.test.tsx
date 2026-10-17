// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { api } from "../lib/api";
import { buildStarterExample } from "../lib/starterExamples";
import { StarterExamples } from "./StarterExamples";

vi.mock("../lib/api", () => ({ api: vi.fn() }));
vi.mock("./WorkbenchUi", () => ({ Modal: ({ title, children, buttons, onClose }: any) => <section role="dialog" aria-label={title}><button aria-label="关闭对话框" onClick={onClose}>关闭</button>{children}<footer>{buttons}</footer></section> }));

afterEach(cleanup);
beforeEach(() => { vi.resetAllMocks(); vi.mocked(api).mockResolvedValue({ reports: [], valid: true }); });

describe("StarterExamples", () => {
  it("builds consistent example fields, defaults, bindings, and expected output", () => {
    const plain = buildStarterExample("plain-text");
    const plainTemplate = plain.bundle.collections.template.records[0];
    expect(plainTemplate.asset.source).toBe(plain.example.source);
    expect(plainTemplate.asset.defaults).toEqual({ user: { name: "Ada" } });
    expect(plainTemplate.asset.metadata.output_name).toBe(plain.example.outputName);
    expect(plainTemplate.asset.metadata.execution.action).toBe("template.render");
    expect(plain.example.source.replace("{{ user.name }}", plainTemplate.asset.defaults.user.name)).toBe(plain.example.expected);

    const shared = buildStarterExample("shared-parameters");
    const templateRecord = shared.bundle.collections.template.records.find((record: any) => record.group === "templates");
    const definitionRecord = shared.bundle.collections.parameters.records.find((record: any) => record.group === "definitions");
    const presetRecord = shared.bundle.collections.parameters.records.find((record: any) => record.group === "presets");
    expect(shared.bundle.collections.template.records).toHaveLength(1);
    expect(templateRecord.asset.defaults).toEqual({ report: { name: "未命名", unit: "件" } });
    expect(templateRecord.asset.metadata.parameters.bindings).toEqual(shared.example.bindings);
    expect(shared.example.bindings["/report/name"]).toBe(definitionRecord.name);
    expect(shared.example.bindings["/report/unit"]).toBeUndefined();
    expect(templateRecord.asset.metadata.parameters.preset_id).toBe(presetRecord.name);
    expect(definitionRecord.asset.default).toBe("未命名");
    expect(presetRecord.asset.values[definitionRecord.name]).toBe("零件 A");
    expect(templateRecord.asset.metadata.output_name).toBe(shared.example.outputName);
    expect(templateRecord.asset.metadata.execution.action).toBe("template.render.managed");
    expect(shared.example.source.replace("{{ report.name }}", "零件 A").replace("{{ report.unit }}", templateRecord.asset.defaults.report.unit)).toBe(shared.example.expected);
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
    fireEvent.click(screen.getByRole("button", { name: "共享参数" }));
    expect(screen.getByText("报告：{{ report.name }}（{{ report.unit }}）")).toBeTruthy();
    expect(screen.getByText(/绑定到共享定义“名称”/)).toBeTruthy();
    expect(screen.getByText(/由模板默认值提供/)).toBeTruthy();
    expect(screen.getByText(/定义默认值：/)).toBeTruthy();
    expect(screen.getByText(/共享预置“示例报告”/)).toBeTruthy();
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
  });
});
