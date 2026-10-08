import { useEffect, useState } from "react";
import { action } from "../lib/api";
import { clone, encode, parseData, type Data } from "../lib/data";
import { encodeDraft, decodeDraft } from "../lib/drafts";
import { validatedSelection } from "../lib/managedParameters";
import { JsonEditor, PageTitle, RunDetail } from "../components/WorkbenchUi";
import type { ViewProps } from "./NcViews";

const titles: Record<string, string> = { definitions: "参数定义", presets: "共享预置", tool_types: "刀具类型", tools: "具体刀具", materials: "材料", profiles: "加工预置", standards: "工艺标准" };
function selectionSignature(value: Data): string {
  const next = clone(value);
  for (const choice of Object.values(next.slots || {}) as Data[]) delete choice.confirmed_for;
  return encode(next);
}
function seed(group: string): Data {
  const base = { format_version: 1, id: "", title: "" };
  const fields: Record<string, Data> = {
    definitions: { schema: { type: "number" }, unit: "mm", group: "", description: "" },
    presets: { values: {} }, tool_types: { operation: "milling" },
    tools: { type_id: "", attributes: {} }, materials: {},
    profiles: { tool_id: "", material_id: "", operation: "milling", mode: "calculate", feed_mode: "per_minute", values: {} },
    standards: { tables: {}, source: "", review_status: "unreviewed" },
  };
  return { ...base, ...fields[group] };
}
export function ParameterLibrary(props: ViewProps) {
  const nc = props.panel.view === "nc.parameters";
  const prefix = nc ? "nc.parameter" : "parameter";
  const groups = nc ? ["tool_types", "tools", "materials", "profiles", "standards"] : ["definitions", "presets"];
  const key = `nctool:parameter-draft:${props.workspaceId}:${prefix}`;
  const [draft, update] = useState<Data>(() => { try { return decodeDraft(localStorage.getItem(key) || ""); } catch { return { group: groups[0], asset: seed(groups[0]), expected: null }; } });
  const [items, setItems] = useState<Data[]>([]), [message, setMessage] = useState("");
  function patch(next: Data) { update(next); localStorage.setItem(key, encodeDraft(next)); setMessage(""); }
  async function reload() { setItems((await action(prefix + ".catalog", { group: draft.group })).data.items); }
  useEffect(() => { void reload().catch(props.fail); }, [draft.group]);
  async function save() {
    try {
      if (draft.raw !== undefined) throw new Error("请先修复资产 JSON 草稿");
      if (!draft.asset.id) throw new Error("请填写稳定资产 ID");
      const result = await action(prefix + ".save", { group: draft.group, name: draft.asset.id, asset: draft.asset, expected: draft.expected });
      patch({ ...draft, expected: result.data.fingerprint }); setMessage("已保存"); await reload();
    } catch (e) { setMessage((e as Error).message || "保存失败；草稿已保留，请重新读取并核对冲突。"); props.fail(e); }
  }
  return <div className="page-content"><PageTitle title={nc ? "刀具与加工参数" : "参数管理"} description="定义与预置独立保存。本次输入和生成结果不会修改共享资产。" />
    <section className="panel"><div className="panel-content">
      <label>资产类型<select aria-label="资产类型" value={draft.group} onChange={e => patch({ group: e.target.value, asset: seed(e.target.value), expected: null })}>{groups.map(g => <option key={g} value={g}>{titles[g]}</option>)}</select></label>
      <div className="toolbar"><button className="button secondary" onClick={() => patch({ group: draft.group, asset: seed(draft.group), expected: null })}>新建资产</button><button className="button secondary" onClick={() => void reload().catch(props.fail)}>刷新目录</button></div>
      <label>读取资产<select aria-label="读取资产" value="" onChange={e => { const item = items.find(i => i.id === e.target.value); if (item) patch({ group: draft.group, asset: clone(item.asset), expected: item.fingerprint }); }}><option value="">选择资产；读取会替换当前编辑草稿</option>{items.map(i => <option key={i.id} value={i.id}>{i.title} ({i.id})</option>)}</select></label>
      <JsonEditor label="资产内容" value={draft.asset} draft={draft.raw} onRawChange={raw => patch({ ...draft, raw })} onChange={asset => patch({ ...draft, asset, raw: undefined })} onError={setMessage} />
      <div className="toolbar"><button className="button primary" disabled={draft.raw !== undefined} onClick={() => void save()}>保存资产</button><button className="button secondary" disabled={!draft.expected} onClick={async () => { try { await action(prefix + ".remove", { group: draft.group, name: draft.asset.id, expected: draft.expected }); patch({ group: draft.group, asset: seed(draft.group), expected: null }); await reload(); } catch (e) { props.fail(e); } }}>删除当前资产</button></div>
      {message && <p role="status">{message}</p>}
    </div></section>
  </div>;
}

