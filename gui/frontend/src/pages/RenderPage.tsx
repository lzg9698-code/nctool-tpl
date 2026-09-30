import { useEffect } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api } from "../api/nctool";
import NcPreview from "../components/NcPreview";
import VarField from "../components/VarField";
import { useAppStore } from "../stores/appStore";
import type { CommandError } from "../types";

export default function RenderPage() {
  const templates = useAppStore((s) => s.templates);
  const machines = useAppStore((s) => s.machines);
  const selected = useAppStore((s) => s.selected);
  const selectedTemplate = templates.find((t) => t.name === selected);
  const spec = useAppStore((s) => s.spec);
  const params = useAppStore((s) => s.params);
  const options = useAppStore((s) => s.options);
  const output = useAppStore((s) => s.output);
  const blocked = useAppStore((s) => s.blocked);
  const renderError = useAppStore((s) => s.renderError);
  const rendering = useAppStore((s) => s.rendering);
  const selectTemplate = useAppStore((s) => s.selectTemplate);
  const setParam = useAppStore((s) => s.setParam);
  const setOption = useAppStore((s) => s.setOption);
  const renderNow = useAppStore((s) => s.renderNow);
  const resetParams = useAppStore((s) => s.resetParams);
  const setStatus = useAppStore((s) => s.setStatus);

  useEffect(() => {
    const st = useAppStore.getState();
    void st.loadTemplates();
    void st.loadMachines();
  }, []);

  const onSave = async () => {
    try {
      if (!output) {
        setStatus({ level: "warn", message: "暂无可保存的 NC 代码" });
        return;
      }
      const base = selected
        ? (selected.replace(/\.j2$/i, "").split("/").pop() ?? "output")
        : "output";
      const path = await save({
        defaultPath: `${base}.nc`,
        filters: [
          { name: "NC 程序", extensions: ["nc", "mpf", "spf", "tap", "txt"] },
        ],
      });
      if (!path) return;
      await api.saveNcFile(path, output);
      setStatus({ level: "ok", message: `已保存：${path}` });
    } catch (e) {
      const err = e as CommandError;
      setStatus({
        level: "err",
        message: `保存失败：${err.message ?? String(e)}`,
      });
    }
  };

  return (
    <div className="page">
      <div className="toolbar">
        <button
          className="btn btn-primary"
          onClick={() => void renderNow()}
          disabled={!selected || rendering}
        >
          ⚡ 生成 NC
        </button>
        <button className="btn" onClick={() => void onSave()} disabled={!output}>
          💾 保存…
        </button>
        <button className="btn" onClick={resetParams} disabled={!selected}>
          ↺ 重置参数
        </button>
        <span className="spacer" />
        <span className="path">{selectedTemplate?.status === "unreviewed" ? `⚠ 未评审 · ${selected}` : selected ?? "未选择模板"}</span>
      </div>

      <div className="page-body">
        {selectedTemplate?.status === "unreviewed" && (
          <div className="config-warning" role="alert">
            此模板尚未经工艺评审或目标机床空运行验证。生成结果仅供模板开发与检查，投产前必须由工艺人员核对。
          </div>
        )}
        <div className="form-row">
          <label>模板选择</label>
          <select
            value={selected ?? ""}
            onChange={(e) => void selectTemplate(e.target.value)}
          >
            <option value="">— 请选择模板 —</option>
            {templates.map((t) => (
              <option key={t.name} value={t.name}>
                {t.status === "unreviewed" ? "⚠ 未评审 · " : t.status === "reviewed" ? "已评审 · " : t.status === "verified" ? "已空运行 · " : ""}{t.description} — {t.name}
              </option>
            ))}
          </select>
        </div>

        <div className="form-grid">
          <div className="form-row">
            <label>机床型号</label>
            <select
              value={options.machine ?? ""}
              onChange={(e) => {
                setOption("machine", e.target.value || null);
                void renderNow();
              }}
            >
              <option value="">默认（generic）</option>
              {machines.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.id}
                  {m.builtin ? "（内建）" : "（自定义）"}
                </option>
              ))}
            </select>
          </div>
          <div className="form-row">
            <label>行号步长（默认 10）</label>
            <input
              type="number"
              value={options.lineStep}
              onChange={(e) =>
                setOption("lineStep", Number(e.target.value) || 0)
              }
            />
          </div>
        </div>

        <div className="options-bar">
          <label className="opt-toggle">
            <input
              type="checkbox"
              checked={options.lineNumbers}
              onChange={(e) => setOption("lineNumbers", e.target.checked)}
            />
            行号
          </label>
          <label className="opt-toggle">
            <input
              type="checkbox"
              checked={options.addHeader}
              onChange={(e) => setOption("addHeader", e.target.checked)}
            />
            头部注释
          </label>
          <label className="opt-toggle">
            <input
              type="checkbox"
              checked={options.stripBlank}
              onChange={(e) => setOption("stripBlank", e.target.checked)}
            />
            清理空行
          </label>
          <label className="opt-toggle">
            <input
              type="checkbox"
              checked={options.ascii}
              onChange={(e) => setOption("ascii", e.target.checked)}
            />
            仅 ASCII
          </label>
          <label className="opt-toggle">
            <input
              type="checkbox"
              checked={options.lenient}
              onChange={(e) => setOption("lenient", e.target.checked)}
            />
            宽松模式
          </label>
          <span className="opt-field">
            最大行号
            <input
              type="number"
              value={options.maxLine}
              onChange={(e) => setOption("maxLine", Number(e.target.value) || 0)}
            />
          </span>
        </div>

        <div className="split-view">
          <div className="split-left">
            <h4 className="panel-title">
              变量参数{rendering ? " · 渲染中…" : ""}
            </h4>
            {spec.length === 0 && (
              <div className="muted">请选择模板以生成参数表单。</div>
            )}
            {spec.map((s) => (
              <VarField
                key={s.name}
                spec={s}
                value={params[s.name]}
                onChange={setParam}
              />
            ))}
          </div>
          <div className="split-divider" />
          <div className="split-right">
            <div className="panel-head">
              <h4 className="panel-title">NC 代码预览</h4>
              <span className="muted">实时更新 ●</span>
            </div>
            <NcPreview code={output} blocked={blocked} error={renderError} />
          </div>
        </div>
      </div>
    </div>
  );
}
