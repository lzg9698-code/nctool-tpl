import { useState } from "react";
import { Editor } from "./Editor";
import { action } from "../lib/api";
import { cloneDraft, decodeDraft, encodeDraft } from "../lib/drafts";
import { safeNumber, type Data } from "../lib/data";

export function SpecializedTemplateEditor({
  storageKey,
  sideTitle,
  templateId,
  operation,
  onSaved,
  fail,
}: {
  storageKey: string;
  sideTitle: string;
  templateId: string;
  operation: string;
  onSaved: (id: string, asset: Data, fingerprint: string) => Promise<void>;
  fail: (error: unknown) => void;
}) {
  const [draft, setDraft] = useState<Data>(() => {
    try {
      return decodeDraft(localStorage.getItem(storageKey) || "");
    } catch {
      return undefined;
    }
  });
  const [busy, setBusy] = useState(false),
    [message, setMessage] = useState(""),
    [error, setError] = useState("");
  function update(value: Data) {
    setDraft(value);
    localStorage.setItem(storageKey, encodeDraft(value));
    setMessage("");
    setError("");
  }
  async function load() {
    setBusy(true);
    try {
      if (!templateId) {
        update({
          id: "",
          sourceId: "",
          expected: null,
          asset: {
            source: "{{ trajectory_program }}",
            metadata: {
              title: sideTitle + "自定义模板",
              nc: {
                trajectory_plan_version: 1,
                operation: { id: operation, version: 1 },
              },
            },
          },
        });
      } else {
        const response = await action("template.read", { name: templateId });
        if (response.data.asset.metadata?.nc?.operation?.id !== operation)
          throw new Error("模板工艺或侧别与当前插件不匹配。");
        update({
          id: templateId,
          sourceId: templateId,
          expected: response.data.fingerprint,
          asset: cloneDraft(response.data.asset),
        });
      }
    } catch (e) {
      setError((e as Error).message);
      fail(e);
    } finally {
      setBusy(false);
    }
  }
  async function inspect() {
    const response = await action("template.inspect", {
      source: draft.asset.source,
      ...(draft.id ? { template: draft.id } : {}),
    });
    return (
      response.data.variables.map((v: Data) => v.name).join("、") || "无变量"
    );
  }
  async function check() {
    setBusy(true);
    setError("");
    setMessage("");
    try {
      setMessage("语法校验通过；引用变量：" + (await inspect()));
    } catch (e) {
      setError("校验失败：" + (e as Error).message);
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
      if (!draft.id?.trim()) throw new Error("请填写模板 ID。");
      if (!draft.asset.metadata?.title?.trim())
        throw new Error("请填写模板名称。");
      await inspect();
      const asset = cloneDraft(draft.asset);
      const response = await action("template.save", {
        name: draft.id,
        asset,
        expected: draft.id === draft.sourceId ? draft.expected : null,
      });
      written = true;
      update({
        ...draft,
        asset,
        sourceId: draft.id,
        expected: response.data.fingerprint,
      });
      await onSaved(draft.id, asset, response.data.fingerprint);
      setMessage("模板已保存。选择此模板格式并重新计算或生成即可应用。");
    } catch (e) {
      setError(
        (written
          ? "模板已保存，但列表刷新失败："
          : "保存失败，源码草稿已保留：") + (e as Error).message,
      );
      fail(e);
    } finally {
      setBusy(false);
    }
  }
  return (
    <section
      className="turning-template-editor"
      aria-label={sideTitle + "专用模板编辑器"}
    >
      <div className="toolbar">
        <button disabled={busy} onClick={() => void load()}>
          {draft
            ? "重新读取所选模板"
            : templateId
              ? "编辑所选模板"
              : "新建自定义模板"}
        </button>
        {draft && (
          <button
            disabled={busy}
            onClick={() => {
              setDraft(undefined);
              localStorage.removeItem(storageKey);
              setError("");
              setMessage("");
            }}
          >
            放弃模板编辑
          </button>
        )}
      </div>
      {draft && (
        <>
          <p className="hint">
            完整 Jinja/NC 源码参与生成。修改 ID 可另存；重新读取会替换源码草稿。
          </p>
          <fieldset disabled={busy}>
            <div className="turning-globals">
              <label>
                模板 ID
                <input
                  aria-label={sideTitle + "模板 ID"}
                  value={draft.id}
                  onChange={(e) => update({ ...draft, id: e.target.value })}
                />
              </label>
              <label>
                模板名称
                <input
                  aria-label={sideTitle + "模板名称"}
                  value={draft.asset.metadata?.title || ""}
                  onChange={(e) =>
                    update({
                      ...draft,
                      asset: {
                        ...draft.asset,
                        metadata: {
                          ...draft.asset.metadata,
                          title: e.target.value,
                        },
                      },
                    })
                  }
                />
              </label>
            </div>
            <label className="switch-label">
              <input
                type="checkbox"
                aria-label={sideTitle + "模板支持专用工作台"}
                checked={
                  safeNumber(
                    draft.asset.metadata?.nc?.trajectory_plan_version,
                  ) === 1
                }
                onChange={(e) =>
                  update({
                    ...draft,
                    asset: {
                      ...draft.asset,
                      metadata: {
                        ...draft.asset.metadata,
                        nc: {
                          ...draft.asset.metadata.nc,
                          trajectory_plan_version: e.target.checked
                            ? 1
                            : undefined,
                        },
                      },
                    },
                  })
                }
              />
              用于专用工作台（轨迹计划 v1）
            </label>
          </fieldset>
          <div className="turning-template-source">
            <Editor
              value={draft.asset.source}
              readonly={busy}
              label={sideTitle + "专用模板源码"}
              editorKey={storageKey}
              onChange={(source) =>
                update({ ...draft, asset: { ...draft.asset, source } })
              }
            />
          </div>
          <div className="toolbar">
            <button disabled={busy} onClick={() => void check()}>
              校验模板
            </button>
            <button
              className="button primary"
              disabled={busy}
              onClick={() => void save()}
            >
              保存模板
            </button>
          </div>
          <p className="hint">
            可引用 trajectory_program
            和派生工艺变量。语法校验不替代实际生成；轨迹图只显示预设轨迹，不包含模板中额外编写的运动代码。
          </p>
        </>
      )}
      {message && (
        <p className="hint" role="status">
          {message}
        </p>
      )}
      {error && (
        <p className="error-box" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
