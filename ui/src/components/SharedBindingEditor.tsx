import { useEffect, useMemo, useRef, useState } from "react";
import { action } from "../lib/api";
import { encode, type Asset, type Data } from "../lib/data";
import { catalogLabel, managedRequest, originLabel, pointerValue, previewParameters, type ParameterPreview } from "../lib/managedParameters";

interface CatalogItem {
  id: string;
  title?: string;
  asset?: Data;
  schema?: Data;
  unit?: Data;
  description?: string;
  default?: Data;
}

export interface SharedBindingEditorProps {
  asset: Asset;
  onChange: (asset: Asset) => void;
}

const RETIRED_ACTION = /^(nc|machine|process|sync-turning|undercut)\./;
const FORBIDDEN_TOKEN = /(^|\/)(?:__proto__|constructor|prototype)(\/|$)/;

function hasOwn(value: Data, key: string): boolean {
  return value !== null && typeof value === "object" && Object.hasOwn(value, key);
}
function bindingShapeError(parameters: Data): string {
  if (parameters === undefined) return "";
  if (!parameters || typeof parameters !== "object" || Array.isArray(parameters))
    return "metadata.parameters 必须是对象；为避免丢失数据，已禁用编辑。";
  if (hasOwn(parameters, "bindings") && (!parameters.bindings || typeof parameters.bindings !== "object" || Array.isArray(parameters.bindings)))
    return "metadata.parameters.bindings 必须是对象；为避免丢失数据，已禁用编辑。";
  if (hasOwn(parameters, "selection") && (!parameters.selection || typeof parameters.selection !== "object" || Array.isArray(parameters.selection)))
    return "metadata.parameters.selection 必须是对象；为避免丢失数据，已禁用编辑。";
  return "";
}
function selectionPreset(parameters: Data): string | null | undefined {
  const selection = parameters?.selection;
  if (hasOwn(selection, "preset_id")) return selection.preset_id;
  if (hasOwn(parameters, "preset_id")) return parameters.preset_id;
  return undefined;
}
function updateParameters(asset: Asset, parameters: Data): Asset {
  return {
    ...asset,
    metadata: { ...(asset.metadata || {}), parameters },
  };
}
function pointerError(pointer: string, bindings: Record<string, Data>): string {
  if (!pointer.startsWith("/") || pointer.length > 1024 || pointer === "/")
    return "请输入非空 JSON Pointer（以 / 开头）。";
  const tokens = pointer.slice(1).split("/");
  if (tokens.some((token) => !token || /~(?![01])/.test(token)))
    return "JSON Pointer token 不能为空，且 ~ 只能写作 ~0 或 ~1。";
  if (FORBIDDEN_TOKEN.test(pointer.replace(/~1/g, "/").replace(/~0/g, "~")))
    return "此 JSON Pointer 包含禁止的路径 token。";
  if (Object.hasOwn(bindings, pointer)) return "这个 JSON Pointer 已经绑定。";
  if (Object.keys(bindings).some((other) => other.startsWith(pointer + "/") || pointer.startsWith(other + "/")))
    return "这个 JSON Pointer 与现有绑定路径重叠。";
  return "";
}
function isManagedLegacySafe(asset: Asset): boolean {
  const metadata = asset.metadata || {};
  return !Object.hasOwn(metadata, "nc") && !RETIRED_ACTION.test(metadata.execution?.action || "");
}
function isRetiredIntent(asset: Asset): boolean {
  return !isManagedLegacySafe(asset);
}
function shownDefault(item?: CatalogItem): string {
  const target = item?.asset || item;
  if (!target || !Object.hasOwn(target, "default")) return "无";
  try { return encode(target.default); }
  catch { return "（无法显示）"; }
}
function withDefinition(binding: Data, definitionId: string, definition?: CatalogItem): Data {
  if (!binding || typeof binding !== "object" || Array.isArray(binding)) return definitionId;
  const next = { ...binding, id: definitionId };
  const unit = definition?.asset?.unit ?? definition?.unit;
  if (unit === undefined) delete next.unit;
  else next.unit = unit;
  return next;
}