export function ParameterSelection({ value = {}, onChange, nc = false, slots = [] }: { value?: Data; onChange: (value: Data) => void; nc?: boolean; slots?: string[] }) {
  const [presets, setPresets] = useState<Data[]>([]), [tools, setTools] = useState<Data[]>([]), [materials, setMaterials] = useState<Data[]>([]), [candidates, setCandidates] = useState<Record<string, Data[]>>({});
  const [preview, setPreview] = useState<Data>(), [error, setError] = useState("");
  const [currentSignature, setCurrentSignature] = useState("");
  useEffect(() => { let alive = true; const load = async () => {
    const p = await action("parameter.catalog", { group: "presets" }); if (alive) setPresets(p.data.items);
    if (nc) { const [t, m] = await Promise.all([action("nc.parameter.catalog", { group: "tools" }), action("nc.parameter.catalog", { group: "materials" })]); if (alive) { setTools(t.data.items); setMaterials(m.data.items); } }
  }; void load().catch(e => alive && setError((e as Error).message)); return () => { alive = false; }; }, [nc]);
  function patch(next: Data) { onChange(next); setError(""); }
  async function changeSlot(slot: string, changes: Data) {
    const choice = { ...value.slots?.[slot], ...changes }; patch({ ...value, slots: { ...value.slots, [slot]: choice } });
  }
  const names = [...new Set([...slots, ...Object.keys(value.slots || {})])];
  const conditions = encode(names.map(slot => [slot, value.slots?.[slot]?.tool_id, value.slots?.[slot]?.material_id, value.slots?.[slot]?.operation]));
  useEffect(() => {
    let alive = true;
    if (nc) for (const slot of names) {
      const choice = value.slots?.[slot] || {};
      if (choice.tool_id && choice.material_id && choice.operation) {
        void action("nc.parameters.match", { tool_id: choice.tool_id, material_id: choice.material_id, operation: choice.operation })
          .then(result => alive && setCandidates(old => ({ ...old, [slot]: result.data.items })))
          .catch(e => alive && setError((e as Error).message));
      } else setCandidates(old => ({ ...old, [slot]: [] }));
    }
    return () => { alive = false; };
  }, [nc, conditions]);
  return <div className="parameter-selection">
    <label>共享预置<select aria-label="共享预置" value={value.preset_id || ""} onChange={e => { const next = { ...value }; if (e.target.value) next.preset_id = e.target.value; else delete next.preset_id; patch(next); }}><option value="">不选择共享预置</option>{presets.map(p => <option value={p.id} key={p.id}>{p.title}</option>)}</select></label>
    {nc && names.map(slot => { const choice = value.slots?.[slot] || {}; const resolved = preview?.slots?.[slot]; return <fieldset key={slot}><legend>{slot} 刀具槽位</legend>
      <label>刀具<select aria-label={slot + "刀具"} value={choice.tool_id || ""} onChange={e => { if (!e.target.value) { const next = { ...value, slots: { ...value.slots } }; delete next.slots[slot]; patch(next); } else void changeSlot(slot, { tool_id: e.target.value }); }}><option value="">使用原始刀具输入</option>{tools.map(t => <option key={t.id} value={t.id}>{t.title}</option>)}</select></label>
      {choice.tool_id && <><label>材料<select aria-label={slot + "材料"} value={choice.material_id || ""} onChange={e => void changeSlot(slot, { material_id: e.target.value })}><option value="">选择材料</option>{materials.map(m => <option key={m.id} value={m.id}>{m.title}</option>)}</select></label>
      <label>工序<select aria-label={slot + "工序"} value={choice.operation || ""} onChange={e => void changeSlot(slot, { operation: e.target.value })}><option value="">选择工序</option><option value="milling">铣削</option><option value="drilling">钻削</option><option value="turning">车削</option></select></label>
      <label>加工预置<select aria-label={slot + "加工预置"} value={choice.manual ? "manual" : choice.profile_id || ""} onChange={e => void changeSlot(slot, { profile_id: e.target.value === "manual" ? undefined : e.target.value, manual: e.target.value === "manual", ...(e.target.value === "manual" ? { mode: "fixed", feed_mode: "per_minute", inputs: {} } : {}) })}><option value="">明确选择加工预置</option>{(candidates[slot] || []).map(p => <option value={p.name} key={p.name}>{p.asset.title}</option>)}<option value="manual">手动模式</option></select></label>
      {choice.manual && <JsonEditor label={slot + "手动加工值"} value={{ mode: choice.mode, feed_mode: choice.feed_mode, inputs: choice.inputs }} draft={choice.inputs_draft} onRawChange={inputs_draft => void changeSlot(slot, { inputs_draft })} onChange={v => void changeSlot(slot, { ...v, inputs_draft: undefined })} onError={setError} />}
      <JsonEditor label={slot + "人工调整（rpm、feed）"} value={choice.overrides || {}} draft={choice.overrides_draft} onRawChange={overrides_draft => void changeSlot(slot, { overrides_draft })} onChange={overrides => void changeSlot(slot, { overrides, overrides_draft: undefined })} onError={setError} />
      <button className="text-button" onClick={() => void changeSlot(slot, { overrides: {}, overrides_draft: undefined, confirmed_for: undefined })}>恢复推荐值</button>
      {resolved && <><p>转速 {encode(resolved.values.rpm)} rpm；进给 {encode(resolved.values.feed)} {resolved.feed_unit}；来源 {resolved.origins.values}</p><p>刀具事实：{encode(resolved.attributes)}</p>{resolved.needs_confirmation && choice.confirmed_for !== resolved.confirmed_for && <button className="button secondary" disabled={currentSignature !== selectionSignature(value)} onClick={() => void changeSlot(slot, { confirmed_for: resolved.confirmed_for })}>确认本次人工调整</button>}{resolved.needs_confirmation && choice.confirmed_for === resolved.confirmed_for && currentSignature === selectionSignature(value) && <p role="status">本次调整已确认</p>}</>}
      </>}
    </fieldset>; })}
    {nc && <button className="button secondary" onClick={async () => { try { const signature = selectionSignature(value); const r = await action("nc.parameters.resolve", { selection: validatedSelection(value) }); setPreview(r.data); setCurrentSignature(signature); setError(""); } catch (e) { setError((e as Error).message); } }}>计算推荐与检查调整</button>}
    {preview && currentSignature !== selectionSignature(value) && <p role="status">加工选择已变化，请重新计算并确认调整。</p>}
    {error && <p className="inline-error" role="alert">{error}</p>}
  </div>;
}

