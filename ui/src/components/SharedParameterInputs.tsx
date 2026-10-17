import { useEffect, useState } from "react";
import { action } from "../lib/api";
import { encode, numberText, parseData, type Data, type Document } from "../lib/data";
import { encodeDraft } from "../lib/drafts";
import {
  catalogLabel, isStructured, managedRequest, mergeParameterLayers, originLabel, overridePointer,
  pointerValue, previewParameters, removePointer, setParameterOverride,
  type ParameterPreview,
} from "../lib/managedParameters";
import { Form } from "./Form";
import { JsonEditor } from "./WorkbenchUi";
import "./SharedParameterInputs.css";

export interface SharedParameterInputsProps {
  doc: Document;
  onChange: (patch: Partial<Document>) => void;
  mode: "form" | "json";
  errors?: Record<string, string>;
  disabled?: boolean;
  jsonError?: string;
  onJsonError?: (error: string) => void;
}

/** Only this run's explicit overrides are written to context. */
export function SharedParameterInputs(props: SharedParameterInputsProps) {
  return <SharedParameterDocument key={props.doc.key} {...props} />;
}

function SharedParameterDocument({ doc, onChange, mode, errors = {}, disabled = false, jsonError = "", onJsonError }: SharedParameterInputsProps) {
  const [refresh, setRefresh] = useState(0);
  const request = managedRequest(doc);
  let inputError = "";
  try {
    // Raw drafts are authoritative even before the parent's context patch lands.
    if (doc.jsonInput !== undefined) request.values = parseData(doc.jsonInput);
    encode(request);
  } catch (error) { inputError = (error as Error).message; }
  const key = encodeDraft([doc.key, request, doc.jsonInput, refresh]);
  // An inheritance baseline is independent of the current edit. A response to
  // an earlier edit must never replace the user's later keystrokes.
  const { values: _values, schema: _schema, ...inheritance } = request;
  const baselineKey = encodeDraft([doc.key, inheritance, refresh]);
  const [baseline, setBaseline] = useState<{ key: string; value: ParameterPreview }>();
  const [baselineError, setBaselineError] = useState<{ key: string; message: string }>();
  const [resolved, setResolved] = useState<{ key: string; value?: ParameterPreview; error?: string }>();
  const [presets, setPresets] = useState<Data[]>([]);
  const [catalogError, setCatalogError] = useState("");

  useEffect(() => {
    let alive = true;
    void action("parameter.catalog", { group: "presets" })
      .then(result => { if (alive) { setPresets(result.data.items); setCatalogError(""); } })
      .catch(error => { if (alive) setCatalogError((error as Error).message); });
    return () => { alive = false; };
  }, []);

  useEffect(() => {
    let alive = true;
    // This response is for editing only, and never establishes schema validity.
    void previewParameters({ ...inheritance, values: {} })
      .then(value => { if (alive) { setBaseline({ key: baselineKey, value }); setBaselineError(undefined); } })
      .catch(error => { if (alive) setBaselineError({ key: baselineKey, message: (error as Error).message }); });
    return () => { alive = false; };
  }, [baselineKey]);

  useEffect(() => {
    if (inputError) return;
    let alive = true;
    const timer = setTimeout(() => {
      void previewParameters(request)
        .then(value => { if (alive) setResolved({ key, value }); })
        .catch(error => { if (alive) setResolved({ key, error: (error as Error).message }); });
    }, 200);
    return () => { alive = false; clearTimeout(timer); };
  }, [key, inputError]);

  const inherited = baseline?.key === baselineKey ? baseline.value : undefined;
  const current = resolved?.key === key ? resolved : undefined;
  const error = inputError || current?.error || "";
  const valid = !inputError && !!current?.value;
  const effective = mergeParameterLayers(inherited?.values ?? doc.asset.defaults ?? {}, doc.context ?? {});
  const selection = doc.parameterSelection ?? doc.asset.metadata?.parameters?.selection ?? {};
  const presetId = request.preset_id;
  const bindings = request.bindings || {};
  const schema = doc.asset.schema && typeof doc.asset.schema === "object" ? doc.asset.schema : { type: "object", properties: {} };

  function edit(context: Data) { onChange({ context, jsonInput: undefined }); onJsonError?.(""); }
  return <div className="shared-parameter-inputs">
    <div className="binding-section">
      <h3 className="binding-heading">共享预置</h3>
      <fieldset disabled={disabled} className="shared-parameter-controls">
        <label>共享预置
          <select aria-label="共享预置" value={presetId ?? ""} onChange={event => onChange({ parameterSelection: { ...selection, preset_id: event.target.value || null } })}>
            <option value="">不使用共享预置</option>
            {presetId && !presets.some(item => item.id === presetId) && <option value={presetId}>{presetId}（当前选择）</option>}
            {presets.map(item => <option key={item.id} value={item.id}>{catalogLabel(item)}</option>)}
          </select>
        </label>
        <div className="binding-actions">
          <button type="button" className="text-button" onClick={() => edit({})}>清除全部本次覆盖</button>
          <button type="button" className="text-button" onClick={() => setRefresh(value => value + 1)}>刷新共享参数</button>
        </div>
      </fieldset>
      {catalogError && <p role="alert" className="inline-error">共享预置目录读取失败：{catalogError}</p>}
    </div>
    <div className="binding-section">
      <h3 className="binding-heading">本次参数</h3>
      <p className="parameter-hint">表单展示继承值，只保存本次手动覆盖；JSON 只编辑本次覆盖层。</p>
      {mode === "json" ? <fieldset disabled={disabled} className="shared-parameter-controls">
        <JsonEditor editorKey={doc.key} label="本次参数 JSON" value={doc.context} draft={doc.jsonInput}
          onRawChange={jsonInput => { if (!disabled) onChange({ jsonInput }); }}
          onChange={context => { if (!disabled) { onChange({ context }); onJsonError?.(""); } }} onError={onJsonError} />
      </fieldset> : <Form schema={schema} value={effective} errors={errors} disabled={disabled}
        onChange={() => { /* Form's path event is authoritative, including equal values. */ }}
        onFieldChange={(pointer, value) => { if (!disabled) edit(setParameterOverride(doc.context, effective, pointer, value)); }} />}
      {jsonError && <p role="alert" className="inline-error">{jsonError}</p>}
      {error ? <p role="alert" className="inline-error">当前参数校验失败：{error}</p>
        : <p role="status" className="parameter-hint">{valid ? "当前参数已通过解析校验" : "正在解析当前参数…"}</p>}
      {baselineError?.key === baselineKey && <p role="alert" className="inline-error">继承值读取失败：{baselineError.message}。可继续填写本次覆盖。</p>}
      {errors[""] && <p role="alert" className="inline-error">{errors[""]}</p>}
    </div>
    {Object.keys(bindings).length > 0 && <div className="binding-section">
      <h3 className="binding-heading">取值与来源{!valid && "（待校验）"}</h3>
      <ul className="binding-list shared-parameter-origins">{Object.entries(bindings).map(([pointer, binding]: [string, Data]) => {
        const definition = current?.value?.definitions[pointer] ?? inherited?.definitions[pointer];
        const restore = overridePointer(doc.context, pointer);
        const manual = pointerValue(doc.context, restore) !== undefined;
        const value = valid ? pointerValue(current!.value!.values, pointer) : pointerValue(effective, pointer);
        const origin = manual ? "manual" : inherited?.origins[pointer];
        const id = typeof binding === "string" ? binding : binding?.id;
        const name = definition?.title || definition?.id || id || pointer;
        const unit = definition?.unit || (typeof binding === "object" ? binding?.unit : "");
        let text: string;
        try { text = value === undefined ? "未填写" : value === null ? "空值" : isStructured(value) || Array.isArray(value) ? encode(value) : numberText(value); }
        catch { text = "含未完成的输入"; }
        return <li key={pointer} className="binding-card">
          <div className="binding-card-head">
            <strong>{name}</strong>
            <code>{pointer}</code>
            <span className="parameter-hint">{origin ? originLabel(origin) : "来源待解析"}{!valid ? " · 待校验" : ""}</span>
          </div>
          <div className="binding-value">{text}{unit ? " " + unit : ""}</div>
          {manual && <button type="button" className="text-button" disabled={disabled} onClick={() => edit(removePointer(doc.context, restore))}>
            {restore !== pointer ? `恢复整个数组跟随（${restore}）` : "恢复跟随"}
          </button>}
        </li>;
      })}</ul>
    </div>}
  </div>;
}
