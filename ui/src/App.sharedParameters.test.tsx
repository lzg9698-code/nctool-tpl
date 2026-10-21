// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { newDocument, type Data, type Document } from "./lib/data";
import App from "./App";
import { api, action } from "./lib/api";
import { loadDrafts } from "./lib/drafts";
import { loadTemplateCatalog } from "./lib/templateCatalog";

vi.mock("./lib/api", () => ({
  api: vi.fn(),
  action: vi.fn(),
  ApiError: class extends Error {},
}));
vi.mock("./lib/drafts", () => ({
  loadDrafts: vi.fn(),
  saveDrafts: () => true,
  cloneDraft: (doc: Document) => doc,
}));
vi.mock("./lib/templateCatalog", async (original) => ({
  ...(await original<typeof import("./lib/templateCatalog")>()),
  loadTemplateCatalog: vi.fn(),
}));
vi.mock("./lib/managedDependencies", () => ({
  useManagedDependencies: () => ({ stale: false }),
}));
vi.mock("./plugins/registry", () => ({ builtinViews: {} }));
vi.mock("./components/LazyEditor", () => ({ Editor: () => <div>源码编辑器</div> }));
vi.mock("./components/SchemaEditor", () => ({
  SchemaEditor: () => <div>参数定义编辑器</div>,
}));
vi.mock("./components/Form", () => ({ Form: () => <div>原参数表单</div> }));
vi.mock("./plugins/ParameterViews", () => ({
  ParameterSelection: () => <div>原共享选择器</div>,
}));
vi.mock("./components/SharedParameterInputs", () => ({
  SharedParameterInputs: ({ doc, onChange, onJsonError }: Data) => (
    <div>
      <span>共享参数输入</span>
      <output data-testid="shared-context">
        {JSON.stringify(doc.context)}
      </output>
      <button
        onClick={() => {
          onChange({ jsonInput: '{"width":' });
          onJsonError("JSON 未完成");
        }}
      >
        输入错误 JSON
      </button>
      <button
        onClick={() => {
          onChange({ jsonInput: '{"width":9}', context: { width: 3 } });
          onJsonError("");
        }}
      >
        输入最新 JSON
      </button>
    </div>
  ),
}));
vi.mock("./components/WorkbenchUi", () => ({
  Empty: ({ title }: Data) => <div>{title}</div>,
  PageTitle: ({ title, children }: Data) => (
    <div>
      {title}
      {children}
    </div>
  ),
  Menu: ({ children }: Data) => <div>{children}</div>,
  Modal: ({ title, children, buttons }: Data) => (
    <div>
      {title}
      {children}
      {buttons}
    </div>
  ),
  JsonEditor: () => <div>原 JSON 编辑器</div>,
}));
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  localStorage.clear();
});

function setup(
  id = "template.render.managed",
  draft = true,
  metadata: Data = {},
) {
  const doc = newDocument();
  doc.mode = "use";
  doc.asset = {
    source: "{{ width }}",
    schema: { type: "object", properties: {} },
    defaults: { width: 20 },
    metadata,
  };
  doc.context = { legacy: "kept" };
  const generation = {
    id,
    ui_schema: {
      document_input: {
        default: true,
        bindings: { source: "source", context: "context" },
      },
    },
  };
  vi.mocked(loadDrafts).mockReturnValue({
    documents: draft ? [doc] : [],
    active: draft ? doc.key : null,
  });
  vi.mocked(loadTemplateCatalog).mockResolvedValue({
    items: [
      {
        id: "sample",
        title: "示例共享模板",
        tags: [],
        asset: doc.asset,
        fingerprint: "fp",
      },
    ],
    diagnostics: [],
  });
  vi.mocked(api).mockImplementation(async (path: string) => {
    if (path === "workspace") return { id: "app-test", profile: "template" };
    if (path === "capabilities") return { actions: [generation], panels: [] };
    if (path === "plugins") return { plugins: [] };
    if (path === "runs") return { runs: [] };
    throw new Error(path);
  });
  vi.mocked(action).mockResolvedValue({
    data: { asset: doc.asset, fingerprint: "fp" },
  });
  render(<App />);
  return doc;
}

