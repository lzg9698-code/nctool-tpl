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
vi.mock("./plugins/registry", () => ({ builtinViews: {} }));
const editorHarness = vi.hoisted(() => ({ ready: undefined as (() => void) | undefined, insert: vi.fn() }));
vi.mock("./components/LazyEditor", async () => {
  const { useEffect, useImperativeHandle } = await import("react");
  return { Editor: ({ value, ref, onReady }: Data) => {
    useImperativeHandle(ref, () => ({ insert: editorHarness.insert, focus: vi.fn() }), []);
    useEffect(() => { editorHarness.ready = onReady; return () => { editorHarness.ready = undefined; }; }, [onReady]);
    return <pre data-testid="editor">{value}</pre>;
  } };
});
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
  editorHarness.ready = undefined;
  vi.clearAllMocks();
  localStorage.clear();
});

const renderAction = {
  id: "template.render",
  title: "普通文本生成",
  ui_schema: {
    document_input: {
      default: true,
      bindings: {
        source: "source",
        context: "context",
        schema: "schema",
        defaults: "defaults",
      },
      title: "普通文本生成",
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
  if (path === "capabilities") return { actions: [renderAction], panels: [] };
  if (path === "plugins") return { plugins: [] };
  return undefined;
}

describe("active App execution lifecycle", () => {
  it.each(["workspace", "capabilities", "plugins"])("reports a failed %s initialization accurately", async (failedPath) => {
    vi.mocked(api).mockImplementation(async (path: string) => {
      if (path === failedPath) throw new Error("offline");
      return shell(path);
    });
    render(<App />);
    await screen.findByText("工作区连接失败");
    expect(screen.queryByText("本地服务已连接")).toBeNull();
  });

  it("keeps the connection established when only the catalog fails", async () => {
    vi.mocked(loadDrafts).mockReturnValue({ documents: [], active: null });
    vi.mocked(loadTemplateCatalog).mockRejectedValue(new Error("catalog unavailable"));
    vi.mocked(api).mockImplementation(async (path: string) => shell(path) || { runs: [] });
    render(<App />);
    await screen.findByText("本地服务已连接");
    expect((await screen.findAllByText("catalog unavailable")).length).toBeGreaterThan(0);
    expect(screen.queryByText("工作区连接失败")).toBeNull();
  });

  it("waits for the target editor and consumes an include insertion only once", async () => {
    const doc = sharedDocument();
    doc.mode = "edit";
    doc.tab = "settings";
    vi.mocked(loadDrafts).mockReturnValue({ documents: [doc], active: doc.key });
    vi.mocked(loadTemplateCatalog).mockResolvedValue({ items: [{ id: "shared", title: "共享模板", tags: [] }], diagnostics: [] });
    vi.mocked(api).mockImplementation(async (path: string) => shell(path) || { runs: [] });
    render(<App />);
    fireEvent.change(await screen.findByLabelText("插入其他模板引用"), { target: { value: "shared" } });
    await screen.findByTestId("editor");
    expect(editorHarness.insert).not.toHaveBeenCalled();
    editorHarness.ready?.();
    editorHarness.ready?.();
    expect(editorHarness.insert).toHaveBeenCalledExactlyOnceWith('{% include "shared" %}');
  });

  it("discards a pending include when switching documents before readiness", async () => {
    const doc = sharedDocument();
    doc.mode = "edit";
    doc.tab = "settings";
    doc.title = "引用目标";
    const other = sharedDocument();
    other.title = "另一个文档";
    vi.mocked(loadDrafts).mockReturnValue({ documents: [doc, other], active: doc.key });
    vi.mocked(loadTemplateCatalog).mockResolvedValue({ items: [{ id: "shared", title: "共享模板", tags: [] }], diagnostics: [] });
    vi.mocked(api).mockImplementation(async (path: string) => shell(path) || { runs: [] });
    render(<App />);
    fireEvent.change(await screen.findByLabelText("插入其他模板引用"), { target: { value: "shared" } });
    await screen.findByTestId("editor");
    const lateReady = editorHarness.ready;
    fireEvent.click(screen.getByRole("button", { name: other.title }));
    lateReady?.();
    fireEvent.click(screen.getByRole("button", { name: doc.title }));
    await screen.findByTestId("editor");
    editorHarness.ready?.();
    expect(editorHarness.insert).not.toHaveBeenCalled();
  });

  it("exposes the current document and selected workbench controls", async () => {
    const first = sharedDocument();
    first.title = "第一个模板";
    const second = sharedDocument();
    second.title = "第二个模板";
    vi.mocked(loadDrafts).mockReturnValue({ documents: [first, second], active: first.key });
    vi.mocked(loadTemplateCatalog).mockResolvedValue({ items: [], diagnostics: [] });
    vi.mocked(api).mockImplementation(async (path: string) => {
      const base = shell(path);
      if (base) return base;
      if (path === "runs") return { runs: [] };
      throw new Error("unexpected api call: " + path);
    });
    vi.mocked(action).mockResolvedValue({ data: { items: [] } });
    render(<App />);
    const firstButton = await screen.findByRole("button", { name: first.title });
    expect(firstButton.getAttribute("aria-current")).toBe("page");
    fireEvent.click(screen.getByRole("button", { name: second.title }));
    expect(firstButton.hasAttribute("aria-current")).toBe(false);
    expect(screen.getByRole("button", { name: second.title }).getAttribute("aria-current")).toBe("page");

    const selected = (name: string, value: boolean) =>
      screen.getByRole("button", { name, pressed: value });
    selected("使用模板", true);
    selected("编辑模板", false);
    selected("生成结果", true);
    fireEvent.click(selected("检查信息", false));
    selected("检查信息", true);
    selected("生成结果", false);
    fireEvent.click(selected("编辑模板", false));
    selected("编辑模板", true);
    selected("使用模板", false);
    selected("模板源码", true);
    fireEvent.click(selected("模板信息", false));
    selected("模板信息", true);
    selected("模板源码", false);
    await screen.findByText("参数");
    expect(screen.queryByRole("button", { name: "参数定义" })).toBeNull();
    expect(screen.queryByRole("button", { name: "默认参数" })).toBeNull();
    expect(screen.queryByRole("button", { name: "共享绑定" })).toBeNull();
    expect(vi.mocked(api).mock.calls.every(([, body]) => body === undefined)).toBe(true);
  });

  it("has a single everyday parameter area and no shared preset controls", async () => {
    const doc = newDocument();
    doc.generationAction = "template.render";
    doc.asset.source = "Hello";
    doc.asset.schema = { type: "object", properties: { name: { type: "string" } } };
    vi.mocked(loadDrafts).mockReturnValue({ documents: [doc], active: doc.key });
    vi.mocked(loadTemplateCatalog).mockResolvedValue({ items: [], diagnostics: [] });
    vi.mocked(api).mockImplementation(async (path: string) => {
      const base = shell(path);
      if (base) return base;
      if (path === "runs") return { runs: [] };
      throw new Error("unexpected api call: " + path);
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "编辑模板" }));
    await screen.findByText("参数");
    expect(screen.queryByRole("button", { name: "共享绑定" })).toBeNull();
    expect(screen.queryByLabelText("共享预置")).toBeNull();
    expect(screen.queryByText("保存本模板输入预设")).toBeNull();
    window.dispatchEvent(new Event("pagehide"));
    const stored = vi.mocked(saveDrafts).mock.calls.at(-1)?.[1] as Document[];
    expect(stored[0].asset.source).toBe("Hello");
    expect(stored[0].generationAction).toBe("template.render");
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
          action: "template.render",
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

  it("restores an ordinary history record through the existing button", async () => {
    const run = {
      id: "run-h1",
      title: "历史记录",
      action: "template.render",
      input: {
        snapshot: {
          template: "tpl-a",
          templates: { "tpl-a": "{{ width }}" },
          asset: {
            source: "{{ width }}",
            schema: { type: "object", properties: {} },
            defaults: { width: 7 },
            metadata: {},
          },
        },
        context: { width: 7 },
        defaults: { width: 7 },
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
      (await screen.findAllByRole("button", { name: /历史记录/ }))[0],
    );
    fireEvent.click(await screen.findByRole("button", { name: "恢复输入" }));
    await screen.findByText("历史记录 · 历史输入");
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
          action: "template.render",
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
