import { useEffect, useState } from "react";
import { Play } from "lucide-react";
import { clone, encode, type Data, type Run, fieldErrors } from "../lib/data";
import { encodeDraft, decodeDraft } from "../lib/drafts";
import { signature, validPluginInput } from "../lib/execution";
import { Form } from "./Form";
import { Empty, PageTitle, JsonEditor, RunDetail } from "./WorkbenchUi";
export function PluginPage({
  panel,
  workspaceId,
  capabilities,
  start,
  runs,
  fail,
}: {
  panel: Data;
  workspaceId: string;
  capabilities: Data;
  start: (id: string, input: Data, title: string) => Promise<string | null>;
  runs: Record<string, Run>;
  fail: (error: Data) => void;
}) {
  const entries = capabilities.actions.filter((a: Data) =>
    panel?.actions.includes(a.id),
  );
  const storageKey = "nctool:plugin:" + workspaceId + ":" + panel?.id;
  const [draft, setDraft] = useState<Data>(() => {
    try {
      const stored = localStorage.getItem(storageKey);
      if (stored) return decodeDraft(stored);
    } catch {}
    return {
      selected: entries[0]?.id || "",
      value: clone(entries[0]?.example || {}),
    };
  });
  const entry = entries.find((a: Data) => a.id === draft.selected);
  const { value, runId, raw } = draft;
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [jsonError, setJsonError] = useState("");
  useEffect(() => {
    try {
      localStorage.setItem(storageKey, encodeDraft(draft));
    } catch {}
  }, [storageKey, draft]);
  function patch(values: Data) {
    setDraft((current: Data) => ({ ...current, ...values }));
  }
  const stale =
    !!runId && draft.lastSignature !== signature(entry?.id || "", value, raw);
  return (
    <div className="page-content plugin-page">
      <PageTitle
        title={panel?.title || "插件操作"}
        description="这个页面由已启用的插件提供，使用相同的任务和结果记录。"
      />
      <div className="plugin-columns">
        <section className="panel">
          <div className="panel-header">
            <strong>选择操作</strong>
            <select
              value={entry?.id || ""}
              onChange={(e) => {
                patch({
                  selected: e.target.value,
                  value: clone(
                    entries.find((a: Data) => a.id === e.target.value)
                      ?.example || {},
                  ),
                  raw: undefined,
                  runId: null,
                  lastSignature: undefined,
                });
                setJsonError("");
                setErrors({});
              }}
            >
              {entries.map((a: Data) => (
                <option key={a.id} value={a.id}>
                  {a.title}
                </option>
              ))}
            </select>
          </div>
          <div className="panel-content">
            <Form
              schema={{
                ...entry?.input_schema,
                properties: Object.fromEntries(
                  Object.entries(entry?.input_schema?.properties || {}).map(
                    ([name, field]) => [
                      name,
                      {
                        ...(field as Data),
                        ...Object.fromEntries(
                          Object.entries(
                            entry?.ui_schema?.properties?.[name] || {},
                          ).filter(([key]) =>
                            [
                              "title",
                              "description",
                              "x-unit",
                              "x-placeholder",
                            ].includes(key),
                          ),
                        ),
                      },
                    ],
                  ),
                ),
              }}
              value={value}
              onChange={(value) => {
                patch({ value, raw: undefined });
                setJsonError("");
              }}
              errors={errors}
            />
            <details className="advanced">
              <summary>高级 JSON 输入</summary>
              <div className="plugin-json">
                <JsonEditor
                  value={value}
                  onChange={(value) => patch({ value })}
                  draft={raw}
                  onRawChange={(raw) => patch({ raw })}
                  onError={setJsonError}
                  label="插件输入 JSON"
                />
              </div>
            </details>
          </div>
          <div className="panel-footer">
            <button
              className="button primary"
              onClick={async () => {
                try {
                  if (!entry) throw new Error("当前动作不可用，请选择可用动作");
                  if (jsonError)
                    throw new Error("请先修复插件输入 JSON：" + jsonError);
                  const input = validPluginInput(
                    entry.input_schema,
                    value,
                    raw,
                  );
                  const problems = fieldErrors(entry.input_schema, input);
                  setErrors(problems);
                  if (Object.keys(problems).length) return;
                  const submitted = signature(entry.id, value, raw);
                  const id = await start(entry.id, input, entry.title);
                  if (id) patch({ runId: id, lastSignature: submitted });
                } catch (error) {
                  fail(error);
                }
              }}
            >
              <Play size={15} />
              执行任务
            </button>
          </div>
        </section>
        <section className="panel">
          {runId && runs[runId] ? (
            <RunDetail run={runs[runId]} stale={stale} />
          ) : (
            <Empty
              title="结果与诊断"
              description="完成操作后，可以在这里查看结果并导出文件。"
            />
          )}
        </section>
      </div>
    </div>
  );
}
