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
import { loadDrafts, saveDrafts } from "./lib/drafts";
import { loadTemplateCatalog } from "./lib/templateCatalog";

vi.mock("./lib/api", () => ({
  api: vi.fn(),
  action: vi.fn(),
  ApiError: class extends Error {},
}));
vi.mock("./lib/drafts", () => ({
  loadDrafts: vi.fn(),
  saveDrafts: vi.fn(() => true),
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
vi.mock("./components/LazyEditor", () => ({
  Editor: ({ value }: Data) => <pre data-testid="editor">{value}</pre>,
}));
vi.mock("./components/SchemaEditor", () => ({
  SchemaEditor: ({ schema, onChange }: Data) => (
    <div>
      <output data-testid="schema">{JSON.stringify(schema)}</output>
      <button
        onClick={() =>
          onChange({
            ...schema,
            properties: {
              ...(schema.properties || {}),
              manual: { type: "string", title: "手动参数" },
            },
            required: [...(schema.required || []), "manual"],
          })
        }
      >
        手动加参数
      </button>
    </div>
  ),
}));
vi.mock("./components/Form", () => ({ Form: () => <div>原有参数表单</div> }));
vi.mock("./plugins/ParameterViews", () => ({
  ParameterSelection: () => <div>原有共享选择器</div>,
}));
vi.mock("./components/SharedParameterInputs", () => ({
  SharedParameterInputs: ({ doc }: Data) => (
    <div>
      <span>共享参数输入</span>
      <output data-testid="shared-context">{JSON.stringify(doc.context)}</output>
    </div>
  ),
}));
vi.mock("./components/WorkbenchUi", () => ({
  Empty: ({ title }: Data) => <div>{title}</div>,
  PageTitle: ({ title }: Data) => <div>{title}</div>,
  Menu: ({ children }: Data) => <div>{children}</div>,
  Modal: ({ title, children, buttons }: Data) => (
    <div role="dialog">
      {title}
      {children}
      {buttons}
    </div>
  ),
  JsonEditor: () => <div>原有 JSON 编辑器</div>,
  RunDetail: ({ run, restore, restoreReason }: Data) => (
    <div>
      <span>记录 {run.id}</span>
      {restore && (
        <button onClick={restore} disabled={!!restoreReason}>
          恢复输入
        </button>
      )}
      {restoreReason && <p>{restoreReason}</p>}
    </div>
  ),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  localStorage.clear();
});

const managedAction = {
  id: "template.render.managed",
  title: "共享参数生成",
  ui_schema: {
    document_input: {
      default: true,
      bindings: { source: "source", context: "context" },
      title: "共享参数生成",
    },
  },
};

function sharedDocument() {
  const doc = newDocument();
  doc.mode = "use";
  doc.asset = {
    source: "{{ width }}",
    schema: { type: "object", properties: { width: { type: "number" } }, required: [] },
    defaults: { width: 20 },
    metadata: {},
    tags: [],
  };
  doc.context = {};
  return doc;
}

function shell(path: string) {
  if (path === "workspace") return { id: "app-test", profile: "template" };
  if (path === "capabilities") return { actions: [managedAction], panels: [] };
  if (path === "plugins") return { plugins: [] };
  return undefined;
}

describe("active App execution lifecycle", () => {
  it("enabling shared bindings updates the active action and survives a draft flush", async () => {
    const doc = newDocument();
    doc.generationAction = "template.render";
    doc.asset.source = "Hello";
    vi.mocked(loadDrafts).mockReturnValue({ documents: [doc], active: doc.key });
    vi.mocked(loadTemplateCatalog).mockResolvedValue({ items: [], diagnostics: [] });
    vi.mocked(api).mockImplementation(async (path: string) => {
      if (path === "capabilities") return { actions: [managedAction, {
        ...managedAction, id: "template.render", title: "普通模板生成",
      }], panels: [] };
      const base = shell(path);
      if (base) return base;
      if (path === "runs") return { runs: [] };
      throw new Error("unexpected api call: " + path);
    });
    vi.mocked(action).mockResolvedValue({ data: { items: [] } });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "共享绑定" }));
    fireEvent.click(await screen.findByRole("button", { name: "启用共享参数绑定" }));
    await waitFor(() => expect(
      (screen.getByLabelText("模板生成方式") as HTMLSelectElement).value,
    ).toBe("template.render.managed"));
    window.dispatchEvent(new Event("pagehide"));
    const stored = vi.mocked(saveDrafts).mock.calls.at(-1)?.[1] as Document[];
    expect(stored[0].asset.metadata?.parameters.bindings).toEqual({});
    expect(stored[0].generationAction).toBe("template.render.managed");
    expect(stored[0].asset.source).toBe("Hello");
  });

  it("never queues a second run from the shortcut while the first is in flight", async () => {
    const doc = sharedDocument();
    vi.mocked(loadDrafts).mockReturnValue({ documents: [doc], active: doc.key });
    vi.mocked(loadTemplateCatalog).mockResolvedValue({ items: [], diagnostics: [] });
    const created: Data[] = [];
    vi.mocked(api).mockImplementation(async (path: string, body?: Data) => {
      const base = shell(path);
      if (base) return base;
      if (path === "runs" && body) {
        created.push(body);
        return { id: "run-1" };
      }
      if (path === "runs") return { runs: [] };
      if (path === "runs/run-1")
        return {
          id: "run-1",
          title: doc.title,
          action: "template.render.managed",
          input: {},
          status: "queued",
          started_at: 0,
        };
      throw new Error("unexpected api call: " + path);
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "生成文件" }));
    await waitFor(() => expect(created).toHaveLength(1));
    // The document now has a queued run, so the surface is busy.
    fireEvent.keyDown(window, { key: "Enter", ctrlKey: true });
    await waitFor(() =>
      expect(screen.getAllByText(/任务正在执行/).length).toBeGreaterThan(0),
    );
    fireEvent.keyDown(window, { key: "Enter", ctrlKey: true });
    expect(created).toHaveLength(1);
  });

  it("keeps Ctrl+Enter and Ctrl+S out of an open dialog", async () => {
    const doc = sharedDocument();
    vi.mocked(loadDrafts).mockReturnValue({ documents: [doc], active: doc.key });
    vi.mocked(loadTemplateCatalog).mockResolvedValue({ items: [], diagnostics: [] });
    const created: Data[] = [];
    vi.mocked(api).mockImplementation(async (path: string, body?: Data) => {
      const base = shell(path);
      if (base) return base;
      if (path === "runs" && body) {
        created.push(body);
        return { id: "run-1" };
      }
      if (path === "runs") return { runs: [] };
      throw new Error("unexpected api call: " + path);
    });
    render(<App />);
    // An unsaved draft closes through a dialog that stays in front of the workbench.
    fireEvent.click(
      await screen.findByRole("button", { name: "关闭" + doc.title }),
    );
    await screen.findByRole("dialog");
    fireEvent.keyDown(window, { key: "Enter", ctrlKey: true });
    fireEvent.keyDown(window, { key: "s", ctrlKey: true });
    expect(created).toHaveLength(0);
    expect(vi.mocked(action)).not.toHaveBeenCalledWith(
      "template.save",
      expect.anything(),
    );
  });

  it("restores a shared-parameter history record through the existing button", async () => {
    const run = {
      id: "run-h1",
      title: "历史共享记录",
      action: "template.render.managed",
      input: {
        snapshot: {
          template: "tpl-a",
          templates: { "tpl-a": "{{ width }}" },
          asset: {
            source: "{{ width }}",
            schema: { type: "object", properties: {} },
            defaults: { width: 7 },
            metadata: { parameters: { bindings: { width: "width" } } },
          },
        },
        parameter_request: {
          bindings: { width: "width" },
          values: { width: 7 },
          defaults: {},
          schema: { type: "object", properties: {} },
        },
      },
      status: "succeeded",
      started_at: 1,
      result: {
        data: { text: "HISTORY-OUTPUT-42" },
        artifacts: [],
        diagnostics: [],
      },
    };
    vi.mocked(loadDrafts).mockReturnValue({ documents: [], active: null });
    vi.mocked(loadTemplateCatalog).mockResolvedValue({ items: [], diagnostics: [] });
    vi.mocked(api).mockImplementation(async (path: string) => {
      const base = shell(path);
      if (base) return base;
      if (path === "runs") return { runs: [run] };
      if (path === "runs/query") return { runs: [run], total: 1, offset: 0, limit: 25 };
      if (path === "runs/run-h1") return run;
      if (path === "runs/run-h1/replay-check")
        return { status: "matching", differences: [] };
      throw new Error("unexpected api call: " + path);
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "执行记录" }));
    fireEvent.click(
      (await screen.findAllByRole("button", { name: /历史共享记录/ }))[0],
    );
    fireEvent.click(await screen.findByRole("button", { name: "恢复输入" }));
    await screen.findByText("共享参数输入");
    expect(screen.getByText("历史共享记录 · 历史输入")).toBeTruthy();
    // The restored document is bound to the run it was restored from.
    expect(screen.getByText("HISTORY-OUTPUT-42")).toBeTruthy();
  });

  it("ignores a late 识别参数 response once the schema changed", async () => {
    const doc = newDocument();
    doc.asset = {
      source: "{{ a }} {{ b }}",
      schema: { type: "object", properties: {}, required: [] },
      defaults: {},
      metadata: {},
      tags: [],
    };
    vi.mocked(loadDrafts).mockReturnValue({ documents: [doc], active: doc.key });
    vi.mocked(loadTemplateCatalog).mockResolvedValue({ items: [], diagnostics: [] });
    let releaseInspect: (variables: Data) => void = () => {};
    const inspection = new Promise<Data>((resolve) => {
      releaseInspect = resolve;
    });
    vi.mocked(api).mockImplementation(async (path: string) => {
      const base = shell(path);
      if (base) return base;
      if (path === "runs") return { runs: [] };
      throw new Error("unexpected api call: " + path);
    });
    vi.mocked(action).mockImplementation(async (id: string) => {
      if (id === "template.inspect")
        return inspection.then((data) => ({ data }));
      throw new Error("unexpected action: " + id);
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /识别参数/ }));
    fireEvent.click(screen.getByRole("button", { name: "手动加参数" }));
    await waitFor(() =>
      expect(screen.getByTestId("schema").textContent).toContain("manual"),
    );
    releaseInspect({ variables: [{ name: "a" }, { name: "b", optional: true }] });
    await screen.findByText(/本次识别结果已忽略/);
    const schema = JSON.parse(screen.getByTestId("schema").textContent!);
    expect(schema.properties.manual).toBeTruthy();
    expect(schema.properties.a).toBeUndefined();
    expect(schema.properties.b).toBeUndefined();
  });

  it("a duplicated document never inherits the source run binding", async () => {
    const doc = sharedDocument();
    doc.id = "tpl-source";
    doc.lastRunId = "run-1";
    doc.lastRunSignature = "signature-of-the-source-run";
    vi.mocked(loadDrafts).mockReturnValue({ documents: [doc], active: doc.key });
    vi.mocked(loadTemplateCatalog).mockResolvedValue({ items: [], diagnostics: [] });
    vi.mocked(api).mockImplementation(async (path: string) => {
      const base = shell(path);
      if (base) return base;
      if (path === "runs") return { runs: [] };
      if (path === "runs/run-1")
        return {
          id: "run-1",
          title: doc.title,
          action: "template.render.managed",
          input: {},
          status: "succeeded",
          started_at: 0,
          result: { data: { text: "SOURCE-OUTPUT" }, artifacts: [], diagnostics: [] },
        };
      throw new Error("unexpected api call: " + path);
    });
    render(<App />);
    fireEvent.click(await screen.findByText("复制模板"));
    // An unload flushes the draft store synchronously; the copy is what a
    // reload would replay into the workspace.
    window.dispatchEvent(new Event("pagehide"));
    const stored = vi.mocked(saveDrafts).mock.calls.at(-1)?.[1] as Data[];
    const copy = stored.find((item) => item.title.endsWith("副本"));
    expect(copy).toBeTruthy();
    expect(copy.key).not.toBe(doc.key);
    expect(copy.lastRunId).toBeUndefined();
    expect(copy.lastRunSignature).toBeUndefined();
  });
});
