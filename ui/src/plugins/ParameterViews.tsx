import { useEffect, useRef, useState } from "react";
import { action } from "../lib/api";
import { clone, type Data } from "../lib/data";
import { encodeDraft, decodeDraft } from "../lib/drafts";
import { JsonEditor, PageTitle } from "../components/WorkbenchUi";
import { DefinitionEditor, PresetEditor } from "../components/ParameterAssetEditor";
import { catalogLabel } from "../lib/managedParameters";
import type { ViewProps } from "./ViewProps";

const titles: Record<string, string> = {
  definitions: "参数定义",
  presets: "共享预置",
};
function seed(group: string): Data {
  const fields: Record<string, Data> = {
    definitions: {
      schema: { type: "string" },
      unit: "",
      group: "",
      description: "",
    },
    presets: { values: {} },
  };
  return { format_version: 1, id: "", title: "", ...fields[group] };
}
export function ParameterLibrary(props: ViewProps) {
  const groups = ["definitions", "presets"];
  const key = `nctool:parameter-draft:${props.workspaceId}:parameter`;
  const [draft, update] = useState<Data>(() => {
    try {
      const saved: Data = decodeDraft(localStorage.getItem(key) || "");
      if (groups.includes(saved.group) && saved.asset) return saved;
    } catch {}
    return { group: groups[0], asset: seed(groups[0]), expected: null };
  });
  // The catalog is stored together with the group it was loaded for, so a
  // response that belongs to another type can never be shown or read back.
  const [catalog, setCatalog] = useState<{ group: string; items: Data[] }>({
    group: draft.group,
    items: [],
  });
  const [definitions, setDefinitions] = useState<Data[]>([]);
  const [query, setQuery] = useState("");
  const [message, setMessage] = useState("");
  const current = useRef(draft);
  const identity = useRef(0);
  const mounted = useRef(false);
  const storageKey = useRef(key);
  storageKey.current = key;
  const requestId = useRef(0);
  const savingRef = useRef(false);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  function patch(next: Data, replace = false) {
    if (
      replace ||
      next.group !== current.current.group ||
      next.asset?.id !== current.current.asset?.id
    )
      identity.current++;
    current.current = next;
    update(next);
    localStorage.setItem(key, encodeDraft(next));
    setMessage("");
  }
  /** Edit the asset of the current draft without touching group, raw or fingerprint. */
  function patchAsset(fields: Data) {
    const asset = { ...current.current.asset, ...fields };
    for (const field of Object.keys(fields))
      if (fields[field] === undefined) delete asset[field];
    // The structured edit supersedes any unresolved raw JSON, so drop the key
    // rather than storing undefined: a tagged draft would round-trip it as set.
    const next = { ...current.current, asset };
    delete next.raw;
    patch(next);
  }
  async function reload(guard: () => boolean = () => true) {
    const group = draft.group;
    // Each request carries its own identity and the group it was issued for, so
    // a slower response from another type is dropped instead of being shown
    // under, or read back into, the group the user has switched to.
    const mine = ++requestId.current;
    const result = await action("parameter.catalog", { group });
    if (guard() && mine === requestId.current)
      setCatalog({ group, items: result.data.items });
  }
  // Presets reference definitions by id, so the editor needs the definition list
  // even while the preset group is the one being edited.
  async function loadDefinitions() {
    try {
      const result = await action("parameter.catalog", { group: "definitions" });
      if (mounted.current) setDefinitions(result.data.items);
    } catch {
      // A missing definition catalog only degrades the preset editor's labels;
      // existing stored values stay visible and editable.
    }
  }
  useEffect(() => {
    // Switching group first drops any catalog that does not belong to it, then
    // reloads; only a response for the group still in flight can be adopted.
    setCatalog((previous) =>
      previous.group === draft.group ? previous : { group: draft.group, items: [] },
    );
    setQuery("");
    void reload().catch(props.fail);
  }, [draft.group]);
  useEffect(() => {
    void loadDefinitions();
  }, []);
  // Reading and rendering only ever see the catalog of the group being edited.
  const items = catalog.group === draft.group ? catalog.items : [];
  const visible = items.filter((item) =>
    query.trim() ? catalogLabel(item).toLowerCase().includes(query.trim().toLowerCase()) : true,
  );
  const stored = !!draft.expected;
  async function save() {
    // The ref also gates clicks before React has rendered the disabled button.
    if (savingRef.current) return;
    const submitted = current.current;
    const submittedIdentity = identity.current;
    const submittedKey = key;
    const isCurrent = () =>
      mounted.current &&
      storageKey.current === submittedKey &&
      identity.current === submittedIdentity;
    savingRef.current = true;
    setSaving(true);
    try {
      if (submitted.raw !== undefined)
        throw new Error("请先修复资产 JSON 草稿");
      if (!submitted.asset.id) throw new Error("请填写稳定资产 ID");
      const result = await action("parameter.save", {
        group: submitted.group,
        name: submitted.asset.id,
        asset: submitted.asset,
        expected: submitted.expected,
      });
      if (!isCurrent()) return;
      const latest = current.current;
      if (
        latest.group !== submitted.group ||
        latest.asset.id !== submitted.asset.id
      )
        return;
      // Advance the saved asset's base without replacing later input (including raw JSON).
      // Keep editor validation messages when the user has continued editing.
      const next = { ...latest, expected: result.data.fingerprint };
      current.current = next;
      update(next);
      localStorage.setItem(key, encodeDraft(next));
      if (latest === submitted) setMessage("已保存");
      await reload(isCurrent);
      if (submitted.group === "definitions") void loadDefinitions();
    } catch (e) {
      if (!isCurrent()) return;
      if (current.current === submitted) {
        setMessage(
          (e as Error).message ||
            "保存失败；草稿已保留，请重新读取并核对冲突。",
        );
      }
      props.fail(e);
    } finally {
      savingRef.current = false;
      if (mounted.current) setSaving(false);
    }
  }
  return (
    <div className="page-content">
      <PageTitle
        title="共享参数"
        description="定义与预置独立保存。本次输入和生成结果不会修改共享资产。"
      />
      <div className="parameter-library">
        <section className="panel parameter-list">
          <div className="panel-header">
            <label className="parameter-list-type">
              资产类型
              <select
                aria-label="资产类型"
                value={draft.group}
                onChange={(e) =>
                  patch(
                    {
                      group: e.target.value,
                      asset: seed(e.target.value),
                      expected: null,
                    },
                    true,
                  )
                }
              >
                {groups.map((g) => (
                  <option key={g} value={g}>
                    {titles[g]}
                  </option>
                ))}
              </select>
            </label>
            <button
              className="button secondary"
              onClick={() =>
                patch(
                  {
                    group: draft.group,
                    asset: seed(draft.group),
                    expected: null,
                  },
                  true,
                )
              }
            >
              新建资产
            </button>
          </div>
          <div className="panel-content">
            <div className="toolbar">
              <input
                aria-label="搜索资产"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="搜索名称或标识"
              />
              <button
                className="button secondary"
                onClick={() => void reload().catch(props.fail)}
              >
                刷新目录
              </button>
            </div>
            <div className="parameter-list-items" role="list">
              {visible.map((item) => (
                <button
                  type="button"
                  role="listitem"
                  key={item.id}
                  className={
                    "parameter-list-item " +
                    (draft.asset.id === item.id && stored ? "selected" : "")
                  }
                  aria-label={`读取资产 ${item.id}`}
                  onClick={() =>
                    // Only the catalog loaded for the current group can be read
                    // back, so a stale entry from another type can never be
                    // loaded as draft.
                    patch(
                      {
                        group: draft.group,
                        asset: clone(item.asset),
                        expected: item.fingerprint,
                      },
                      true,
                    )
                  }
                >
                  <strong>{item.title || item.id}</strong>
                  <code>{item.id}</code>
                </button>
              ))}
              {!visible.length && (
                <p className="parameter-hint">
                  {items.length ? "没有匹配的资产。" : `还没有${titles[draft.group]}，点击“新建资产”开始。`}
                </p>
              )}
            </div>
          </div>
        </section>
        <section className="panel parameter-editor">
          <div className="panel-header">
            <strong>
              {draft.asset.id || "新资产"}
              {stored ? "" : "（未保存）"}
            </strong>
            <span className="parameter-hint">{titles[draft.group]}</span>
          </div>
          <div className="panel-content">
            {draft.group === "definitions" ? (
              <DefinitionEditor asset={draft.asset} stored={stored} onChange={patchAsset} />
            ) : (
              <PresetEditor
                asset={draft.asset}
                stored={stored}
                definitions={definitions}
                onChange={patchAsset}
              />
            )}
            <p className="parameter-hint">
              高级 JSON 直接编辑完整资产；表单与 JSON 表示同一份内容。
            </p>
            <JsonEditor
              editorKey={`${key}:${draft.group}:${identity.current}`}
              label="资产内容"
              value={draft.asset}
              draft={draft.raw}
              onRawChange={(raw) => patch({ ...draft, raw })}
              onChange={(asset) => patch({ ...draft, asset, raw: undefined })}
              onError={setMessage}
            />
          </div>
          <div className="panel-footer">
            <button
              className="button primary"
              disabled={saving || draft.raw !== undefined}
              onClick={() => void save()}
            >
              保存资产
            </button>
            <button
              className="button secondary"
              disabled={saving || !draft.expected}
              onClick={async () => {
                try {
                  await action("parameter.remove", {
                    group: draft.group,
                    name: draft.asset.id,
                    expected: draft.expected,
                  });
                  patch({
                    group: draft.group,
                    asset: seed(draft.group),
                    expected: null,
                  });
                  await reload();
                  if (draft.group === "definitions") void loadDefinitions();
                } catch (e) {
                  props.fail(e);
                }
              }}
            >
              删除当前资产
            </button>
          </div>
          {message && (
            <div className="panel-footer">
              <p role="status">{message}</p>
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

export function ParameterSelection({
  value = {},
  onChange,
}: {
  value?: Data;
  onChange: (value: Data) => void;
}) {
  const [presets, setPresets] = useState<Data[]>([]);
  const [error, setError] = useState("");
  useEffect(() => {
    let alive = true;
    void action("parameter.catalog", { group: "presets" })
      .then((result) => alive && setPresets(result.data.items))
      .catch((e) => alive && setError((e as Error).message));
    return () => {
      alive = false;
    };
  }, []);
  function patch(next: Data) {
    onChange(next);
    setError("");
  }
  return (
    <div className="parameter-selection">
      <label>
        共享预置
        <select
          aria-label="共享预置"
          value={Object.hasOwn(value, "preset_id") ? value.preset_id ?? "__none" : ""}
          onChange={(e) => {
            const next = { ...value };
            if (e.target.value === "__none") next.preset_id = null;
            else if (e.target.value) next.preset_id = e.target.value;
            else delete next.preset_id;
            patch(next);
          }}
        >
          <option value="">跟随模板共享预置</option>
          <option value="__none">不使用共享预置</option>
          {presets.map((p) => (
            <option value={p.id} key={p.id}>
              {p.title}
            </option>
          ))}
        </select>
      </label>
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