export function ManagedProcess(props: ViewProps) {
  const key = "nctool:managed-process:" + props.workspaceId;
  const [draft, setDraft] = useState<Data>(() => { try { return parseData(localStorage.getItem(key) || ""); } catch { return { name: "program", ops: [] }; } });
  const [raw, setRaw] = useState<string | undefined>(() => localStorage.getItem(key + ":raw") ?? undefined), [error, setError] = useState(""), [run, setRun] = useState<string | null>(null), [signature, setSignature] = useState("");
  return <div className="page-content"><PageTitle title="受管工序与组合程序" description="按顺序生成工序，可绑定前置工序通道、推进同步编号并分配 MPF/SPF 文件。" /><section className="panel"><div className="panel-content">
    <JsonEditor label="组合任务" value={draft} draft={raw} onRawChange={text => { setRaw(text); localStorage.setItem(key + ":raw",text); }} onChange={next => { setDraft(next); setRaw(undefined); localStorage.removeItem(key + ":raw"); localStorage.setItem(key, encode(next)); }} onError={setError} />
    {error && <p role="alert">{error}</p>}<button className="button primary" disabled={raw !== undefined} onClick={async () => { try { if (!draft || typeof draft !== "object" || Array.isArray(draft)) throw new Error("组合任务必须是 JSON 对象"); setSignature(encode(draft)); setRun(await props.start("process.generate.managed", draft, draft.name || "组合程序")); } catch (e) { props.fail(e); } }}>生成全部文件</button>
    {run && props.runs[run] && <RunDetail run={props.runs[run]} stale={signature !== encode(draft)} />}
  </div></section></div>;
}
