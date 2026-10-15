import { useRef, useState } from "react";
import { type EditorHandle } from "../../components/Editor";
import { type HistoryComparison } from "../../components/HistoryPage";
import { action, api, ApiError } from "../../lib/api";
import {
  clone,
  diagnosticsFrom,
  encode,
  fieldErrors,
  mergeDefaults,
  parseData,
  type Data,
  type Diagnostic,
  type Document,
  type Run,
} from "../../lib/data";
import {
  activePanels,
  documentActions,
  documentInput,
  documentSignature,
  restoreDocument,
  retiredAction,
  sameSchema,
  selectDocumentAction,
} from "../../lib/documentExecution";
import { useManagedDependencies } from "../../lib/managedDependencies";
import { loadTemplateCatalog } from "../../lib/templateCatalog";
import { builtinViews } from "../../plugins/registry";

import { toast } from "sonner";
import { normalizeSchema, type Dialog } from "./model";
import {
  useDocumentOperations,
  useParameterPresets,
} from "./useDocumentOperations";
import { useWorkspaceSession } from "./useWorkspaceSession";
import { useWorkspaceState } from "./workspaceStore";
export { normalizeSchema, stateNames } from "./model";
export type { Item } from "./model";
interface DiscoverOutcome {
  stale: boolean;
  added: number;
}
export function useWorkbenchController() {
  const {
    workspace,
    setWorkspace,
    capabilities,
    setCapabilities,
    inventory,
    setInventory,
    items,
    setItems,
    documents,
    setDocuments,
    active,
    setActive,
    page,
    setPage,
    runList,
    setRunList,
    runs,
    setRuns,
  } = useWorkspaceState();
  const [query, setQuery] = useState(""),
    [tag, setTag] = useState(""),
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
    [recovered, setRecovered] = useState(false),
    [errors, setErrors] = useState<Record<string, string>>({});
  const [selectedHistory, setSelectedHistory] = useState<Run | null>(null);
  const [inspector, setInspector] = useState<"definition" | "defaults">(
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
  const visibleDocuments = documents;
  const doc = documents.find((d) => d.key === active);
  const panels = activePanels(capabilities.panels, capabilities.actions);
  const parameterPanel = panels.find(
    (panel: Data) => panel.view === "parameter.library",
  );
  const externalPanels = panels.filter(
    (panel: Data) => panel.view !== "parameter.library",
  );
  const documentChoices = documentActions(capabilities.actions);
  const selection = doc
    ? selectDocumentAction(
        doc,
        capabilities.actions,
        capabilities.document_actions,
      )
    : {};
  const generation = selection.entry;
  const latest = doc?.lastRunId ? runs[doc.lastRunId] : undefined;
  const managedDependencies = useManagedDependencies(
    latest,
    page === "workbench",
    workspace?.id || "",
  );
  const running = Object.values(runs).filter((run) =>
    ["queued", "running"].includes(run.status),
  );
  const currentPanel =
    page === "parameters"
      ? parameterPanel
      : panels.find((panel: Data) => panel.id === page);
  const contributedView = currentPanel?.view;
  const ContributedView = builtinViews[contributedView];
  const stale = !!(
    doc &&
    latest &&
    (managedDependencies.stale ||
      doc.lastRunSignature !== documentSignature(doc, generation?.id))
  );
  // Generation and save shortcuts belong to the template workbench and its
  // shared parameter surface only. Every other page renders its own views
  // (history, plugin workbenches, syntax guide) and must keep its own keys.
  const scopedShortcuts = page === "workbench" || page === "parameters";
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
    const message = <span role={error ? "alert" : "status"}>{text}</span>;
    if (error) toast.error(message, { duration: 10000 });
    else toast.success(message);
  }
  function fail(error: Data) {
    notify(error.message || "操作失败", true);
  }
  async function catalog() {
    setCatalogState("loading");
    setCatalogError("");
    try {
      const result = await loadTemplateCatalog();
      setItems(result.items);
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
  const {
    saving,
    openTemplate,
    useTemplate,
    create,
    closeDocument,
    saveDocument,
    copyTemplate,
    deleteTemplate,
    importFile,
  } = useDocumentOperations({
    doc,
    documents,
    active,
    generation,
    editDoc,
    setDocuments,
    setActive,
    setPage,
    setDialog,
    setErrors,
    setJsonError,
    setParamMode,
    catalog,
    notify,
    fail,
  });
  const { savePreset, loadPreset } = useParameterPresets({
    doc,
    editDoc,
    setDialog,
    notify,
    fail,
  });
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
  async function discover() {
    if (!doc) return;
    // Inspection describes exactly the source and schema captured here. A late
    // response must never write back a version the user has already left, and
    // must not drop definitions added while it was in flight.
    const request = {
      key: doc.key,
      source: doc.asset.source,
      schema: clone(normalizeSchema(doc.asset)),
    };
    try {
      const inspected = await action("template.inspect", {
        source: doc.asset.source,
        ...(doc.id ? { template: doc.id } : {}),
      });
      const outcome: DiscoverOutcome = { stale: false, added: 0 };
      editDoc((d) => {
        if (
          d.key !== request.key ||
          d.asset.source !== request.source ||
          !sameSchema(request.schema, normalizeSchema(d.asset))
        ) {
          outcome.stale = true;
          return d;
        }
        const schema = clone(normalizeSchema(d.asset));
        const declared = new Set(
          Object.keys(request.schema.properties || {}),
        );
        const added = new Set<string>();
        for (const v of inspected.data.variables) {
          if (!schema.properties![v.name]) {
            schema.properties![v.name] = { title: v.name, "x-untyped": true };
            if (!declared.has(v.name)) added.add(v.name);
          }
          if (!v.optional && !schema.required!.includes(v.name))
            schema.required!.push(v.name);
        }
        outcome.added = added.size;
        if (!added.size && sameSchema(schema, d.asset)) return d;
        return { ...d, asset: { ...d.asset, schema } };
      }, request.key);
      setInspector("definition");
      if (outcome.stale)
        notify("源码或规格已修改，本次识别结果已忽略，请重新识别", true);
      else
        notify(
          outcome.added
            ? `发现 ${outcome.added} 个新参数，请选择类型`
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
    // the same condition, so a shortcut must never slip a second task past it
    // while this document's previous task is still queued or running.
    if (busy) {
      notify(busyHint, true);
      return;
    }
    if (!generation) {
      notify(selection.error || "请选择生成方式", true);
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
          mergeDefaults(doc.asset.defaults, doc.context),
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
      const input = documentInput(doc, generation);
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
      if (retiredAction(id)) throw new Error("此加工动作已退役，不能执行。");
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
  async function restoreRun(run: Run) {
    try {
      const restored = restoreDocument(run, capabilities.actions);
      setDocuments((d) => [...d, restored]);
      setActive(restored.key);
      setRuns((r) => ({ ...r, [run.id]: run }));
      setPage("workbench");
      setJsonError("");
      setErrors({});
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
  if (doc?.jsonInput !== undefined) {
    try {
      parseData(doc.jsonInput);
    } catch {
      inputError = "请先修正参数 JSON，再生成文件";
    }
  }
  if (doc) {
    try {
      encode(doc.context);
    } catch {
      inputError = "请先填写完整、有效的数值";
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

  useWorkspaceSession({
    workspace,
    documents,
    active,
    runs,
    doc,
    saving,
    capabilities,
    catalog,
    fail,
    notify,
    saveDocument,
    generate,
    setWorkspace,
    setCapabilities,
    setInventory,
    setSidebarCollapsed,
    setPaneRatio,
    setDocuments,
    setActive,
    setRecovered,
    setRuns,
    setRunList,
    setLoading,
    scopedShortcuts,
  });
  return {
    runs,
    runList,
    parameterPanel,
    workspace,
    capabilities,
    inventory,
    items,
    documents,
    active,
    page,
    setPage,
    query,
    setQuery,
    tag,
    setTag,
    dialog,
    setDialog,
    sidebarCollapsed,
    catalogState,
    catalogError,
    comparison,
    setComparison,
    bundleInput,
    paneRatio,
    submitting,
    loading,
    saving,
    recovered,
    setRecovered,
    errors,
    selectedHistory,
    setSelectedHistory,
    inspector,
    setInspector,
    paramMode,
    setParamMode,
    jsonError,
    setJsonError,
    resultTab,
    setResultTab,
    code,
    jump,
    imported,
    visibleDocuments,
    doc,
    externalPanels,
    documentChoices,
    selection,
    generation,
    latest,
    managedDependencies,
    running,
    currentPanel,
    ContributedView,
    stale,
    resizePane,
    notify,
    fail,
    catalog,
    editDoc,
    toggleSidebar,
    openTemplate,
    useTemplate,
    create,
    closeDocument,
    saveDocument,
    copyTemplate,
    deleteTemplate,
    importFile,
    discover,
    generate,
    runAction,
    jumpDiagnostic,
    savePreset,
    loadPreset,
    restoreRun,
    visible,
    tags,
    diagnostics,
    text,
    busy,
    inputError,
    exportReason,
    generateHint,
    setRunList,
    setRuns,
    setInventory,
    setActive,
    setErrors,
  };
}
export type WorkbenchController = ReturnType<typeof useWorkbenchController>;
