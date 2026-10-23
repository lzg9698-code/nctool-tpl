import { createStore } from "zustand/vanilla";
import { persist, createJSONStorage } from "zustand/middleware";
import type { StateStorage } from "zustand/middleware";
import { encode, decode, merge, defaultFor, type Data } from "../lib/json";

export interface Draft {
  key: string;
  id: string | null;
  title: string;
  source: string;
  schema: string;
  defaults: string;
  metadata: string;
  tags: string;
  extra: string;
  parameters: string;
  invalidNumbers: Record<string, string>;
  fingerprint: string | null;
  saved: string;
  action: string;
  extensions: string[];
  preset: string | null | undefined;
  mode: "use" | "edit";
  tab: string;
  parameterMode: "form" | "json";
  outputName: string;
  runId?: string;
  runSignature?: string;
  replay?: string;
}
export interface AuxiliaryDraft {
  raw: string;
  expected: string | null;
  saved: string;
  invalid?: Record<string, string>;
  runId?: string;
  runSignature?: string;
}
export interface Session {
  documents: Draft[];
  active: string | null;
  storageError: string;
  tasks: string[];
  track: (id: string) => void;
  auxiliary: Record<string, AuxiliaryDraft>;
  add: (draft: Draft) => void;
  patch: (key: string, values: Partial<Draft>) => void;
  close: (key: string) => void;
  activate: (key: string) => void;
  setAuxiliary: (key: string, draft: AuxiliaryDraft) => void;
}
export function assetOf(draft: Draft): Data {
  const schema = decode(draft.schema),
    defaults = decode(draft.defaults),
    metadata = decode(draft.metadata),
    tags = decode(draft.tags);
  if (
    !(
      typeof schema === "boolean" ||
      (schema && typeof schema === "object" && !Array.isArray(schema))
    )
  )
    throw new Error("参数定义必须是 Schema 对象或布尔值");
  if (!metadata || typeof metadata !== "object" || Array.isArray(metadata))
    throw new Error("元数据必须是对象");
  if (!Array.isArray(tags) || tags.some((tag) => typeof tag !== "string"))
    throw new Error("标签必须是字符串数组");
  return {
    ...decode(draft.extra),
    source: draft.source,
    schema,
    defaults,
    tags,
    metadata: {
      ...metadata,
      title: draft.title,
      output_name: draft.outputName,
      execution: {
        ...metadata.execution,
        action: draft.action || metadata.execution?.action,
        extensions: draft.extensions,
      },
    },
  };
}
export function assetSignature(draft: Draft): string {
  return JSON.stringify([
    draft.title,
    draft.source,
    draft.schema,
    draft.defaults,
    draft.metadata,
    draft.tags,
    draft.extra,
    draft.action,
    draft.extensions,
    draft.outputName,
  ]);
}
export function signature(draft: Draft): string {
  return JSON.stringify([
    assetSignature(draft),
    draft.parameters,
    draft.invalidNumbers,
    draft.preset,
    draft.replay,
  ]);
}
export function dirty(draft: Draft): boolean {
  return !draft.id || assetSignature(draft) !== draft.saved;
}
export function makeDraft(
  asset: Data = {},
  id: string | null = null,
  fingerprint: string | null = null,
  selectedAction?: string,
): Draft {
  const action =
    selectedAction ||
    asset.metadata?.execution?.action ||
    (asset.metadata?.parameters
      ? "template.render.managed"
      : "template.render");
  const draft: Draft = {
    key: crypto.randomUUID(),
    id,
    fingerprint,
    title: asset.metadata?.title || id || "未命名模板",
    source: asset.source || "",
    schema: encode(asset.schema ?? { type: "object", properties: {} }, true),
    defaults: encode(asset.defaults ?? {}, true),
    metadata: encode(asset.metadata ?? {}, true),
    tags: encode(asset.tags ?? []),
    extra: encode(
      Object.fromEntries(
        Object.entries(asset).filter(
          ([key]) =>
            !["source", "schema", "defaults", "metadata", "tags"].includes(key),
        ),
      ),
    ),
    parameters: encode(
      action === "template.render.managed"
        ? {}
        : merge(defaultFor(asset.schema ?? {}), asset.defaults || {}),
      true,
    ),
    invalidNumbers: {},
    saved: "",
    action,
    extensions: asset.metadata?.execution?.extensions || [],
    preset: undefined,
    mode: id ? "use" : "edit",
    tab: "source",
    parameterMode: "form",
    outputName: asset.metadata?.output_name || "output.txt",
  };
  draft.saved = id ? assetSignature(draft) : "";
  return draft;
}
// Only the new namespace is ever read. Draft fields are raw text, so ordinary JSON
// persistence cannot coerce a large integer or an incomplete numeric input.
export function createSession(workspace: string, storage?: StateStorage) {
  let failed = "";
  let writable = true;
  let report: ((message: string) => void) | undefined;
  const guarded: StateStorage = {
    getItem: (key) => {
      try {
        return (storage || localStorage).getItem(key);
      } catch {
        failed = "无法读取新版浏览器草稿";
        return null;
      }
    },
    setItem: (key, value) => {
      if (!writable) return;
      try {
        (storage || localStorage).setItem(key, value);
      } catch {
        failed = "浏览器草稿写入失败，请及时保存资产";
        queueMicrotask(() => report?.(failed));
      }
    },
    removeItem: (key) => {
      try {
        (storage || localStorage).removeItem(key);
      } catch {
        /* no migration or clearing */
      }
    },
  };
  const store = createStore<Session>()(
    persist(
      (set) => ({
        documents: [],
        active: null,
        auxiliary: {},
        storageError: "",
        tasks: [],
        track: (id) =>
          set((state) => ({ tasks: [...new Set([...state.tasks, id])] })),
        add: (draft) =>
          set((state) => ({
            documents: state.documents.some((item) => item.key === draft.key)
              ? state.documents
              : [...state.documents, draft],
            active: draft.key,
          })),
        patch: (key, values) =>
          set((state) => ({
            documents: state.documents.map((doc) =>
              doc.key === key ? { ...doc, ...values } : doc,
            ),
          })),
        close: (key) =>
          set((state) => ({
            documents: state.documents.filter((doc) => doc.key !== key),
            active:
              state.active === key
                ? state.documents.find((doc) => doc.key !== key)?.key || null
                : state.active,
          })),
        activate: (active) => set({ active }),
        setAuxiliary: (key, draft) =>
          set((state) => ({ auxiliary: { ...state.auxiliary, [key]: draft } })),
      }),
      {
        name: "nctool:rewrite:v1:" + workspace,
        version: 1,
        storage: createJSONStorage(() => guarded),
        partialize: (state) => ({
          documents: state.documents,
          active: state.active,
          auxiliary: state.auxiliary,
          tasks: state.tasks,
        }),
        merge: (persisted, current) => {
          if (!persisted) return current;
          const value = persisted as Partial<Session>;
          if (
            !Array.isArray(value.documents) ||
            !value.documents.every(
              (doc) =>
                doc &&
                typeof doc.key === "string" &&
                [
                  "source",
                  "schema",
                  "metadata",
                  "defaults",
                  "parameters",
                  "extra",
                  "tags",
                ].every((key) => typeof doc[key as keyof Draft] === "string"),
            ) ||
            !Array.isArray(value.tasks) ||
            !value.tasks.every((task) => typeof task === "string") ||
            !value.auxiliary ||
            typeof value.auxiliary !== "object"
          ) {
            writable = false;
            failed =
              "新版草稿格式无效，原始存储保留；当前编辑无法自动保存，请及时保存资产";
            return current;
          }
          return { ...current, ...value };
        },
        onRehydrateStorage: () => (_state, error) => {
          if (error) {
            writable = false;
            failed = "新版草稿无法读取，原始存储保留；请及时保存资产";
          }
        },
      },
    ),
  );
  report = (storageError) => {
    if (store.getState().storageError !== storageError)
      store.setState({ storageError });
  };
  if (failed) store.setState({ storageError: failed });
  return store;
}
export type SessionStore = ReturnType<typeof createSession>;
