import {
  documentActions,
  domainTemplate,
  machiningRemovedMessage,
  restoreDocument,
  sameSchema,
  selectDocumentAction,
  documentInput,
  documentSignature,
} from "./lib/documentExecution";
import { WorkspaceImport } from "./components/WorkspaceImport";
import { useManagedDependencies } from "./lib/managedDependencies";
import { ParameterSelection } from "./plugins/ParameterViews";
import { SharedParameterInputs } from "./components/SharedParameterInputs";
const SharedBindingEditor = lazy(() =>
  import("./components/SharedBindingEditor").then((module) => ({
    default: module.SharedBindingEditor,
  })),
);
import {
  sharedParameterDocument,
  initialDocumentContext,
  resetDocumentParameters,
  documentForGeneration,
  templatePresetParameters,
  loadTemplatePresetParameters,
} from "./lib/appDocumentParameters";
import {
  cloneDocumentForCopy,
  mergeInspectedParameters,
  normalizeSchema,
  prepareDocument,
} from "./lib/documentModel";
// Secondary pages are split out: only the workbench is needed to reach first
// paint, and the history page alone pulls the diff library.
const SyntaxGuide = lazy(() =>
  import("./components/SyntaxGuide").then((module) => ({
    default: module.SyntaxGuide,
  })),
);
const PluginPage = lazy(() =>
  import("./components/PluginPage").then((module) => ({
    default: module.PluginPage,
  })),
);
const PluginSettings = lazy(() =>
  import("./components/PluginSettings").then((module) => ({
    default: module.PluginSettings,
  })),
);
const HistoryPage = lazy(() =>
  import("./components/HistoryPage").then((module) => ({
    default: module.HistoryPage,
  })),
);
import { builtinViews } from "./plugins/registry";
import { generalDocuments, loadTemplateCatalog } from "./lib/templateCatalog";
import type { HistoryComparison } from "./components/HistoryPage";
import {
  lazy,
  useEffect,
  useRef,
  useState,
  Suspense,
  type ReactNode,
} from "react";
import {
  Boxes,
  BookOpen,
  FileCode2,
  FolderOpen,
  Plus,
  Search,
  Play,
  Save,
  Download,
  Copy,
  Trash2,
  Settings2,
  History,
  ChevronRight,
  Check,
  AlertTriangle,
  X,
  FileInput,
  Code2,
  ListChecks,
  LoaderCircle,
  Square,
  PanelLeftClose,
  PanelLeftOpen,
} from "lucide-react";
import { Editor, type EditorHandle } from "./components/LazyEditor";
import {
  Empty,
  Menu,
  Modal,
  JsonEditor,
} from "./components/WorkbenchUi";
import { Divider } from "./components/Divider";
import { Form } from "./components/Form";
import { SchemaEditor } from "./components/SchemaEditor";
import { api, action, ApiError } from "./lib/api";
import {
  encode,
  parseData,
  newDocument,
  isDirty,
  mergeDefaults,
  download,
  diagnosticsFrom,
  type Document,
  type Asset,
  type Data,
  type Run,
  type Diagnostic,
  fieldErrors,
} from "./lib/data";
import { loadDrafts, cloneDraft } from "./lib/drafts";
import { useDraftPersistence } from "./hooks/useDraftPersistence";
import { useRunPolling } from "./hooks/useRunPolling";
import { applyTemplateSaveResult, templateSaveAsset } from "./lib/templateSave";
import { ConflictDialog } from "./components/ConflictDialog";
import { TemplateWelcome } from "./components/TemplateWelcome";
import "./styles.css";
interface Item {
  id: string;
  title: string;
  description: string;
  tags: string[];
  fingerprint: string;
}
type Page = "workbench" | "history" | "plugins" | string;
interface Dialog {
  title: string;
  body: ReactNode;
  buttons?: ReactNode;
}
const stateNames: Record<string, string> = {
  queued: "等待执行",
  running: "执行中",
  succeeded: "已完成",
  failed: "生成失败",
  cancelled: "已取消",
  interrupted: "已中断",
};
export default function App() {
  const [workspace, setWorkspace] = useState<Data>(null),
    [capabilities, setCapabilities] = useState<Data>({
      actions: [],
      panels: [],
    }),
    [inventory, setInventory] = useState<Data>({ plugins: [] });
  const [items, setItems] = useState<Item[]>([]),
    [documents, setDocuments] = useState<Document[]>([]),
    [active, setActive] = useState<string | null>(null),
    [page, setPage] = useState<Page>("workbench");
  const [query, setQuery] = useState(""),
    [tag, setTag] = useState(""),
    [toast, setToast] = useState<{ text: string; error?: boolean } | null>(
      null,
    ),
    [dialog, setDialog] = useState<Dialog | null>(null);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [catalogState, setCatalogState] = useState<
    "loading" | "ready" | "error"
  >("loading");
  const [catalogError, setCatalogError] = useState("");
  const [comparison, setComparison] = useState<HistoryComparison | null>(null);
  const bundleInput = useRef<HTMLInputElement>(null);
  const [paneRatio, setPaneRatio] = useState({ use: 42, edit: 61 });
  const [submitting, setSubmitting] = useState<string | null>(null);
  const [loading, setLoading] = useState(true),
    [saving, setSaving] = useState(false),
    [recovered, setRecovered] = useState(false),
    [errors, setErrors] = useState<Record<string, string>>({});
  const [runList, setRunList] = useState<Data[]>([]),
    [runs, setRuns] = useState<Record<string, Run>>({}),
    [execution, setExecution] = useState<
      Record<string, { id: string; signature: string }>
    >({}),
    [selectedHistory, setSelectedHistory] = useState<Run | null>(null);
  const [inspector, setInspector] = useState<"definition" | "defaults" | "bindings">(
      "definition",
    ),
    [paramMode, setParamMode] = useState<"form" | "json">("form"),
    [jsonError, setJsonError] = useState(""),
    [resultTab, setResultTab] = useState<
      "result" | "diagnostics" | "parameters"
    >("result");
  const code = useRef<EditorHandle>(null);
  const [jump, setJump] = useState<number | undefined>();
  const imported = useRef<HTMLInputElement>(null);
  // Latest documents for async continuations that must not act on a stale copy.
  const documentsRef = useRef(documents);
  documentsRef.current = documents;
  const visibleDocuments = generalDocuments(documents);
  const doc = visibleDocuments.find((d) => d.key === active);
  useEffect(() => {
    setJsonError("");
  }, [active]);
  useEffect(() => {
    if (
      active &&
      documents.some((d) => d.key === active) &&
      !generalDocuments(documents).some((d) => d.key === active)
    )
      setActive(generalDocuments(documents)[0]?.key || null);
  }, [documents, active]);
  const documentChoices = documentActions(capabilities.actions);
  const selection = doc
    ? selectDocumentAction(
        doc,
        capabilities.actions,
        capabilities.document_actions,
      )
    : {};
  const generation = selection.entry;
  const sharedParameters =
    !!doc && sharedParameterDocument(doc.asset, generation?.id);
  function actionForAsset(asset: Asset) {
    return selectDocumentAction(
      { ...newDocument(), asset },
      capabilities.actions,
      capabilities.document_actions,
    ).entry?.id;
  }
  const latest =
    doc && execution[doc.key] ? runs[execution[doc.key].id] : undefined;
  const managedDependencies = useManagedDependencies(latest);
  const running = Object.values(runs).filter((run) =>
    ["queued", "running"].includes(run.status),
  );
  const contributedView = capabilities.panels.find(
    (panel: Data) => panel.id === page,
  )?.view;
  const ContributedView = builtinViews[contributedView];
  const stale = !!(
    doc &&
    latest &&
    (managedDependencies.stale ||
      execution[doc.key]?.signature !== documentSignature(doc, generation?.id))
  );
  function resizePane(value: number) {
    const mode = doc?.mode || "use";
    setPaneRatio((current) => {
      const next = { ...current, [mode]: value };
      try {
        if (workspace)
          localStorage.setItem(
            "nctool:layout:" + workspace.id,
            JSON.stringify(next),
          );
      } catch {}
      return next;
    });
  }
  function notify(text: string, error = false) {
    setToast({ text, error });
  }
  function fail(error: Data) {
    notify(error.message || "操作失败", true);
  }
  async function catalog() {
    setCatalogState("loading");
    setCatalogError("");
    try {
      const result = await loadTemplateCatalog();
      setItems(result.items.filter((item) => !domainTemplate(item.asset)));
      setCatalogState("ready");
      if (result.diagnostics?.length) {
        setCatalogError("部分模板无法读取，请检查模板文件");
        notify("部分模板无法读取，请检查模板文件", true);
      }
    } catch (error) {
      setCatalogState("error");
      setCatalogError((error as Error).message || "模板列表读取失败");
      fail(error);
    }
  }
  function editDoc(
    update: Partial<Document> | ((doc: Document) => Document),
    key = active,
  ) {
    setDocuments((docs) =>
      docs.map((d) =>
        d.key === key
          ? typeof update === "function"
            ? update(d)
            : { ...d, ...update }
          : d,
      ),
    );
    setErrors({});
    setJump(undefined);
  }
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [w, c, i] = await Promise.all([
          api("workspace"),
          api("capabilities"),
          api("plugins"),
        ]);
        if (cancelled) return;
        setWorkspace(w);
        try {
          setSidebarCollapsed(
            localStorage.getItem("nctool:sidebar:" + w.id) === "collapsed",
          );
          const saved = JSON.parse(
            localStorage.getItem("nctool:layout:" + w.id) || "null",
          );
          if (
            saved &&
            saved.use >= 25 &&
            saved.use <= 70 &&
            saved.edit >= 25 &&
            saved.edit <= 70
          )
            setPaneRatio(saved);
        } catch {}

        setCapabilities(c);
        setInventory(i);
        const drafts = loadDrafts(w.id);
        setDocuments(drafts.documents);
        const visibleDrafts = generalDocuments(drafts.documents);
        setActive(
          visibleDrafts.find((d) => d.key === drafts.active)?.key ||
            visibleDrafts[0]?.key ||
            null,
        );
        setRecovered(visibleDrafts.length > 0);
        setExecution(
          Object.fromEntries(
            drafts.documents
              .filter((d) => d.lastRunId)
              .map((d) => [
                d.key,
                { id: d.lastRunId!, signature: d.lastRunSignature || "" },
              ]),
          ),
        );
        for (const draft of drafts.documents) {
          if (draft.lastRunId) {
            try {
              const previous = await api("runs/" + draft.lastRunId);
              setRuns((r) => ({ ...r, [previous.id]: previous }));
            } catch {}
          }
        }
        await catalog();
        const records = await api("runs");
        setRunList(records.runs);
      } catch (error) {
        fail(error);
      } finally {
        setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);
  function toggleSidebar() {
    const next = !sidebarCollapsed;
    setSidebarCollapsed(next);
    try {
      if (workspace)
        localStorage.setItem(
          "nctool:sidebar:" + workspace.id,
          next ? "collapsed" : "expanded",
        );
    } catch {}
  }
  useDraftPersistence(
    workspace?.id,
    documents,
    active,
    () => notify("浏览器草稿存储空间不足，请及时保存模板", true),
  );
  useEffect(() => {
    function keys(event: KeyboardEvent) {
      if (!(event.ctrlKey || event.metaKey)) return;
      // An open dialog or menu owns the keyboard: saving or generating behind it
      // would act on a document the user cannot see. Text fields keep their own
      // keys — the editor status bar documents Ctrl+S / Ctrl+Enter there.
      if (document.querySelector('[role="dialog"], [role="menu"]')) return;
      if (event.key.toLowerCase() === "s") {
        event.preventDefault();
        if (doc && !saving) void saveDocument();
      }
      if (event.key === "Enter") {
        event.preventDefault();
        if (doc) void generate();
      }
    }
    window.addEventListener("keydown", keys);
    return () => window.removeEventListener("keydown", keys);
  }, [doc, saving, capabilities, jsonError, submitting]);
  useRunPolling(runs, {
    onRuns: setRuns,
    onRunList: setRunList,
    onFailure: fail,
    onRunFailure: (message) => notify(message, true),
  });
  async function openTemplate(item: Item) {
    const existing = documents.find((d) => d.id === item.id);
    if (existing) {
      setActive(existing.key);
      setPage("workbench");

      return;
    }
    try {
      const result = await action("template.read", { name: item.id });
      if (domainTemplate(result.data.asset))
        throw new Error(machiningRemovedMessage);
      const loaded = prepareDocument(
        item.id,
        result.data.asset,
        result.data.fingerprint,
        actionForAsset(result.data.asset),
      );
      setDocuments((docs) => [...docs, loaded]);
      setActive(loaded.key);
      setPage("workbench");
      setErrors({});
      setJsonError("");
    } catch (error) {
      fail(error);
    }
  }
  function useTemplate() {
    if (doc) editDoc({ mode: "use" });
  }
  function create() {
    const draft = newDocument();
    setDocuments((docs) => [...docs, draft]);
    setActive(draft.key);
    setPage("workbench");
    setParamMode("form");
    setJsonError("");
    setErrors({});
  }
  function closeDocument(key: string) {
    const closing = documents.find((d) => d.key === key);
    const close = () => {
      setDocuments((docs) => docs.filter((d) => d.key !== key));
      if (active === key)
        setActive(
          generalDocuments(documents).find((d) => d.key !== key)?.key || null,
        );
      setDialog(null);
    };
    if (closing && isDirty(closing))
      setDialog({
        title: "关闭未保存的模板？",
        body: (
          <p>
            「{closing.title}」的草稿尚未保存。选择继续编辑，或明确丢弃草稿。
          </p>
        ),
        buttons: (
          <>
            <button
              className="button secondary"
              onClick={() => setDialog(null)}
            >
              继续编辑
            </button>
            <button className="button danger" onClick={close}>
              丢弃草稿并关闭
            </button>
          </>
        ),
      });
    else close();
  }
  async function saveDocument() {
    if (!doc) return;
    if (!doc.title.trim()) {
      notify("请填写模板名称", true);
      return;
    }
    setSaving(true);
    const savingKey = doc.key;
    try {
      const id = doc.id || "tpl-" + crypto.randomUUID();
      const asset = templateSaveAsset(doc, generation?.id);
      const response = await action("template.save", {
        name: id,
        asset,
        expected: doc.fingerprint,
      });
      setDocuments((current) =>
        current.map((currentDoc) =>
          currentDoc.key === savingKey
            ? applyTemplateSaveResult(
                currentDoc,
                doc,
                id,
                asset,
                response.data.fingerprint,
              )
            : currentDoc,
        ),
      );
      await catalog();
      notify("模板已保存");
    } catch (error) {
      if (error instanceof ApiError && error.detail.code === "conflict") {
        const response = doc.id
          ? await action("template.read", { name: doc.id })
          : null;
        setDialog({
          title: "模板已被其他编辑修改",
          body: (
            <ConflictDialog
              draftSource={doc.asset.source}
              diskAsset={response?.data.asset}
              onContinue={() => setDialog(null)}
              onLoadDisk={() => {
                if (response && doc.id) {
                  editDoc(
                    (current) => ({
                      ...prepareDocument(
                        doc.id!,
                        response.data.asset,
                        response.data.fingerprint,
                        actionForAsset(response.data.asset),
                      ),
                      key: savingKey,
                      mode: doc.mode,
                      tab: doc.tab,
                      context: doc.context,
                      // Preserve the run binding while the new fingerprint marks
                      // the input as changed against its previous result.
                      lastRunId: current.lastRunId,
                      lastRunSignature: current.lastRunSignature,
                    }),
                    savingKey,
                  );
                }
                setDialog(null);
              }}
              onSaveCopy={() => {
                const copy = cloneDocumentForCopy(doc);
                setDocuments((documents) => [...documents, copy]);
                setActive(copy.key);
                setDialog(null);
              }}
            />
          ),
        });
      } else fail(error);
    } finally {
      setSaving(false);
    }
  }
  async function copyTemplate() {
    if (!doc) return;
    try {
      if (doc.id && !isDirty(doc)) {
        const id = "tpl-" + crypto.randomUUID();
        const result = await action("template.copy", {
          name: doc.id,
          new_name: id,
          title: doc.title + " 副本",
        });
        await catalog();
        const copied = prepareDocument(
          id,
          result.data.asset,
          result.data.fingerprint,
          actionForAsset(result.data.asset),
        );
        copied.mode = "edit";
        setDocuments((d) => [...d, copied]);
        setActive(copied.key);
      } else {
        const copied = cloneDocumentForCopy(doc);
        setDocuments((d) => [...d, copied]);
        setActive(copied.key);
      }
    } catch (error) {
      fail(error);
    }
  }
  function deleteTemplate() {
    if (!doc?.id) return;
    setDialog({
      title: "删除这个模板？",
      body: (
        <p>
          删除「{doc.title}
          」会移除已保存的模板。正在引用它的其他模板会阻止删除。
        </p>
      ),
      buttons: (
        <>
          <button className="button secondary" onClick={() => setDialog(null)}>
            保留模板
          </button>
          <button
            className="button danger"
            onClick={async () => {
              try {
                await action("template.remove", {
                  name: doc.id,
                  expected: doc.fingerprint,
                });
                setDocuments((d) => d.filter((x) => x.key !== doc.key));
                setActive(null);
                setDialog(null);
                await catalog();
                notify("模板已删除");
              } catch (error) {
                fail(error);
              }
            }}
          >
            删除模板
          </button>
        </>
      ),
    });
  }
  async function importFile(file: File) {
    try {
      if (file.size > 1024 * 1024)
        throw new Error("单个模板文件不能超过 1 MiB");
      const text = await file.text();
      const asset: Asset = file.name.endsWith(".json")
        ? parseData(text)
        : {
            source: text,
            schema: { type: "object", properties: {}, required: [] },
            defaults: {},
            tags: [],
            metadata: {},
          };
      if (typeof asset.source !== "string")
        throw new Error("模板文件缺少 source 字段");
      if (domainTemplate(asset))
        throw new Error(machiningRemovedMessage);
      const draft = newDocument();
      draft.asset = asset;
      draft.title =
        asset.metadata?.title || file.name.replace(/\.(json|j2|jinja2)$/, "");
      draft.context = initialDocumentContext(asset, actionForAsset(asset));
      setDocuments((d) => [...d, draft]);
      setActive(draft.key);
      setPage("workbench");
      notify("已导入为草稿，保存后进入模板库");
    } catch (error) {
      fail(error);
    }
  }
  async function discover() {
    if (!doc) return;
    // Inspection describes exactly the source and schema captured here. A late
    // response must never overwrite a schema the user edited while it was in
    // flight, and must not drop definitions added in the meantime.
    const request = {
      key: doc.key,
      source: doc.asset.source,
      schema: cloneDraft(normalizeSchema(doc.asset)),
    };
    try {
      const inspected = await action("template.inspect", {
        source: doc.asset.source,
        ...(doc.id ? { template: doc.id } : {}),
      });
      const current = documentsRef.current.find((d) => d.key === request.key);
      if (
        !current ||
        current.asset.source !== request.source ||
        !sameSchema(request.schema, normalizeSchema(current.asset))
      ) {
        notify("源码或规格已修改，本次识别结果已忽略，请重新识别", true);
        return;
      }
      const merged = mergeInspectedParameters(
        current.asset,
        inspected.data.variables,
      );
      if (merged.changed) editDoc({ asset: merged.asset }, request.key);
      setInspector("definition");
      notify(
        merged.added
          ? `发现 ${merged.added} 个新参数，请选择类型`
          : "参数已与源码引用同步",
      );
    } catch (error) {
      fail(error);
      const line = errorLine(error);
      if (line) setJump(line);
    }
  }
  async function generate() {
    if (!doc) return;
    // One gate for every entry point. The toolbar and footer buttons disable on
    // the same condition, so a shortcut must never queue a second task while
    // this document's previous task is still waiting or running.
    if (busy) {
      notify(busyHint, true);
      return;
    }
    if (jsonError || inputError) {
      notify(
        jsonError
          ? "请先修正 JSON 输入，原有参数不会代替当前输入执行"
          : inputError,
        true,
      );
      return;
    }
    const validation = generation?.id.endsWith(".managed")
      ? {}
      : fieldErrors(
          normalizeSchema(doc.asset),
          mergeDefaults(doc.asset.defaults, documentForGeneration(doc).context),
        );
    setErrors(validation);
    if (Object.keys(validation).length) {
      notify("请补齐或修正标记的参数", true);
      return;
    }
    if (!doc.asset.source.trim()) {
      notify("请先编写或选择一个模板", true);
      return;
    }
    setSubmitting(doc.key);
    try {
      if (!generation) throw new Error(selection.error || "请选择生成方式");
      const input = documentInput(documentForGeneration(doc), generation);
      const response = await api("runs", {
        action: generation.id,
        input,
        title: doc.title,
      });
      const pending: Run = {
        id: response.id,
        title: doc.title,
        action: generation.id,
        input,
        status: "queued",
        started_at: Date.now(),
      };
      setRuns((r) => ({ ...r, [pending.id]: pending }));
      setExecution((e) => ({
        ...e,
        [doc.key]: {
          id: pending.id,
          signature: documentSignature(doc, generation?.id),
        },
      }));
      editDoc(
        {
          lastRunId: pending.id,
          lastRunSignature: documentSignature(doc, generation?.id),
        },
        doc.key,
      );
      setResultTab("result");
      notify("任务已开始，可继续编辑其他模板");
    } catch (error) {
      fail(error);
      if (
        error instanceof ApiError &&
        error.detail.code === "numeric_underflow"
      )
        setErrors({ "": "数值过小，无法由后端精确表示；原始输入已保留" });
    } finally {
      setSubmitting(null);
    }
  }
  async function runAction(id: string, input: Data, title: string) {
    try {
      const created = await api("runs", { action: id, input, title });
      const pending: Run = {
        id: created.id,
        title,
        action: id,
        input,
        status: "queued",
        started_at: Date.now(),
      };
      setRuns((r) => ({ ...r, [pending.id]: pending }));
      return pending.id;
    } catch (error) {
      fail(error);
      return null;
    }
  }
  function errorLine(error: Data): number | undefined {
    const message = error?.message || "";
    const match = message.match(/(?:in .*?:|第\s*)(\d+)(?:\)|\s*行)/);
    return match ? Number(match[1]) : undefined;
  }
  function jumpDiagnostic(diagnostic: Diagnostic) {
    const path = (diagnostic.path || "").replace(/^\/context/, "");
    const field = Array.from(
      document.querySelectorAll<HTMLElement>("[data-field-path]"),
    ).find(
      (el) =>
        el.dataset.fieldPath === path || el.dataset.fieldPath === "/" + path,
    );
    if (field) {
      field.scrollIntoView({ block: "center" });
      field.querySelector<HTMLElement>("input,textarea,select")?.focus();
      return;
    }
    const line = errorLine(diagnostic);
    if (line) {
      editDoc({ mode: "edit", tab: "source" });
      setJump(line);
      code.current?.focus();
    }
  }
  function savePreset() {
    if (!doc) return;
    let name = "";
    setDialog({
      title: "保存参数预设",
      body: (
        <label>
          预设名称
          <input
            placeholder="例如：标准尺寸"
            onChange={(e) => (name = e.target.value)}
          />
        </label>
      ),
      buttons: (
        <>
          <button className="button secondary" onClick={() => setDialog(null)}>
            取消
          </button>
          <button
            className="button primary"
            onClick={async () => {
              if (!name.trim()) {
                notify("请输入预设名称", true);
                return;
              }
              try {
                await action("preset.save", {
                  name: "preset-" + crypto.randomUUID(),
                  asset: {
                    title: name,
                    template: doc.id,
                    document_title: doc.title,
                    ...templatePresetParameters(documentForGeneration(doc)),
                  },
                });
                setDialog(null);
                notify("预设已保存");
              } catch (error) {
                fail(error);
              }
            }}
          >
            保存预设
          </button>
        </>
      ),
    });
  }
  async function loadPreset() {
    try {
      const listed = await action("preset.list", {});
      const entries = await Promise.all(
        listed.data.names.map(async (name: string) => {
          const read = await action("preset.read", { name });
          return { id: name, ...read.data.asset };
        }),
      );
      const relevant = entries.filter((entry: Data) =>
        doc?.id
          ? entry.template === doc.id
          : entry.document_title === doc?.title,
      );
      setDialog({
        title: "选择参数预设",
        body: (
          <div className="preset-list">
            {relevant.length ? (
              relevant.map((entry: Data) => (
                <button
                  key={entry.id}
                  onClick={() => {
                    editDoc(loadTemplatePresetParameters(entry));
                    setJsonError("");
                    setDialog(null);
                  }}
                >
                  <FileCode2 size={16} />
                  {entry.title || entry.id}
                  <ChevronRight size={14} />
                </button>
              ))
            ) : (
              <Empty
                title="还没有这个模板的预设"
                description="填写好参数后，可以保存为预设供下次使用。"
              />
            )}
          </div>
        ),
      });
    } catch (error) {
      fail(error);
    }
  }
  async function restoreRun(run: Run) {
    try {
      // restoreDocument owns the rules: retired machining intent and unavailable
      // actions are refused, and every generic render action — including shared
      // parameters — restores its frozen input, preset choice, extensions and
      // run binding instead of only the plain render action.
      const restored = restoreDocument(run, capabilities.actions);
      setDocuments((d) => [...d, restored]);
      setActive(restored.key);
      setExecution((e) => ({
        ...e,
        [restored.key]: {
          id: run.id,
          signature:
            restored.lastRunSignature ||
            documentSignature(restored, run.action),
        },
      }));
      setRuns((r) => ({ ...r, [run.id]: run }));
      setPage("workbench");
      setErrors({});
      setJsonError("");
      setParamMode("form");
    } catch (error) {
      fail(error);
    }
  }
  const visible = items.filter(
    (item) =>
      (!query ||
        [item.title, item.description, item.id, ...item.tags]
          .join(" ")
          .toLowerCase()
          .includes(query.toLowerCase())) &&
      (!tag || item.tags.includes(tag)),
  );
  const tags = [...new Set(items.flatMap((item) => item.tags))];
  const diagnostics = latest?.error
    ? diagnosticsFrom(latest.error)
    : latest?.result?.diagnostics || [];
  const text =
    latest?.status === "succeeded"
      ? latest.result?.data.text || latest.result?.artifacts?.[0]?.text || ""
      : "";
  const busy =
    submitting === doc?.key ||
    !!(latest && ["queued", "running"].includes(latest.status));
  let inputError = "";
  if (doc) {
    try {
      documentForGeneration(doc);
    } catch {
      inputError =
        doc.jsonInput !== undefined
          ? "请先修正参数 JSON，再生成文件"
          : "请先填写完整、有效的数值";
    }
  }
  const exportReason =
    inputError ||
    (stale
      ? "输入已修改，请重新生成后导出"
      : latest?.status === "succeeded"
        ? ""
        : busy
          ? "任务正在执行，完成后才能导出"
          : latest
            ? "本次任务没有可交付结果，请重新生成"
            : "尚未生成文件，生成成功后才能导出");
  const busyHint = "任务正在执行，可在结果区取消";
  const generateHint = busy
    ? busyHint
    : inputError || selection.error || "Ctrl+Enter 生成文件";
  return (
    <div
      className={"app-shell" + (sidebarCollapsed ? " sidebar-collapsed" : "")}
    >
      <aside className="sidebar">
        <button
          className="icon-button sidebar-toggle"
          onClick={toggleSidebar}
          aria-label={sidebarCollapsed ? "展开侧栏" : "折叠侧栏"}
          title={sidebarCollapsed ? "展开侧栏" : "折叠侧栏"}
        >
          {sidebarCollapsed ? (
            <PanelLeftOpen size={18} />
          ) : (
            <PanelLeftClose size={18} />
          )}
        </button>
        <div className="brand">
          <span className="brand-mark">
            <Boxes size={22} />
          </span>
          <div className="brand-name">
            NCtool<span>模板工作台</span>
          </div>
          <span className="version-label">2.2</span>
        </div>
        <div className="workspace-indicator">
          <span className="online-dot" />
          通用模板工作区
        </div>        <div className="sidebar-section-title">工作空间</div>
        <nav className="sidebar-nav" aria-label="工作空间导航">
          <button
            aria-label={"模板库 · " + items.length + " 个模板"}
            title="模板库"
            className={"nav-item " + (page === "workbench" ? "active" : "")}
            onClick={() => setPage("workbench")}
          >
            <FolderOpen size={17} />
            <span className="nav-label">模板库</span>
            <span className="nav-count">{items.length}</span>
          </button>
          <button
            aria-label="执行记录"
            title="执行记录"
            className={"nav-item " + (page === "history" ? "active" : "")}
            onClick={() => {
              setPage("history");
              api("runs")
                .then((r) => setRunList(r.runs))
                .catch(fail);
            }}
          >
            <History size={17} />
            <span className="nav-label">执行记录</span>
            <span className="nav-count">{runList.length || ""}</span>
          </button>
          {capabilities.panels
            .filter(
              (p: Data) => p.view !== "template-editor" && p.id !== "presets",
            )
            .map((panel: Data) => (
              <button
                key={panel.id}
                aria-label={panel.title}
                title={panel.title}
                className={"nav-item " + (page === panel.id ? "active" : "")}
                onClick={() => setPage(panel.id)}
              >
                <Boxes size={17} />
                <span className="nav-label">{panel.title}</span>
              </button>
            ))}
        </nav>
        <div className="sidebar-divider" />
        <div className="sidebar-section-title">
          我的模板
          <button
            className="icon-button"
            aria-label="新建模板"
            onClick={create}
          >
            <Plus size={15} />
          </button>
        </div>
        <button
          className="nav-item collapsed-create"
          aria-label="新建模板"
          title="新建模板"
          onClick={create}
        >
          <Plus size={18} />
        </button>
        <div className="search-field">
          <Search size={14} />
          <input
            aria-label="搜索模板"
            placeholder="搜索名称、标签…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          {query && (
            <button
              className="icon-button"
              aria-label="清除搜索"
              onClick={() => setQuery("")}
            >
              <X size={12} />
            </button>
          )}
        </div>
        {tags.length > 0 && (
          <div className="tag-filter">
            <select
              aria-label="按标签筛选模板"
              value={tag}
              onChange={(e) => setTag(e.target.value)}
            >
              <option value="">全部标签</option>
              {tags.map((label) => (
                <option key={label} value={label}>
                  {label}
                </option>
              ))}
            </select>
          </div>
        )}
        <div className="filter-summary" role="status">
          <span>
            {catalogState === "loading"
              ? "正在读取模板…"
              : `显示 ${visible.length} / ${items.length} 个模板`}
          </span>
          {(query || tag) && (
            <button
              className="text-button"
              onClick={() => {
                setQuery("");
                setTag("");
              }}
            >
              清除筛选
            </button>
          )}
        </div>
        <div className="template-list">
          {catalogError && (
            <div className="catalog-error" role="alert">
              <AlertTriangle size={16} />
              <p>{catalogError}</p>
              <button className="text-button" onClick={() => void catalog()}>
                重新读取
              </button>
            </div>
          )}
          {catalogState === "loading" && !items.length && (
            <div className="sidebar-empty">
              <LoaderCircle size={18} className="spin" /> 正在读取模板
            </div>
          )}
          {visible.map((item) => (
            <button
              key={item.id}
              title={
                item.title + (item.description ? "\n" + item.description : "")
              }
              className={
                "template-item " + (doc?.id === item.id ? "selected" : "")
              }
              onClick={() => void openTemplate(item)}
            >
              <FileCode2 size={16} />
              <span>
                <strong>{item.title}</strong>
                {item.description && <small>{item.description}</small>}
              </span>
            </button>
          ))}
          {!items.length && catalogState === "ready" && (
            <div className="sidebar-empty">
              模板库为空
              <br />
              <button onClick={create}>创建第一个模板</button>
            </div>
          )}
          {items.length > 0 && !visible.length && catalogState !== "error" && (
            <div className="sidebar-empty">没有匹配的模板</div>
          )}
        </div>
        <div className="sidebar-bottom">
          <button
            className="nav-item"
            aria-label="导入模板"
            title="导入模板"
            onClick={() => imported.current?.click()}
          >
            <FileInput size={17} />
            <span className="nav-label">导入模板</span>
          </button>
          <Menu
            label="工作区资产"
            text="工作区资产"
            icon={<FolderOpen size={17} />}
          >
            <button
              className="nav-item"
              onClick={async () => {
                try {
                  const bundle = await api("bundle");
                  download(
                    encode(bundle),
                    "nctool-workspace.json",
                    "application/json",
                  );
                  notify("已导出当前已启用插件提供的全部资产集合");
                } catch (error) {
                  fail(error);
                }
              }}
            >
              <Download size={17} />
              导出工作区资产
            </button>
            <button
              className="nav-item"
              onClick={() => bundleInput.current?.click()}
            >
              <FileInput size={17} />
              导入工作区资产
            </button>
          </Menu>
          <button
            aria-label="模板语法"
            title="模板语法"
            className={"nav-item " + (page === "syntax-guide" ? "active" : "")}
            onClick={() => setPage("syntax-guide")}
          >
            <BookOpen size={17} />
            <span className="nav-label">模板语法</span>
          </button>
          <button
            aria-label="插件与设置"
            title="插件与设置"
            className={"nav-item " + (page === "plugins" ? "active" : "")}
            onClick={() => setPage("plugins")}
          >
            <Settings2 size={17} />
            <span className="nav-label">插件与设置</span>
          </button>
        </div>
      </aside>
      <main className="main-area">
        {toast && (
          <div
            role={toast.error ? "alert" : "status"}
            className={"toast " + (toast.error ? "error" : "")}
          >
            <span>
              {toast.error ? <AlertTriangle size={16} /> : <Check size={16} />}
            </span>
            {toast.text}
            <button
              className="icon-button"
              aria-label="关闭提示"
              onClick={() => setToast(null)}
            >
              <X size={15} />
            </button>
          </div>
        )}
        {loading ? (
          <div className="center-loading">
            <LoaderCircle className="spin" />
            正在连接工作区…
          </div>
        ) : page === "workbench" ? (
          <>
            <div className="document-tabs">
              {visibleDocuments.map((document) => (
                <div
                  key={document.key}
                  className={
                    "document-tab " + (document.key === active ? "active" : "")
                  }
                >
                  <button
                    onClick={() => {
                      setActive(document.key);
                      setErrors({});
                      setJsonError("");
                      setParamMode("form");
                    }}
                  >
                    <FileCode2 size={14} />
                    {document.title}
                    {isDirty(document) && <span className="dirty-dot" />}
                  </button>
                  <button
                    aria-label={"关闭" + document.title}
                    onClick={() => closeDocument(document.key)}
                  >
                    <X size={12} />
                  </button>
                </div>
              ))}
              <button className="icon-button" title="新建模板" onClick={create}>
                <Plus size={16} />
              </button>
            </div>
            {recovered && (
              <div className="draft-banner">
                <Check size={14} />
                已恢复上次的工作区草稿
                <button
                  className="text-button"
                  onClick={() => setRecovered(false)}
                >
                  知道了
                </button>
              </div>
            )}
            {!doc ? (
              <TemplateWelcome
                items={items}
                onCreate={create}
                onImport={() => imported.current?.click()}
                onOpen={(item) => void openTemplate(item)}
                onSaved={catalog}
                emptyReady={catalogState === "ready" && !items.length}
              />
            ) : (
              <>
                <header className="workbench-toolbar">
                  <div className="toolbar-row document-row">
                    <div className="document-title">
                      <input
                        aria-label="模板名称"
                        value={doc.title}
                        title={doc.title}
                        onChange={(e) => editDoc({ title: e.target.value })}
                      />
                      <span className="document-state">
                        {isDirty(doc) ? (
                          <span className="dirty-dot" />
                        ) : (
                          <Check size={12} />
                        )}
                        {isDirty(doc) ? "有未保存修改" : "已保存"}
                        {doc.id ? "" : " · 新模板"}
                      </span>
                    </div>
                    <div className="toolbar-actions">
                      <button
                        className="button secondary"
                        onClick={() => void saveDocument()}
                        disabled={saving}
                      >
                        {saving ? (
                          <LoaderCircle size={15} className="spin" />
                        ) : (
                          <Save size={15} />
                        )}
                        保存
                      </button>
                      <Menu>
                        <button onClick={() => void copyTemplate()}>
                          <Copy size={14} />
                          复制模板
                        </button>
                        <button
                          onClick={() =>
                            download(
                              encode(
                                {
                                  ...doc.asset,
                                  metadata: {
                                    ...doc.asset.metadata,
                                    title: doc.title,
                                  },
                                },
                                true,
                              ),
                              doc.title + ".template.json",
                              "application/json",
                            )
                          }
                        >
                          <Download size={14} />
                          导出模板
                        </button>
                        {doc.id && (
                          <button className="danger" onClick={deleteTemplate}>
                            <Trash2 size={14} />
                            删除模板
                          </button>
                        )}
                      </Menu>
                    </div>
                  </div>
                  <div className="toolbar-row mode-row">
                    <div className="mode-controls">
                      <div className="segmented">
                        <button
                          className={doc.mode === "use" ? "selected" : ""}
                          onClick={() => useTemplate()}
                        >
                          <ListChecks size={14} />
                          使用模板
                        </button>
                        <button
                          className={doc.mode === "edit" ? "selected" : ""}
                          onClick={() => editDoc({ mode: "edit" })}
                        >
                          <Code2 size={14} />
                          编辑模板
                        </button>
                      </div>
                      <label className="generation-choice">
                        生成方式
                        <select
                          aria-label="模板生成方式"
                          value={generation?.id || doc.generationAction || ""}
                          onChange={(event) =>
                            editDoc({ generationAction: event.target.value })
                          }
                        >
                          {!generation && (
                            <option value={doc.generationAction || ""}>
                              请选择可用方式
                            </option>
                          )}
                          {documentChoices.map((entry: Data) => (
                            <option key={entry.id} value={entry.id}>
                              {entry.ui_schema.document_input.title ||
                                entry.title}
                            </option>
                          ))}
                        </select>
                      </label>
                    </div>
                    {doc.mode === "edit" && (
                      <div className="author-generate">
                        <button
                          className="button primary"
                          disabled={busy}
                          title={generateHint}
                          onClick={() => void generate()}
                        >
                          <Play size={15} />
                          生成文件
                        </button>
                        {busy && (
                          <span className="action-hint">{generateHint}</span>
                        )}
                      </div>
                    )}
                  </div>
                </header>
                {selection.error && (
                  <div className="generation-notice inline-error" role="alert">
                    {selection.error}
                  </div>
                )}
                {generation?.ui_schema.document_input.match_metadata && (
                  <div className="generation-notice" role="status">
                    <strong>{generation.ui_schema.document_input.title}</strong>{" "}
                    · {generation.ui_schema.document_input.description}
                  </div>
                )}
                <div
                  className={
                    "workbench-body " +
                    (doc.mode === "edit" ? "author" : "runner")
                  }
                  style={{
                    gridTemplateColumns: `minmax(250px, ${paneRatio[doc.mode]}fr) minmax(250px, ${100 - paneRatio[doc.mode]}fr)`,
                  }}
                >
                  <Divider
                    value={paneRatio[doc.mode]}
                    onChange={resizePane}
                    author={doc.mode === "edit"}
                  />
                  {doc.mode === "edit" ? (
                    <>
                      <section className="source-pane">
                        <div className="pane-toolbar">
                          <div className="pane-tabs">
                            <button
                              className={doc.tab === "source" ? "selected" : ""}
                              onClick={() => editDoc({ tab: "source" })}
                            >
                              模板源码
                            </button>
                            <button
                              className={doc.tab === "schema" ? "selected" : ""}
                              onClick={() => editDoc({ tab: "schema" })}
                            >
                              高级规格
                            </button>
                            <button
                              className={
                                doc.tab === "settings" ? "selected" : ""
                              }
                              onClick={() => editDoc({ tab: "settings" })}
                            >
                              模板信息
                            </button>
                          </div>
                          {doc.tab === "source" && (
                            <button
                              className="text-button"
                              onClick={() => setPage("syntax-guide")}
                            >
                              <BookOpen size={13} />
                              语法提示
                            </button>
                          )}
                          {doc.tab === "source" && (
                            <button
                              className="text-button"
                              onClick={() => void discover()}
                            >
                              <Search size={13} />
                              识别参数
                            </button>
                          )}
                        </div>
                        {doc.tab === "source" ? (
                          <Editor
                            ref={code}
                            editorKey={doc.key}
                            value={doc.asset.source}
                            onChange={(source) =>
                              editDoc((d) => ({
                                ...d,
                                asset: { ...d.asset, source },
                              }))
                            }
                            line={jump}
                            label="模板源码"
                          />
                        ) : doc.tab === "schema" ? (
                          <JsonEditor
                            value={normalizeSchema(doc.asset)}
                            onChange={(schema) =>
                              editDoc((d) => ({
                                ...d,
                                asset: { ...d.asset, schema },
                              }))
                            }
                            label="高级参数规格"
                          />
                        ) : (
                          <div className="pane-scroll metadata-editor">
                            <label>
                              说明
                              <textarea
                                rows={3}
                                value={doc.asset.metadata?.description || ""}
                                onChange={(e) =>
                                  editDoc((d) => ({
                                    ...d,
                                    asset: {
                                      ...d.asset,
                                      metadata: {
                                        ...d.asset.metadata,
                                        description: e.target.value,
                                      },
                                    },
                                  }))
                                }
                              />
                            </label>
                            <label>
                              标签（逗号分隔）
                              <input
                                value={(doc.asset.tags || []).join(", ")}
                                onChange={(e) =>
                                  editDoc((d) => ({
                                    ...d,
                                    asset: {
                                      ...d.asset,
                                      tags: e.target.value
                                        .split(/[,，]/)
                                        .map((x) => x.trim())
                                        .filter(Boolean),
                                    },
                                  }))
                                }
                              />
                            </label>
                            <label>
                              输出文件名
                              <input
                                value={doc.outputName}
                                onChange={(e) =>
                                  editDoc({ outputName: e.target.value })
                                }
                              />
                            </label>
                            <label>
                              模板标识
                              <input
                                readOnly
                                value={doc.id || "保存后自动生成"}
                              />
                            </label>
                            <p className="hint">
                              模板名称可以修改，模板标识保持稳定，已有引用不受影响。
                            </p>
                            <label>
                              插入其他模板引用
                              <select
                                defaultValue=""
                                onChange={(e) => {
                                  if (e.target.value) {
                                    editDoc({ tab: "source" });
                                    setTimeout(
                                      () =>
                                        code.current?.insert(
                                          `{% include "${e.target.value}" %}`,
                                        ),
                                      50,
                                    );
                                  }
                                }}
                              >
                                <option value="">选择模板</option>
                                {items
                                  .filter((item) => item.id !== doc.id)
                                  .map((item) => (
                                    <option key={item.id} value={item.id}>
                                      {item.title}
                                    </option>
                                  ))}
                              </select>
                            </label>
                          </div>
                        )}
                        <div className="editor-status">
                          <span>Jinja · UTF-8</span>
                          <span>{doc.asset.source.split("\n").length} 行</span>
                          <span>Ctrl+S 保存 · Ctrl+Enter 生成</span>
                        </div>
                      </section>
                      <section className="definition-pane">
                        <div className="pane-toolbar">
                          <div className="pane-tabs">
                            <button
                              className={
                                inspector === "definition" ? "selected" : ""
                              }
                              onClick={() => setInspector("definition")}
                            >
                              参数定义
                            </button>
                            <button
                              className={
                                inspector === "defaults" ? "selected" : ""
                              }
                              onClick={() => setInspector("defaults")}
                            >
                              默认参数
                            </button>
                            <button
                              className={inspector === "bindings" ? "selected" : ""}
                              onClick={() => setInspector("bindings")}
                            >
                              共享绑定
                            </button>
                          </div>
                        </div>
                        <div className="pane-scroll">
                          {inspector === "bindings" ? (
                            <Suspense fallback={<p>正在加载共享绑定…</p>}>
                            <SharedBindingEditor
                              key={doc.key}
                              asset={doc.asset}
                              onChange={(asset) => editDoc((current) => {
                                const before = !!current.asset.metadata?.parameters;
                                const after = !!asset.metadata?.parameters;
                                return {
                                  ...current,
                                  asset,
                                  parameterSelection: undefined,
                                  replayInput: undefined,
                                  ...(before !== after ? {
                                    generationAction: after ? "template.render.managed" : "template.render",
                                  } : {}),
                                };
                              })}
                            />
                            </Suspense>
                          ) : inspector === "definition" ? (
                            <SchemaEditor
                              schema={normalizeSchema(doc.asset)}
                              onChange={(schema) =>
                                editDoc((d) => ({
                                  ...d,
                                  asset: { ...d.asset, schema },
                                }))
                              }
                              onInsert={(name) =>
                                code.current?.insert("{{ " + name + " }}")
                              }
                            />
                          ) : (
                            <>
                              <p className="hint">
                                默认值会应用到新的使用任务；不覆盖已经填写的值。
                              </p>
                              <Form
                                schema={normalizeSchema(doc.asset)}
                                value={doc.asset.defaults || {}}
                                onChange={(defaults) =>
                                  editDoc((d) => ({
                                    ...d,
                                    asset: { ...d.asset, defaults },
                                  }))
                                }
                              />
                              <button
                                className="button secondary full-width"
                                onClick={() => {
                                  editDoc(
                                    resetDocumentParameters(
                                      doc,
                                      generation?.id,
                                    ),
                                  );
                                  setJsonError("");
                                }}
                              >
                                {sharedParameters
                                  ? "清除本次覆盖并试用继承参数"
                                  : "将默认参数用于本次试运行"}
                              </button>
                            </>
                          )}
                        </div>
                      </section>
                    </>
                  ) : (
                    <section className="parameters-pane">
                      <div className="pane-toolbar">
                        <strong>本次参数</strong>
                        <div className="pane-tabs">
                          <button
                            className={paramMode === "form" ? "selected" : ""}
                            onClick={() => {
                              setParamMode("form");
                              setJsonError("");
                            }}
                          >
                            表单
                          </button>
                          <button
                            className={paramMode === "json" ? "selected" : ""}
                            onClick={() => setParamMode("json")}
                          >
                            JSON
                          </button>
                        </div>
                      </div>
                      <div className="parameter-actions">
                        <button
                          className="text-button"
                          onClick={() => void loadPreset()}
                        >
                          <FolderOpen size={13} />
                          加载预设
                        </button>
                        <button className="text-button" onClick={savePreset}>
                          <Save size={13} />
                          保存预设
                        </button>
                        {!sharedParameters && (
                          <button
                            className="text-button"
                            onClick={() => {
                              editDoc(
                                resetDocumentParameters(doc, generation?.id),
                              );
                              setJsonError("");
                            }}
                          >
                            恢复默认值
                          </button>
                        )}
                      </div>
                      {sharedParameters ? (
                        <SharedParameterInputs
                          key={doc.key}
                          doc={doc}
                          mode={paramMode}
                          onChange={(patch) => editDoc(patch)}
                          errors={errors}
                          disabled={busy}
                          jsonError={jsonError}
                          onJsonError={setJsonError}
                        />
                      ) : paramMode === "json" ? (
                        <JsonEditor
                          draft={doc.jsonInput}
                          onRawChange={(jsonInput) => editDoc({ jsonInput })}
                          value={doc.context}
                          onChange={(context) => {
                            editDoc({ context });
                            setJsonError("");
                          }}
                          onError={setJsonError}
                          label="本次参数 JSON"
                        />
                      ) : (
                        <div className="pane-scroll">
                          {doc.asset.metadata?.parameters && (
                            <ParameterSelection
                              value={
                                doc.parameterSelection ||
                                doc.asset.metadata.parameters.selection ||
                                {}
                              }
                              onChange={(parameterSelection) =>
                                editDoc({ parameterSelection })
                              }
                            />
                          )}
                          <Form
                            schema={normalizeSchema(doc.asset)}
                            value={mergeDefaults(
                              doc.asset.defaults,
                              doc.context,
                            )}
                            onChange={(context) =>
                              editDoc({ context, jsonInput: undefined })
                            }
                            errors={errors}
                          />
                          {!Object.keys(
                            normalizeSchema(doc.asset).properties || {},
                          ).length && (
                            <Empty
                              title="这个模板没有定义参数"
                              description="如果源码使用了变量，请在编辑模式识别变量并定义类型。"
                            />
                          )}
                          {errors[""] && (
                            <div className="field-message">{errors[""]}</div>
                          )}
                        </div>
                      )}
                      {!sharedParameters && jsonError && (
                        <div className="inline-error">{jsonError}</div>
                      )}
                      <div className="parameter-footer">
                        <span className="action-hint" role="status">
                          {generateHint}
                        </span>
                        <button
                          className="button primary"
                          disabled={busy}
                          title={generateHint}
                          onClick={() => void generate()}
                        >
                          <Play size={14} />
                          生成文件
                        </button>
                      </div>
                    </section>
                  )}
                  <section
                    className={
                      "result-pane " +
                      (doc.mode === "edit" ? "author-result" : "")
                    }
                  >
                    <div className="pane-toolbar">
                      <div className="pane-tabs">
                        <button
                          className={resultTab === "result" ? "selected" : ""}
                          onClick={() => setResultTab("result")}
                        >
                          生成结果
                        </button>
                        <button
                          className={
                            resultTab === "diagnostics" ? "selected" : ""
                          }
                          onClick={() => setResultTab("diagnostics")}
                        >
                          检查信息
                          {diagnostics.length > 0 && (
                            <span className="count-badge">
                              {diagnostics.length}
                            </span>
                          )}
                        </button>
                        {latest?.result?.data.parameters && (
                          <button
                            className={
                              resultTab === "parameters" ? "selected" : ""
                            }
                            onClick={() => setResultTab("parameters")}
                          >
                            参数与计算
                          </button>
                        )}
                      </div>
                      <div className="run-controls">
                        <span
                          role="status"
                          className={
                            "run-status " +
                            (latest?.status || "idle") +
                            (latest?.status === "succeeded" &&
                            (stale || inputError)
                              ? " stale"
                              : "")
                          }
                        >
                          {busy ? (
                            <LoaderCircle size={14} className="spin" />
                          ) : (latest?.status === "succeeded" &&
                              (stale || inputError)) ||
                            latest?.status === "failed" ||
                            latest?.status === "interrupted" ? (
                            <AlertTriangle size={14} />
                          ) : latest?.status === "succeeded" ? (
                            <Check size={14} />
                          ) : latest?.status === "cancelled" ? (
                            <Square size={14} />
                          ) : (
                            <FileCode2 size={14} />
                          )}
                          {submitting === doc.key
                            ? "提交中"
                            : latest?.status === "succeeded" &&
                                (stale || inputError)
                              ? "结果已过期"
                              : latest
                                ? stateNames[latest.status]
                                : "待生成"}
                        </span>
                        {latest &&
                          ["queued", "running"].includes(latest.status) && (
                            <button
                              className="text-button"
                              onClick={() =>
                                api("runs/" + latest.id + "/cancel", {}).catch(
                                  fail,
                                )
                              }
                            >
                              <Square size={13} />
                              取消任务
                            </button>
                          )}
                      </div>
                    </div>
                    {(stale || inputError) && (
                      <div className="stale-banner">
                        <AlertTriangle size={13} />
                        结果对应旧输入，请重新生成以应用当前修改。
                      </div>
                    )}
                    {resultTab === "parameters" &&
                    latest?.result?.data.parameters ? (
                      <div className="pane-scroll">
                        <pre>
                          {encode(
                            {
                              有效输入: latest?.result?.data.parameters?.values,
                              参数来源:
                                latest?.result?.data.parameters?.origins,
                              单位: Object.fromEntries(
                                Object.entries(
                                  latest?.result?.data.parameters
                                    ?.definitions || {},
                                ).map(([path, definition]: [string, Data]) => [
                                  path,
                                  definition.unit || "",
                                ]),
                              ),
                            },
                            true,
                          )}
                        </pre>
                      </div>
                    ) : resultTab === "diagnostics" ? (
                      <div className="pane-scroll">
                        {diagnostics.length ? (
                          diagnostics.map((d, i) => (
                            <button
                              className={"diagnostic " + d.level}
                              key={i}
                              onClick={() => jumpDiagnostic(d)}
                            >
                              <AlertTriangle size={15} />
                              <span>
                                {d.message}
                                {d.path && <small>{d.path}</small>}
                              </span>
                              <ChevronRight size={14} />
                            </button>
                          ))
                        ) : (
                          <Empty
                            title="没有检查问题"
                            description="生成时发现的问题会显示在这里。"
                          />
                        )}
                      </div>
                    ) : latest?.status === "succeeded" ? (
                      <Editor
                        value={String(text)}
                        readonly
                        language="text"
                        label="生成结果"
                      />
                    ) : latest &&
                      ["queued", "running"].includes(latest.status) ? (
                      <Empty
                        icon={<LoaderCircle size={28} className="spin" />}
                        title="正在生成文件"
                        description="你可以继续处理其他模板，或在结果区顶部取消任务。"
                      />
                    ) : latest &&
                      ["failed", "cancelled", "interrupted"].includes(
                        latest.status,
                      ) ? (
                      <Empty
                        icon={<AlertTriangle size={30} />}
                        title={
                          latest.status === "cancelled"
                            ? "任务已取消"
                            : latest.status === "interrupted"
                              ? "任务已中断"
                              : "生成失败"
                        }
                        description={
                          latest.error?.message ||
                          "本次任务没有生成可交付文件，请重新生成。"
                        }
                        action={
                          <button
                            className="button secondary"
                            onClick={() => setResultTab("diagnostics")}
                          >
                            查看并定位问题
                          </button>
                        }
                      />
                    ) : (
                      <Empty
                        title="结果会显示在这里"
                        description="填写参数后点击「生成文件」，查看正文和检查信息。"
                      />
                    )}
                    <div className="export-hint" role="status">
                      {exportReason || "当前结果已生成，可复制或导出文件"}
                    </div>
                    <div className="result-footer">
                      <label className="filename-field">
                        <span>文件名</span>
                        <input
                          aria-label="输出文件名"
                          value={doc.outputName}
                          onChange={(e) =>
                            editDoc({ outputName: e.target.value })
                          }
                        />
                      </label>
                      <button
                        className="icon-button"
                        aria-label="复制结果"
                        disabled={!!exportReason}
                        title={exportReason || "导出当前结果"}
                        onClick={() =>
                          navigator.clipboard
                            .writeText(String(text))
                            .then(() => notify("结果已复制"))
                            .catch(fail)
                        }
                      >
                        <Copy size={15} />
                      </button>
                      <button
                        className="button primary"
                        disabled={!!exportReason}
                        title={exportReason || "导出当前结果"}
                        onClick={() =>
                          download(
                            String(
                              latest?.result?.artifacts?.length &&
                                latest.result.artifacts.length > 1
                                ? latest.result.artifacts[0].text
                                : text,
                            ),
                            doc.outputName || "output.txt",
                          )
                        }
                      >
                        <Download size={14} />
                        导出文件
                      </button>
                      {(latest?.result?.artifacts?.length || 0) > 1 &&
                        latest!.result!.artifacts!.map((artifact, i) => (
                          <button
                            key={i}
                            className="button secondary"
                            disabled={!!exportReason}
                            onClick={() =>
                              download(artifact.text, artifact.name)
                            }
                          >
                            导出 {artifact.name}
                          </button>
                        ))}
                    </div>
                  </section>
                </div>
              </>
            )}
          </>
        ) : page === "history" ? (
          <Suspense fallback={<div className="center-loading">正在加载页面…</div>}>
            <HistoryPage
            actions={capabilities.actions}
            runList={runList}
            selectedHistory={selectedHistory}
            comparison={comparison}
            setRunList={setRunList}
            setSelectedHistory={setSelectedHistory}
            setComparison={setComparison}
            restoreRun={restoreRun}
            onCreated={(id, original) =>
              setRuns((current) => ({
                ...current,
                [id]: {
                  ...original,
                  id,
                  status: "queued",
                  result: undefined,
                  error: undefined,
                  environment: undefined,
                  replay: undefined,
                },
              }))
            }
            notify={notify}
            fail={fail}
          />
          </Suspense>
        ) : page === "syntax-guide" ? (
          <Suspense fallback={<div className="center-loading">正在加载页面…</div>}>
            <SyntaxGuide
              capabilities={capabilities}
              back={() => setPage("workbench")}
              notify={notify}
            />
          </Suspense>
        ) : page === "plugins" ? (
          <Suspense fallback={<div className="center-loading">正在加载页面…</div>}>
            <PluginSettings
              inventory={inventory}
              reload={() => api("plugins").then(setInventory)}
              notify={notify}
              fail={fail}
            />
          </Suspense>
        ) : ContributedView ? (
          <Suspense
            fallback={<div className="center-loading">正在加载插件工作台…</div>}
          >
            <ContributedView
              key={(workspace?.id || "") + ":" + page}
              panel={capabilities.panels.find((p: Data) => p.id === page)}
              capabilities={capabilities}
              start={runAction}
              runs={runs}
              fail={fail}
              workspaceId={workspace?.id || ""}
            />
          </Suspense>
        ) : (
          <Suspense fallback={<div className="center-loading">正在加载页面…</div>}>
            <PluginPage
              key={(workspace?.id || "") + ":" + page}
              workspaceId={workspace?.id || ""}
              panel={capabilities.panels.find((p: Data) => p.id === page)}
              capabilities={capabilities}
              start={runAction}
              runs={runs}
              fail={fail}
            />
          </Suspense>
        )}
        <footer className="status-bar">
          <span>
            <span className="online-dot" />
            本地服务已连接
          </span>
          <span>
            草稿自动恢复 ·{" "}
            {running.length
              ? `${running.length} 个任务运行中`
              : "所有任务已结束"}
          </span>
          {running.length > 0 && (
            <button onClick={() => setPage("history")}>
              查看任务
              <ChevronRight size={12} />
            </button>
          )}
        </footer>
      </main>
      <input
        className="hidden"
        type="file"
        accept=".json,.j2,.jinja2,.txt"
        ref={imported}
        onChange={(e) => {
          const file = e.target.files?.[0];
          if (file) void importFile(file);
          e.target.value = "";
        }}
      />
      <WorkspaceImport
        picker={bundleInput}
        refreshCatalog={catalog}
        notify={notify}
        fail={fail}
      />
      {dialog && (
        <Modal
          title={dialog.title}
          buttons={dialog.buttons}
          onClose={() => setDialog(null)}
        >
          {dialog.body}
        </Modal>
      )}
    </div>
  );
}
