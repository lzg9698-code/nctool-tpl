import { useState } from "react";
import { action } from "../lib/api";
import { DraftNumber, encode, numberText, type Data } from "../lib/data";
import { cloneDraft, decodeDraft, encodeDraft } from "../lib/drafts";
import type { TurningKind } from "../lib/turningModel";

export function TrajectoryPresetEditor({
  kind,
  storageKey,
  sideTitle,
  presetId,
  builtin,
  schema,
  labels,
  onSaved,
  fail,
}: {
  kind: TurningKind;
  storageKey: string;
  sideTitle: string;
  presetId: string;
  builtin: Data;
  schema: Data;
  labels: Record<string, string>;
  onSaved: (asset: Data, fingerprint: string) => Promise<void>;
  fail: (error: unknown) => void;
}) {
  const [editor, setEditor] = useState<Data>(() => {
    try {
      return decodeDraft(localStorage.getItem(storageKey) || "");
    } catch {
      return undefined;
    }
  });
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  function update(next: Data) {
    setEditor(next);
    localStorage.setItem(storageKey, encodeDraft(next));
    setMessage("");
    setError("");
  }
  async function load() {
    setBusy(true);
    try {
      let asset = builtin,
        expected = null;
      if (presetId !== "v3-compatible") {
        const response = await action(kind + ".asset.catalog", {
          group: "trajectories",
        });
        const selected = response.data.items.find(
          (p: Data) => p.id === presetId,
        );
        if (!selected) throw new Error("所选轨迹预设不存在，请重新选择。");
        asset = selected.asset;
        expected = selected.fingerprint;
      }
      if (!asset) throw new Error("轨迹定义尚未加载。");
      const copy = cloneDraft(asset);
      if (presetId === "v3-compatible") {
        copy.id = "";
        copy.title += "（自定义）";
      }
      update({ asset: copy, sourceId: presetId, expected });
    } catch (e) {
      setError((e as Error).message);
      fail(e);
    } finally {
      setBusy(false);
    }
  }
  async function save() {
    setBusy(true);
    setError("");
    setMessage("");
    let written = false;
    try {
      const asset = cloneDraft(editor.asset);
      if (!asset.id?.trim() || asset.id === "v3-compatible")
        throw new Error("请填写新的预设 ID，内置 v3-compatible 不可覆盖。");
      if (!asset.title?.trim()) throw new Error("请填写预设名称。");
      encode(asset);
      const result = await action(kind + ".asset.save", {
        group: "trajectories",
        name: asset.id,
        asset,
        expected: asset.id === editor.sourceId ? editor.expected : null,
      });
      written = true;
      update({ asset, sourceId: asset.id, expected: result.data.fingerprint });
      await onSaved(asset, result.data.fingerprint);
      setMessage("预设已保存。可在轨迹预设列表中选择；本次人工调整保持原值。");
    } catch (e) {
      setError(
        (written
          ? "预设已保存，但列表刷新失败："
          : "保存失败，编辑草稿已保留：") + (e as Error).message,
      );
      fail(e);
    } finally {
      setBusy(false);
    }
  }
  return (
    <section
      className="turning-preset-editor"
      aria-label={sideTitle + "轨迹预设编辑器"}
    >
      <div className="toolbar">
        <button disabled={busy || !builtin} onClick={() => void load()}>
          {editor ? "重新读取所选预设" : "编辑所选预设"}
        </button>
        {editor && (
          <button
            disabled={busy}
            onClick={() => {
              setEditor(undefined);
              localStorage.removeItem(storageKey);
              setError("");
              setMessage("");
            }}
          >
            放弃预设编辑
          </button>
        )}
      </div>
      {editor && (
        <>
          <p className="hint">
            直接编辑预设，无需填写加工行或先计算。修改 ID
            可另存一份；重新读取会替换此处编辑草稿。
          </p>
          <fieldset disabled={busy}>
            <div className="turning-globals">
              <label>
                预设 ID
                <input
                  aria-label={sideTitle + "预设 ID"}
                  value={editor.asset.id}
                  onChange={(e) =>
                    update({
                      ...editor,
                      asset: { ...editor.asset, id: e.target.value },
                    })
                  }
                />
              </label>
              <label>
                预设名称
                <input
                  aria-label={sideTitle + "预设名称"}
                  value={editor.asset.title}
                  onChange={(e) =>
                    update({
                      ...editor,
                      asset: { ...editor.asset, title: e.target.value },
                    })
                  }
                />
              </label>
            </div>
            <div className="turning-globals">
              {Object.keys(schema?.properties || {}).map((k) => (
                <label key={k}>
                  {labels[k] || k}
                  <input
                    inputMode="decimal"
                    aria-label={sideTitle + "预设 " + (labels[k] || k)}
                    value={numberText(editor.asset.parameters?.[k])}
                    onChange={(e) =>
                      update({
                        ...editor,
                        asset: {
                          ...editor.asset,
                          parameters: {
                            ...editor.asset.parameters,
                            [k]: new DraftNumber(e.target.value),
                          },
                        },
                      })
                    }
                  />
                </label>
              ))}
            </div>
            <button className="button primary" onClick={() => void save()}>
              {busy ? "保存中…" : "保存预设"}
            </button>
          </fieldset>
        </>
      )}
      {message && (
        <p role="status" className="hint">
          {message}
        </p>
      )}
      {error && (
        <p role="alert" className="error-box">
          {error}
        </p>
      )}
    </section>
  );
}
