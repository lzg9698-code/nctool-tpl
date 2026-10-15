import { ChevronRight, FileCode2 } from "lucide-react";
import { useState } from "react";
import { Empty } from "../../components/WorkbenchUi";
import { action, ApiError } from "../../lib/api";
import {
  clone,
  defaultValue,
  documentValue,
  isDirty,
  mergeDefaults,
  newDocument,
  parseData,
  type Asset,
  type Data,
  type Document,
} from "../../lib/data";
import { cloneDraft } from "../../lib/drafts";
import {
  normalizeSchema,
  prepareDocument,
  type Dialog,
  type Item,
} from "./model";
type DocumentUpdate = Partial<Document> | ((doc: Document) => Document);
interface ParameterPresetsProps {
  doc: Document | undefined;
  editDoc: (update: DocumentUpdate, key?: string | null) => void;
  setDialog: (dialog: Dialog | null) => void;
  notify: (text: string, error?: boolean) => void;
  fail: (error: Data) => void;
}
interface DocumentOperationsProps extends ParameterPresetsProps {
  documents: Document[];
  active: string | null;
  generation: { id: string } | undefined;
  setDocuments: (
    update: Document[] | ((docs: Document[]) => Document[]),
  ) => void;
  setActive: (key: string | null) => void;
  setPage: (page: string) => void;
  setErrors: (errors: Record<string, string>) => void;
  setJsonError: (error: string) => void;
  setParamMode: (mode: "form" | "json") => void;
  catalog: () => Promise<void>;
}

export function useDocumentOperations({
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
}: DocumentOperationsProps) {
  const [saving, setSaving] = useState(false);

  async function openTemplate(item: Item) {
    const existing = documents.find((d) => d.id === item.id);
    if (existing) {
      setActive(existing.key);
      setPage("workbench");
      return;
    }
    try {
      const result = await action("template.read", { name: item.id });
      const loaded = prepareDocument(
        item.id,
        result.data.asset,
        result.data.fingerprint,
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
        setActive(documents.find((d) => d.key !== key)?.key || null);
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
      const asset = {
        ...doc.asset,
        metadata: {
          ...(doc.asset.metadata || {}),
          title: doc.title,
          output_name: doc.outputName,
          ...(generation
            ? {
                execution: {
                  ...(doc.asset.metadata?.execution || {}),
                  action: generation.id,
                  ...(doc.extensions !== undefined
                    ? { extensions: doc.extensions }
                    : {}),
                },
              }
            : {}),
        },
      };
      const response = await action("template.save", {
        name: id,
        asset,
        expected: doc.fingerprint,
      });
      const savedDoc = {
        ...doc,
        id,
        asset,
        fingerprint: response.data.fingerprint,
      };
      savedDoc.saved = documentValue(savedDoc);
      setDocuments((current) =>
        current.map((d) =>
          d.key === savingKey
            ? {
                ...d,
                id,
                fingerprint: response.data.fingerprint,
                saved: savedDoc.saved,
                asset:
                  documentValue(d) === documentValue(doc) ? asset : d.asset,
              }
            : d,
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
            <>
              <p>当前草稿保留。请比较两个版本，再决定如何处理。</p>
              <div className="conflict-columns">
                <div>
                  <strong>当前草稿</strong>
                  <pre>{doc.asset.source}</pre>
                </div>
                <div>
                  <strong>磁盘版本</strong>
                  <pre>{response?.data.asset.source}</pre>
                </div>
              </div>
            </>
          ),
          buttons: (
            <>
              <button
                className="button secondary"
                onClick={() => setDialog(null)}
              >
                继续编辑
              </button>
              <button
                className="button secondary"
                onClick={() => {
                  if (response && doc.id) {
                    editDoc(
                      (current) => ({
                        ...prepareDocument(
                          doc.id!,
                          response.data.asset,
                          response.data.fingerprint,
                        ),
                        key: savingKey,
                        mode: doc.mode,
                        tab: doc.tab,
                        context: doc.context,
                        lastRunId: current.lastRunId,
                        lastRunSignature: current.lastRunSignature,
                      }),
                      savingKey,
                    );
                  }
                  setDialog(null);
                }}
              >
                加载磁盘版本
              </button>
              <button
                className="button primary"
                onClick={() => {
                  const copy = {
                    ...doc,
                    key: crypto.randomUUID(),
                    id: null,
                    fingerprint: null,
                    title: doc.title + " 副本",
                    saved: "",
                    lastRunId: undefined,
                    lastRunSignature: undefined,
                  };
                  setDocuments((d) => [...d, copy]);
                  setActive(copy.key);
                  setDialog(null);
                }}
              >
                另存为副本
              </button>
            </>
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
        );
        copied.mode = "edit";
        setDocuments((d) => [...d, copied]);
        setActive(copied.key);
      } else {
        const copied = {
          ...cloneDraft(doc),
          key: crypto.randomUUID(),
          id: null,
          fingerprint: null,
          title: doc.title + " 副本",
          saved: "",
          lastRunId: undefined,
          lastRunSignature: undefined,
        };
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
      const draft = newDocument();
      draft.asset = asset;
      draft.title =
        asset.metadata?.title || file.name.replace(/\.(json|j2|jinja2)$/, "");
      draft.context = asset.metadata?.parameters
        ? {}
        : mergeDefaults(defaultValue(normalizeSchema(asset)), asset.defaults);
      setDocuments((d) => [...d, draft]);
      setActive(draft.key);
      setPage("workbench");
      notify("已导入为草稿，保存后进入模板库");
    } catch (error) {
      fail(error);
    }
  }

  return {
    saving,
    openTemplate,
    useTemplate,
    create,
    closeDocument,
    saveDocument,
    copyTemplate,
    deleteTemplate,
    importFile,
  };
}

export function useParameterPresets({
  doc,
  editDoc,
  setDialog,
  notify,
  fail,
}: ParameterPresetsProps) {
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
                    context: doc.context,
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
                    editDoc({
                      context: clone(entry.context),
                      jsonInput: undefined,
                    });
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

  return { savePreset, loadPreset };
}