export function SharedBindingEditor({ asset, onChange }: SharedBindingEditorProps) {
  const parameters: Data = asset.metadata?.parameters;
  const malformed = bindingShapeError(parameters);
  const active = parameters !== undefined;
  const bindings: Record<string, Data> = active && !malformed ? parameters.bindings || {} : {};
  const currentPreset = selectionPreset(parameters);
  const [definitions, setDefinitions] = useState<CatalogItem[]>([]);
  const [presets, setPresets] = useState<CatalogItem[]>([]);
  const [catalogError, setCatalogError] = useState("");
  const [newPointer, setNewPointer] = useState("");
  const [newDefinition, setNewDefinition] = useState("");
  const [check, setCheck] = useState<{ key: string; result?: ParameterPreview; error?: string }>();
  const [checking, setChecking] = useState(false);
  const requestKeyRef = useRef("");
  const requestSequence = useRef(0);

  useEffect(() => {
    let alive = true;
    Promise.all([
      action("parameter.catalog", { group: "definitions" }),
      action("parameter.catalog", { group: "presets" }),
    ]).then(([definitionResult, presetResult]) => {
      if (!alive) return;
      setDefinitions(definitionResult.data.items || []);
      setPresets(presetResult.data.items || []);
      setCatalogError("");
    }).catch((error) => {
      if (alive) setCatalogError((error as Error).message || "目录读取失败");
    });
    return () => { alive = false; };
  }, []);

  const request = useMemo(() => {
    if (!active || malformed) return undefined;
    // managedRequest is the canonical frontend builder for saved binding and preset precedence.
    return managedRequest({ asset, context: {}, parameterSelection: undefined });
  }, [asset, active, malformed]);
  let requestError = "";
  let requestKey = "";
  if (request) {
    try { request.values = {}; requestKey = encode(request); }
    catch (error) { requestError = (error as Error).message; }
  }
  const currentCheck = check?.key === requestKey ? check : undefined;
  useEffect(() => {
    requestKeyRef.current = requestKey;
    requestSequence.current += 1;
    setChecking(false);
    return () => { requestSequence.current += 1; };
  }, [requestKey]);

  function emit(nextParameters: Data) {
    setCheck(undefined);
    setChecking(false);
    requestKeyRef.current = "";
    requestSequence.current += 1;
    onChange(updateParameters(asset, nextParameters));
  }
  function saveBinding(pointer: string, definitionId: string) {
    if (!definitionId || pointerError(pointer, bindings)) return;
    emit({ ...parameters, bindings: { ...bindings, [pointer]: definitionId } });
    setNewPointer("");
    setNewDefinition("");
  }
  function choosePreset(value: string) {
    const selection = { ...(parameters.selection || {}), preset_id: value || null };
    emit({ ...parameters, selection });
  }
  async function runCheck() {
    if (!request || malformed || requestError) return;
    const snapshotKey = requestKey;
    const sequence = ++requestSequence.current;
    requestKeyRef.current = snapshotKey;
    setCheck(undefined);
    setChecking(true);
    try {
      const result = await previewParameters(request);
      if (requestKeyRef.current === snapshotKey && requestSequence.current === sequence) setCheck({ key: snapshotKey, result });
    } catch (error) {
      if (requestKeyRef.current === snapshotKey && requestSequence.current === sequence) setCheck({ key: snapshotKey, error: (error as Error).message });
    } finally {
      if (requestKeyRef.current === snapshotKey && requestSequence.current === sequence) setChecking(false);
    }
  }

  if (isRetiredIntent(asset)) return null;
  if (!active) {
    return <section className="shared-parameter-inputs" aria-label="共享参数绑定">
      <button type="button" onClick={() => {
        const metadata = asset.metadata || {};
        const execution = metadata.execution && typeof metadata.execution === "object" ? metadata.execution : {};
        onChange({ ...asset, metadata: { ...metadata, parameters: { bindings: {} }, execution: { ...execution, action: "template.render.managed" } } });
      }}>启用共享参数绑定</button>
    </section>;
  }

  const definitionsById = new Map(definitions.map((item) => [item.id, item]));
  const addError = newPointer ? pointerError(newPointer, bindings) : "";
  const savedPointerErrors = Object.keys(bindings).flatMap((pointer) => {
    const syntaxError = pointerError(pointer, Object.fromEntries(Object.entries(bindings).filter(([key]) => key !== pointer)));
    return syntaxError && !syntaxError.includes("已经绑定") ? [[pointer, syntaxError] as const] : [];
  });
  const presetValue = currentPreset === undefined ? "" : currentPreset === null ? "" : String(currentPreset);
  return <section className="shared-parameter-inputs" aria-label="共享参数绑定">
    {malformed && <p role="alert" className="inline-error">{malformed}</p>}
    {catalogError && <p role="alert" className="inline-error">参数目录读取失败：{catalogError}</p>}
    {active && !malformed && <>
      <div className="binding-section">
        <h3 className="binding-heading">模板默认共享预置</h3>
        <p className="parameter-hint">新建的使用任务默认套用这个预置；单次使用仍可在参数区改选或取消。</p>
        <label>共享预置
          <select aria-label="模板默认共享预置" value={presetValue} onChange={(event) => choosePreset(event.target.value)}>
            <option value="">不使用共享预置</option>
            {currentPreset && !presets.some((item) => item.id === currentPreset) && <option value={currentPreset}>{currentPreset}（当前选择，目录中未知）</option>}
            {presets.map((item) => <option key={item.id} value={item.id}>{catalogLabel(item)}</option>)}
          </select>
        </label>
      </div>
      <div className="binding-section">
        <h3 className="binding-heading">当前绑定</h3>
        <p className="parameter-hint">默认取值顺序：共享定义默认值 → 模板默认值 → 共享预置 → 本次覆盖。</p>
        <ul className="shared-parameter-origins binding-list" aria-label="当前绑定">
          {savedPointerErrors.map(([pointer, message]) => <li key={`error:${pointer}`} className="binding-card"><code>{pointer}</code><p role="alert" className="inline-error">{message}</p></li>)}
          {Object.entries(bindings).map(([pointer, binding]) => {
            const id = typeof binding === "string" ? binding : binding?.id;
            const definition = definitionsById.get(id);
            const schema = definition?.asset?.schema || definition?.schema;
            const displayType = schema?.type === undefined ? "未识别" : Array.isArray(schema.type) ? schema.type.join(" | ") : schema.type;
            return <li key={pointer} className="binding-card">
              <div className="binding-card-head">
                <code>{pointer}</code>
                <button type="button" className="text-button" aria-label={`移除绑定 ${pointer}`} onClick={() => {
                  const next = { ...bindings }; delete next[pointer]; emit({ ...parameters, bindings: next });
                }}>移除此绑定</button>
              </div>
              <label>共享定义
                <select aria-label={`共享定义 ${pointer}`} value={id || ""} onChange={(event) => {
                  if (event.target.value) emit({ ...parameters, bindings: { ...bindings, [pointer]: withDefinition(binding, event.target.value, definitionsById.get(event.target.value)) } });
                }}>
                  {id && !definitionsById.has(id) && <option value={id}>{id}（未知定义，保留当前值）</option>}
                  {definitions.map((item) => <option key={item.id} value={item.id}>{catalogLabel(item)}</option>)}
                </select>
              </label>
              <dl className="binding-summary">
                <div><dt>类型</dt><dd>{displayType}</dd></div>
                <div><dt>单位</dt><dd>{definition?.asset?.unit ?? definition?.unit ?? "无"}</dd></div>
                <div><dt>默认值</dt><dd>{shownDefault(definition)}</dd></div>
                <div><dt>说明</dt><dd>{definition?.asset?.description ?? definition?.description ?? "无"}</dd></div>
              </dl>
            </li>;
          })}
          {!Object.keys(bindings).length && !savedPointerErrors.length && <li className="parameter-hint">还不知道哪些字段使用共享参数。新增绑定后即可检查取值与来源。</li>}
        </ul>
      </div>
      <div className="binding-section">
        <h3 className="binding-heading">新增绑定</h3>
        <p className="parameter-hint">把模板里的某个 JSON 路径指向一个共享定义。</p>
        <div className="binding-add">
          <label>JSON Pointer
            <input aria-label="新增绑定 JSON Pointer" value={newPointer} onChange={(event) => setNewPointer(event.target.value)} placeholder="例如 /report/width" />
          </label>
          <label>共享定义
            <select aria-label="新增绑定共享定义" value={newDefinition} onChange={(event) => setNewDefinition(event.target.value)}>
              <option value="">选择共享定义</option>
              {definitions.map((item) => <option key={item.id} value={item.id}>{catalogLabel(item)}</option>)}
            </select>
          </label>
          <button type="button" className="button secondary" disabled={!newDefinition || !newPointer || !!addError || !!catalogError} onClick={() => saveBinding(newPointer, newDefinition)}>添加绑定</button>
        </div>
        {newPointer && <p className="parameter-hint">尚未添加</p>}
        {addError && <p role="alert" className="inline-error">{addError}</p>}
      </div>
      <div className="binding-section">
        <h3 className="binding-heading">识别与检查</h3>
        <p className="parameter-hint">“检查”只解析继承值与来源，不执行模板，也不保存资产。</p>
        <button type="button" className="button secondary" disabled={!!requestError || !!malformed || checking} onClick={() => void runCheck()}>{checking ? "正在检查共享绑定…" : "检查共享绑定"}</button>
        {requestError && <p role="alert" className="inline-error">检查输入无法序列化：{requestError}</p>}
        {currentCheck?.error && <p role="alert" className="inline-error">共享绑定检查失败：{currentCheck.error}</p>}
        {currentCheck?.result && <section aria-label="共享绑定检查结果" className="shared-parameter-origins binding-list">
          <ul>
            {Object.keys(currentCheck.result.values || {}).length
              ? Object.keys(bindings).map((pointer) => {
                  const value = pointerValue(currentCheck.result!.values, pointer);
                  const origin = currentCheck.result!.origins[pointer];
                  let text: string;
                  try { text = value === undefined ? "未填写" : value === null ? "空值" : encode(value); }
                  catch { text = "（无法显示）"; }
                  return <li key={pointer} className="binding-card">
                    <div className="binding-card-head"><code>{pointer}</code><span className="parameter-hint">{origin ? originLabel(origin) : "来源待解析"}</span></div>
                    <div>{text}</div>
                  </li>;
                })
              : <li className="parameter-hint">没有可展示的解析结果。</li>}
          </ul>
        </section>}
      </div>
      <details className="binding-section binding-danger">
        <summary>停用共享参数绑定</summary>
        <p>这只会移除共享参数设置；若当前使用共享参数渲染，也会切换为普通模板渲染。其他模板设置和扩展内容会保留。</p>
        <button type="button" className="text-button" onClick={() => {
          const metadata = { ...(asset.metadata || {}) };
          delete metadata.parameters;
          const execution = metadata.execution;
          if (execution && typeof execution === "object" && execution.action === "template.render.managed")
            metadata.execution = { ...execution, action: "template.render" };
          onChange({ ...asset, metadata });
          setCheck(undefined);
        }}>确认移除共享参数绑定</button>
      </details>
    </>}
  </section>;
}