describe("active App shared parameter integration", () => {
  it("routes only generic managed documents to shared inputs and preserves old draft overrides", async () => {
    setup();
    await screen.findByText("共享参数输入");
    expect(screen.getByTestId("shared-context").textContent).toBe(
      '{"legacy":"kept"}',
    );
    expect(screen.queryByText("原参数表单")).toBeNull();
    expect(screen.queryByText("原共享选择器")).toBeNull();
    expect(screen.queryByText("恢复默认值")).toBeNull();
  });
  it("keeps ordinary template controls", async () => {
    setup("template.render");
    await screen.findByText("原参数表单");
    expect(screen.queryByText("共享参数输入")).toBeNull();
    expect(screen.getByText("恢复默认值")).toBeTruthy();
  });
  it("preserves retired NC drafts without opening any executable parameter controls", async () => {
    setup("template.render.managed", true, {
      nc: null,
      parameters: {},
    });
    await waitFor(() => expect(api).toHaveBeenCalledWith("workspace"));
    await screen.findByText("让模板成为可重复的工作");
    expect(screen.queryByText("原参数表单")).toBeNull();
    expect(screen.queryByText("原共享选择器")).toBeNull();
    expect(screen.queryByText("共享参数输入")).toBeNull();
    expect(screen.queryByRole("button", { name: "生成文件" })).toBeNull();
    expect(vi.mocked(loadDrafts).mock.results[0].value.documents[0].asset.metadata.nc).toBeNull();
  });
  it("opens a shared template with empty overrides", async () => {
    setup("template.render.managed", false);
    fireEvent.click((await screen.findAllByText("示例共享模板"))[0]);
    await screen.findByText("共享参数输入");
    expect(screen.getByTestId("shared-context").textContent).toBe("{}");
  });
  it("imports generic shared assets with empty overrides", async () => {
    setup();
    await screen.findByText("共享参数输入");
    const input = document.querySelector<HTMLInputElement>(
      'input[type="file"][accept=".json,.j2,.jinja2,.txt"]',
    )!;
    const asset = {
      source: "{{ width }}",
      defaults: { width: 20 },
      metadata: { parameters: {} },
    };
    fireEvent.change(input, {
      target: {
        files: [
          {
            name: "imported.json",
            size: 100,
            text: async () => JSON.stringify(asset),
          },
        ],
      },
    });
    await screen.findByText("已导入为草稿，保存后进入模板库");
    fireEvent.click(screen.getByText("使用模板"));
    expect(screen.getByTestId("shared-context").textContent).toBe("{}");
  });
  it("clears author trial overrides while preserving the shared choice", async () => {
    setup();
    await screen.findByText("共享参数输入");
    fireEvent.click(screen.getByText("编辑模板"));
    fireEvent.click(screen.getByText("默认参数"));
    fireEvent.click(screen.getByText("清除本次覆盖并试用继承参数"));
    fireEvent.click(screen.getByText("使用模板"));
    expect(screen.getByTestId("shared-context").textContent).toBe("{}");
  });
  it("blocks generate and Ctrl+Enter for incomplete JSON and sends the latest complete raw value", async () => {
    setup();
    fireEvent.click(await screen.findByText("输入错误 JSON"));
    fireEvent.click(screen.getByText("生成文件"));
    fireEvent.keyDown(window, { key: "Enter", ctrlKey: true });
    expect(
      vi.mocked(api).mock.calls.filter(([, body]) => body?.action),
    ).toHaveLength(0);
    fireEvent.click(screen.getByText("输入最新 JSON"));
    fireEvent.keyDown(window, { key: "Enter", ctrlKey: true });
    await waitFor(() =>
      expect(
        vi
          .mocked(api)
          .mock.calls.some(
            ([, body]) => body?.input?.context?.width?.toString() === "9",
          ),
      ).toBe(true),
    );
  });
});
