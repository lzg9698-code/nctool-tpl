import { FolderOpen, Play, RotateCcw, Save } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Form } from "../../components/Form";
import { Empty, JsonEditor } from "../../components/WorkbenchUi";
import { clone, defaultValue, mergeDefaults, type Data } from "../../lib/data";
import {
  applyOverrides,
  isManual,
  managedFields,
  managedRequest,
  originLabel,
  pointerTokens,
  pointerValue,
  previewParameters,
  removePointer,
  type ParameterPreview,
} from "../../lib/managedParameters";
import { ParameterSelection } from "../../plugins/ParameterViews";

import {
  normalizeSchema,
  type WorkbenchController,
} from "./useWorkbenchController";
export function ParametersPane({
  controller,
}: {
  controller: WorkbenchController;
}) {
  const {
    errors,
    paramMode,
    setParamMode,
    jsonError,
    setJsonError,
    doc,
    generation,
    editDoc,
    generate,
    savePreset,
    loadPreset,
    busy,
    generateHint,
  } = controller;
  const [preview, setPreview] = useState<ParameterPreview | null>(null);
  const [previewError, setPreviewError] = useState("");
  const managed = generation?.id === "template.render.managed";
  // The same request generation will send, so the preview matches the output.
  const request = useMemo(
    () =>
      doc
        ? managedRequest({
            asset: doc.asset,
            context: doc.context,
            parameterSelection: doc.parameterSelection,
          })
        : null,
    [doc?.asset, doc?.context, doc?.parameterSelection],
  );
  useEffect(() => {
    if (!doc || !managed || paramMode !== "form") {
      setPreview(null);
      setPreviewError("");
      return;
    }
    let alive = true;
    const timer = setTimeout(async () => {
      try {
        const next = await previewParameters(request!);
        if (!alive) return;
        setPreview(next);
        setPreviewError("");
      } catch (error) {
        if (!alive) return;
        setPreview(null);
        setPreviewError((error as Error).message || "参数预览失败");
      }
    }, 250);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [doc?.key, managed, paramMode, request]);
  if (!doc) return null;
  const current = doc;
  // Show the effective values, not the sparse override layer, so an inherited
  // value never looks like an empty field the user has to fill in.
  const effective = preview
    ? preview.values
    : mergeDefaults(doc.asset.defaults, doc.context);
  // Remember what the form showed, so an edit can be reduced to its real delta
  // instead of freezing every inherited value into a manual override.
  const shown = useRef<{ key: string; value: Data } | null>(null);
  const baseline =
    shown.current?.key === doc.key && shown.current.value !== undefined
      ? shown.current.value
      : effective;
  if (shown.current?.key !== doc.key || shown.current.value === undefined)
    shown.current = { key: doc.key, value: clone(effective) };
  // Refresh the baseline whenever the effective values change from elsewhere
  // (a new preset, a cleared override), so the next edit diffs against them.
  useEffect(() => {
    if (shown.current?.key === doc.key) shown.current.value = clone(effective);
  }, [effective]);
  function clearOverride(pointer: string) {
    editDoc({
      context: removePointer(current.context, pointer),
      jsonInput: undefined,
    });
  }
  return (
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
        <button className="text-button" onClick={() => void loadPreset()}>
          <FolderOpen size={13} />
          加载预设
        </button>
        <button className="text-button" onClick={savePreset}>
          <Save size={13} />
          保存预设
        </button>
        <button
          className="text-button"
          onClick={() =>
            editDoc({
              // A shared template's context holds only this run's overrides, so
              // clearing it restores every inherited value; an ordinary template
              // keeps its default-filling behaviour.
              context: managed
                ? {}
                : mergeDefaults(
                    defaultValue(normalizeSchema(doc.asset)),
                    doc.asset.defaults,
                  ),
              jsonInput: undefined,
            })
          }
        >
          <RotateCcw size={13} />
          {managed ? "清除本次覆盖" : "恢复默认值"}
        </button>
      </div>
      {managed && paramMode === "json" && (
        <p className="parameter-hint">
          这里的 JSON 只写本次要覆盖的值；共享定义、模板默认值和共享预置会补齐其余字段。
        </p>
      )}
      {paramMode === "json" ? (
        <JsonEditor
          editorKey={doc.key}
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
                doc.asset.metadata.parameters.selection || {
                  preset_id: doc.asset.metadata.parameters.preset_id,
                }
              }
              onChange={(parameterSelection) => editDoc({ parameterSelection })}
            />
          )}
          {managed && previewError && (
            <p className="inline-error" role="alert">
              {previewError}
            </p>
          )}
          <Form
            schema={normalizeSchema(doc.asset)}
            value={effective}
            onChange={(context) =>
              editDoc({
                context: managed
                  ? applyOverrides(baseline, context, doc.context)
                  : context,
                jsonInput: undefined,
              })
            }
            errors={errors}
          />
          {!Object.keys(normalizeSchema(doc.asset).properties || {}).length && (
            <Empty
              title="这个模板没有定义参数"
              description="如果源码使用了变量，请在编辑模式识别变量并定义类型。"
            />
          )}
          {managed && preview && (
            <OriginSummary
              preview={preview}
              fields={managedFields(doc.asset)}
              onRestore={clearOverride}
            />
          )}
          {errors[""] && <div className="field-message">{errors[""]}</div>}
        </div>
      )}
      {jsonError && <div className="inline-error">{jsonError}</div>}
      <div className="parameter-footer">
        <span className="action-hint" role="status">
          {generateHint}
        </span>
        <button
          className="button primary"
          disabled={busy || !generation}
          title={generateHint}
          onClick={() => void generate()}
        >
          <Play size={14} />
          生成文件
        </button>
      </div>
    </section>
  );
}
function OriginSummary({
  preview,
  fields,
  onRestore,
}: {
  preview: ParameterPreview;
  fields: string[];
  onRestore: (pointer: string) => void;
}) {
  const rows = Object.entries(preview.origins).filter(([pointer]) =>
    fields.length ? fields.includes(pointerTokens(pointer)[0] ?? "") : true,
  );
  if (!rows.length) return null;
  const overridden = rows.filter(([, origin]) => isManual(origin));
  return (
    <div className="origin-summary">
      <strong>参数来源</strong>
      <ul>
        {rows.map(([pointer, origin]) => {
          const value = pointerValue(preview.values, pointer);
          const definition = preview.definitions[pointer];
          return (
            <li key={pointer}>
              <span className="origin-label">{originLabel(origin)}</span>
              <span className="origin-value">
                {value === undefined
                  ? "未填写"
                  : typeof value === "object" && value !== null
                    ? "（结构化值）"
                    : String(value)}
                {definition?.unit ? " " + definition.unit : ""}
              </span>
              {isManual(origin) && (
                <button
                  className="text-button"
                  onClick={() => onRestore(pointer)}
                >
                  恢复跟随
                </button>
              )}
            </li>
          );
        })}
      </ul>
      {overridden.length > 0 && (
        <p className="parameter-hint">
          有 {overridden.length} 项为本次填写，会覆盖对应的共享取值。
        </p>
      )}
    </div>
  );
}
