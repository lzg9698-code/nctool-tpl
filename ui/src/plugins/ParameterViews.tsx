import { useEffect, useRef, useState } from "react";
import { action } from "../lib/api";
import { clone, type Data } from "../lib/data";
import { encodeDraft, decodeDraft } from "../lib/drafts";
import { JsonEditor, PageTitle } from "../components/WorkbenchUi";
import type { ViewProps } from "./ViewProps";

const titles: Record<string, string> = {
  definitions: "参数定义",
  presets: "共享预置",
};
const groups = ["definitions", "presets"];
function seed(group: string): Data {
  const fields: Record<string, Data> = {
    definitions: {
      schema: { type: "number" },
      unit: "",
      group: "",
      description: "",
    },
    presets: { values: {} },
  };
  return { format_version: 1, id: "", title: "", ...fields[group] };
}
export function ParameterLibrary(props: ViewProps) {
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
  useEffect(() => {
    // Switching group first drops any catalog that does not belong to it, then
    // reloads; only a response for the group still in flight can be adopted.
    setCatalog((previous) =>
      previous.group === draft.group ? previous : { group: draft.group, items: [] },
    );
    void reload().catch(props.fail);
  }, [draft.group]);
  // Reading and rendering only ever see the catalog of the group being edited.
  const items = catalog.group === draft.group ? catalog.items : [];
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
      <section className="panel">
        <div className="panel-content">
          <label>
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
          <div className="toolbar">
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
            <button
              className="button secondary"
              onClick={() => void reload().catch(props.fail)}
            >
              刷新目录
            </button>
          </div>
          <label>
            读取资产
            <select
              aria-label="读取资产"
              value=""
              onChange={(e) => {
                // Only the catalog loaded for the current group can be read back,
                // so a stale entry from another type can never be loaded as draft.
                const item = items.find((i) => i.id === e.target.value);
                if (item)
                  patch(
                    {
                      group: draft.group,
                      asset: clone(item.asset),
                      expected: item.fingerprint,
                    },
                    true,
                  );
              }}
            >
              <option value="">选择资产；读取会替换当前编辑草稿</option>
              {items.map((i) => (
                <option key={i.id} value={i.id}>
                  {i.title} ({i.id})
                </option>
              ))}
            </select>
          </label>
          <JsonEditor
            editorKey={`${key}:${draft.group}:${identity.current}`}
            label="资产内容"
            value={draft.asset}
            draft={draft.raw}
            onRawChange={(raw) => patch({ ...draft, raw })}
            onChange={(asset) => patch({ ...draft, asset, raw: undefined })}
            onError={setMessage}
          />
          <div className="toolbar">
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
                } catch (e) {
                  props.fail(e);
                }
              }}
            >
              删除当前资产
            </button>
          </div>
          {message && <p role="status">{message}</p>}
        </div>
      </section>
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
      .then((result) => {
        if (alive) setPresets(result.data.items);
      })
      .catch((e) => {
        if (alive) setError((e as Error).message);
      });
    return () => {
      alive = false;
    };
  }, []);
  return (
    <div className="parameter-selection">
      <label>
        共享预置
        <select
          aria-label="共享预置"
          value={value.preset_id || ""}
          onChange={(e) => {
            const next = { ...value };
            // Decline explicitly: an absent preset_id would fall back to the
            // preset saved on the template, which is not what "don't use one" means.
            if (e.target.value) next.preset_id = e.target.value;
            else next.preset_id = null;
            onChange(next);
            setError("");
          }}
        >
          <option value="">不选择共享预置</option>
          {presets.map((p) => (
            <option value={p.id} key={p.id}>
              {p.title}
            </option>
          ))}
        </select>
      </label>
      <p className="parameter-hint">
        共享预置按参数定义提供取值；本次填写仍优先于预置。
      </p>
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
