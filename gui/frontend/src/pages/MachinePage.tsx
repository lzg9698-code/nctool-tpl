import { useEffect, useMemo, useState } from "react";
import { api } from "../api/nctool";
import { useAppStore } from "../stores/appStore";
import type { CommandError, MachineKeySpec, MachineSummary } from "../types";

type MachineForm = { id: string; vendor: string; model: string; config: Record<string, string> };
const blank: MachineForm = { id: "", vendor: "", model: "", config: {} };

export default function MachinePage() {
  const machines = useAppStore((s) => s.machines);
  const loadMachines = useAppStore((s) => s.loadMachines);
  const loadConfig = useAppStore((s) => s.loadConfig);
  const setStatus = useAppStore((s) => s.setStatus);
  const [schema, setSchema] = useState<MachineKeySpec[]>([]);
  const [fingerprint, setFingerprint] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState("");
  const [form, setForm] = useState<MachineForm>(blank);
  const [creating, setCreating] = useState(false);
  const [busy, setBusy] = useState(false);

  const refresh = async () => {
    try {
      const result = await api.listMachines();
      setSchema(result.schema);
      setFingerprint(result.fileFingerprint);
      await loadMachines();
    } catch (e) {
      setStatus({ level: "err", message: `读取机床配置失败：${(e as CommandError).message}` });
    }
  };

  useEffect(() => { void refresh(); }, []);

  const selected = machines.find((m) => m.id === selectedId) ?? null;
  const known = useMemo(() => new Set(schema.map((s) => s.key)), [schema]);
  const extraKeys = Object.keys(form.config).filter((key) => !known.has(key)).sort();

  const chooseMachine = (machine: MachineSummary | undefined) => {
    setCreating(false);
    setSelectedId(machine?.id ?? "");
    if (!machine) { setForm(blank); return; }
    setForm({ id: machine.id, vendor: machine.vendor, model: machine.model, config: { ...machine.config } });
  };

  const startCreate = () => {
    const base = machines.find((m) => m.id === "generic");
    setCreating(true);
    setSelectedId("");
    setForm({ id: "", vendor: base?.vendor ?? "", model: base?.model ?? "", config: { ...(base?.config ?? {}) } });
  };

  const setField = (key: string, value: string) => setForm((f) => ({ ...f, [key]: value }));
  const setConfig = (key: string, value: string) => setForm((f) => ({ ...f, config: { ...f.config, [key]: value } }));

  const save = async () => {
    if (!form.id.trim() || !form.vendor.trim() || !form.model.trim()) {
      setStatus({ level: "warn", message: "机床 ID、厂商和型号都必须填写" });
      return;
    }
    setBusy(true);
    try {
      const res = await api.saveMachine({ ...form, id: form.id.trim(), vendor: form.vendor.trim(), model: form.model.trim() }, fingerprint);
      setFingerprint(res.fileFingerprint);
      await refresh();
      await loadConfig();
      setCreating(false);
      setSelectedId(res.machine.id);
      setStatus({ level: "ok", message: `机床配置已${res.action === "created" ? "创建" : "保存"}：${res.machine.id}${res.warnings.length ? `；${res.warnings.join("；")}` : ""}` });
    } catch (e) {
      const err = e as CommandError;
      setStatus({ level: "err", message: `保存机床失败：${err.message}` });
    } finally { setBusy(false); }
  };

  const remove = async () => {
    if (!selected || selected.builtin || !window.confirm(`确定删除自定义机床「${selected.id}」？`)) return;
    setBusy(true);
    try {
      await api.deleteMachine(selected.id, fingerprint);
      await refresh();
      await loadConfig();
      setForm(blank);
      setSelectedId("");
      setStatus({ level: "ok", message: `已删除机床配置：${selected.id}` });
    } catch (e) {
      setStatus({ level: "err", message: `删除失败：${(e as CommandError).message}` });
    } finally { setBusy(false); }
  };

  const renderKey = (spec: MachineKeySpec) => {
    const value = form.config[spec.key] ?? "";
    return <div className="machine-key" key={spec.key}>
      <label htmlFor={`machine-${spec.key}`}>{spec.key}<small>{spec.description}</small></label>
      {spec.kind === "Choice" && spec.options?.length ? <select id={`machine-${spec.key}`} value={value} onChange={(e) => setConfig(spec.key, e.target.value)}>
        {spec.options.map((option) => <option key={option} value={option}>{option}</option>)}
      </select> : <input id={`machine-${spec.key}`} type="text" value={value} placeholder={`默认：${spec.default}`} onChange={(e) => setConfig(spec.key, e.target.value)} />}
    </div>;
  };

  return <div className="page">
    <div className="toolbar">
      <strong>机床配置</strong><span className="muted">内置预设只读；自定义配置写入项目 nctool.toml</span>
      <span className="spacer" />
      <button className="btn" onClick={() => void refresh()} disabled={busy}>刷新</button>
      <button className="btn btn-primary" onClick={startCreate} disabled={busy}>新增自定义机床</button>
    </div>
    <div className="page-body manager-layout">
      <section className="manager-list">
        <label htmlFor="machine-pick">预设与自定义机床</label>
        <select id="machine-pick" size={12} value={selectedId} onChange={(e) => chooseMachine(machines.find((m) => m.id === e.target.value))}>
          {machines.map((m) => <option key={m.id} value={m.id}>{m.id} · {m.vendor} {m.model}{m.builtin ? "（内置）" : "（自定义）"}</option>)}
        </select>
        {selected && <p className="muted machine-note">{selected.builtin ? "这是内置基线。点击“新增自定义机床”可从 generic 配置派生。" : "可编辑的项目机床配置。保存时会校验配置键和值。"}</p>}
      </section>
      <section className="manager-editor">
        {!selected && !creating ? <div className="nc-empty">选择一台机床查看完整配置，或新建自定义机床。</div> : <>
          <div className="panel-head"><div className="panel-title">{creating ? "新增自定义机床" : `${form.id} 配置`}</div>
            {!creating && selected && !selected.builtin && <button className="btn btn-danger" onClick={() => void remove()} disabled={busy}>删除</button>}
          </div>
          <div className="form-grid">
            <div className="form-row"><label>机床 ID</label><input value={form.id} disabled={!creating} onChange={(e) => setField("id", e.target.value)} placeholder="例如 lathe_custom" /></div>
            <div className="form-row"><label>厂商</label><input value={form.vendor} disabled={!creating && !!selected?.builtin} onChange={(e) => setField("vendor", e.target.value)} /></div>
            <div className="form-row"><label>型号</label><input value={form.model} disabled={!creating && !!selected?.builtin} onChange={(e) => setField("model", e.target.value)} /></div>
          </div>
          <div className="machine-schema-grid">{schema.map(renderKey)}</div>
          {extraKeys.length > 0 && <div className="machine-extra"><strong>扩展键（保留）</strong><div className="machine-schema-grid">{extraKeys.map((key) => <div className="machine-key" key={key}><label>{key}<small>未知键允许扩展，保存时会提示</small></label><input value={form.config[key]} onChange={(e) => setConfig(key, e.target.value)} /></div>)}</div></div>}
          {(!selected?.builtin || creating) && <div className="machine-actions"><button className="btn btn-primary" onClick={() => void save()} disabled={busy}>{busy ? "保存中…" : "保存配置"}</button></div>}
        </>}
      </section>
    </div>
  </div>;
}
