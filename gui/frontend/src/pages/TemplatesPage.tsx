import { useEffect, useState } from "react";
import { api } from "../api/nctool";
import { useAppStore } from "../stores/appStore";
import type { CommandError, TemplateDetail } from "../types";

const CATEGORIES = [
  ["general", "通用"],
  ["milling", "铣削"],
  ["turning", "车削"],
  ["drilling", "钻孔"],
  ["grooving", "切槽"],
  ["machine", "机床"],
];

export default function TemplatesPage() {
  const templates = useAppStore((s) => s.templates);
  const loadTemplates = useAppStore((s) => s.loadTemplates);
  const setStatus = useAppStore((s) => s.setStatus);
  const [selected, setSelected] = useState("");
  const [detail, setDetail] = useState<TemplateDetail | null>(null);
  const [source, setSource] = useState("");
  const [createName, setCreateName] = useState("");
  const [category, setCategory] = useState("general");
  const [newName, setNewName] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!templates.length) void loadTemplates();
  }, [templates.length, loadTemplates]);

  const loadDetail = async (name: string) => {
    setSelected(name);
    setDetail(null);
    setSource("");
    if (!name) return;
    try {
      const { template } = await api.getTemplate(name);
      setDetail(template);
      setSource(template.source);
    } catch (e) {
      setStatus({ level: "err", message: `载入模板失败：${(e as CommandError).message}` });
    }
  };

  const refresh = async (message: string) => {
    await loadTemplates();
    if (selected) await loadDetail(selected);
    setStatus({ level: "ok", message });
  };

  const create = async () => {
    if (!createName.trim()) return;
    setBusy(true);
    try {
      const result = await api.createTemplate(createName.trim(), category);
      setCreateName("");
      await loadTemplates();
      await loadDetail(result.name);
      setStatus({ level: result.manifestWarning ? "warn" : "ok", message: `模板已创建：${result.name}${result.manifestWarning ? `；${result.manifestWarning}` : ""}` });
    } catch (e) {
      const err = e as CommandError;
      setStatus({ level: "err", message: `创建失败：${err.message}` });
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    if (!detail?.fingerprint || detail.builtin) return;
    setBusy(true);
    try {
      await api.saveTemplate(detail.name, source, detail.fingerprint);
      await refresh(`模板已保存：${detail.name}`);
    } catch (e) {
      const err = e as CommandError;
      setStatus({ level: "err", message: `保存失败：${err.message}` });
    } finally {
      setBusy(false);
    }
  };

  const derive = async () => {
    if (!detail || !newName.trim()) return;
    setBusy(true);
    try {
      const result = await api.deriveTemplate(detail.name, newName.trim());
      setNewName("");
      await loadTemplates();
      await loadDetail(result.name);
      setStatus({ level: "ok", message: `已派生模板：${result.name}${result.manifestWarning ? `；${result.manifestWarning}` : ""}` });
    } catch (e) {
      setStatus({ level: "err", message: `派生失败：${(e as CommandError).message}` });
    } finally {
      setBusy(false);
    }
  };

  const rename = async () => {
    if (!detail || detail.builtin || !newName.trim()) return;
    setBusy(true);
    try {
      const result = await api.renameTemplate(detail.name, newName.trim());
      setNewName("");
      await loadTemplates();
      await loadDetail(result.name);
      setStatus({ level: "ok", message: `模板已重命名：${result.oldName} → ${result.name}${result.manifestWarning ? `；${result.manifestWarning}` : ""}` });
    } catch (e) {
      setStatus({ level: "err", message: `重命名失败：${(e as CommandError).message}` });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="page">
      <div className="toolbar">
        <strong>模板管理</strong>
        <span className="muted">{templates.length} 个模板 · 内置模板只读</span>
        <span className="spacer" />
        <button className="btn" onClick={() => void refresh("模板列表已刷新")} disabled={busy}>刷新</button>
      </div>
      <div className="page-body manager-layout">
        <section className="manager-list">
          <div className="panel-title">新建模板</div>
          <div className="form-row">
            <input aria-label="新模板名" value={createName} onChange={(e) => setCreateName(e.target.value)} placeholder="模板名（自动添加 .j2）" />
          </div>
          <div className="form-row">
            <select aria-label="模板分类" value={category} onChange={(e) => setCategory(e.target.value)}>
              {CATEGORIES.map(([id, label]) => <option key={id} value={id}>{label}</option>)}
            </select>
          </div>
          <button className="btn btn-primary full-width" onClick={() => void create()} disabled={busy || !createName.trim()}>创建空白模板</button>
          <div className="manager-divider" />
          <label htmlFor="template-pick">模板列表</label>
            <select id="template-pick" size={14} value={selected} onChange={(e) => void loadDetail(e.target.value)}>
            {templates.map((t) => <option key={t.name} value={t.name}>{t.name} · {t.category}{t.status === "unreviewed" ? " · 未评审" : t.status === "reviewed" ? " · 已评审" : t.status === "verified" ? " · 已空运行" : ""}</option>)}
          </select>
        </section>

        <section className="manager-editor">
          {!detail ? <div className="nc-empty">选择一个模板查看参数与源码。</div> : <>
            <div className="panel-head">
              <div>
                <div className="panel-title">{detail.name}</div>
                <div className="muted">{detail.description || "无描述"} · {detail.builtin ? "内置只读" : "磁盘模板，可编辑"}{detail.status === "unreviewed" ? " · 未经工艺评审" : detail.status === "reviewed" ? " · 已工艺评审" : detail.status === "verified" ? " · 已机床空运行" : ""}</div>
              </div>
              <button className="btn btn-primary" onClick={() => void save()} disabled={busy || detail.builtin || source === detail.source}>保存源码</button>
            </div>
            {detail.params.length > 0 && <div className="template-param-list">
              <strong>参数规格</strong>
              {detail.params.map((p) => <span key={p.name} className="tag">{p.name} · {p.kind}{p.required ? " · 必填" : ""}</span>)}
            </div>}
            <textarea className="source-editor" spellCheck={false} value={source} readOnly={detail.builtin} onChange={(e) => setSource(e.target.value)} aria-label="模板源码" />
            {!detail.builtin && <div className="template-actions">
              <input aria-label="新名称" value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="新名称（同目录）" />
              <button className="btn" disabled={busy || !newName.trim()} onClick={() => void derive()}>派生</button>
              <button className="btn" disabled={busy || !newName.trim()} onClick={() => void rename()}>重命名</button>
            </div>}
          </>}
        </section>
      </div>
    </div>
  );
}
