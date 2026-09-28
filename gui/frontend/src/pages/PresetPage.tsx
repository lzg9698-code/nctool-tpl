//! 参数预设页（Phase 2 第一批）。
//!
//! 复用 cli 契约层三条**已有端点**：`list_presets`（GET /api/presets）、
//! `save_preset`（POST /api/presets）、`delete_preset`（POST /api/presets/delete）。
//!
//! ★ **"应用预设"**：`GET /api/presets` 列表项已带**扁平** `params` 取值
//! （`server.rs::presets_list`），故可从列表直接"应用"——把取值写入渲染页表单并
//! 切到渲染页（`appStore.applyPreset`，与 `RenderPage` 表单状态对齐）。
//! **陈旧（stale）不是不能应用，而是应用前你得知道**（与 Web UI 同一口径）：
//! 陈旧项给警告提示后再应用；模板缺失（`resolvable=false`）的项禁用"应用"。
//!
//! ★ 参数通道安全：`savePreset` 的 `paramsJson` 是 `JSON.stringify(params)`，
//! Rust 侧**文本拼接**（保留下溢守卫）。

import { useCallback, useEffect, useState } from "react";
import { api } from "../api/nctool";
import VarField from "../components/VarField";
import { useAppStore } from "../stores/appStore";
import type { CommandError, ParamSpec, PresetSummary } from "../types";

/** 由 `ParamSpec` 初始化表单值（默认值优先，其次按类型给中性值）。 */
function initialParams(spec: ParamSpec[]): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const s of spec) {
    if (s.default !== null && s.default !== undefined) out[s.name] = s.default;
    else if (s.kind === "Bool") out[s.name] = false;
    else out[s.name] = "";
  }
  return out;
}

/** `WriteAction` → 中文（serde 小写：created/updated/unchanged/deleted）。 */
const ACTION_LABEL: Record<string, string> = {
  created: "已创建",
  updated: "已覆盖",
  unchanged: "无变化",
  deleted: "已删除",
};

export default function PresetPage() {
  const templates = useAppStore((s) => s.templates);
  const setStatus = useAppStore((s) => s.setStatus);
  const setPage = useAppStore((s) => s.setPage);
  const applyPreset = useAppStore((s) => s.applyPreset);

  // 列表
  const [filter, setFilter] = useState<string>("");
  const [presets, setPresets] = useState<PresetSummary[]>([]);
  const [presetPath, setPresetPath] = useState<string>("");
  const [presetWarnings, setPresetWarnings] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);

  // 保存表单
  const [formTemplate, setFormTemplate] = useState<string>("");
  const [spec, setSpec] = useState<ParamSpec[]>([]);
  const [params, setParams] = useState<Record<string, unknown>>({});
  const [presetName, setPresetName] = useState<string>("");
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const data = await api.listPresets(filter || undefined);
      setPresets(data.presets);
      setPresetPath(data.path);
      setPresetWarnings(data.warnings ?? []);
    } catch (e) {
      const err = e as CommandError;
      setStatus({ level: "err", message: `加载预设失败：${err.message}` });
    } finally {
      setLoading(false);
    }
  }, [filter, setStatus]);

  useEffect(() => {
    const st = useAppStore.getState();
    if (st.templates.length === 0) void st.loadTemplates();
  }, []);

  // 过滤变化 → 重新拉取。
  useEffect(() => {
    void load();
  }, [load]);

  // 选择模板 → 取规格 + 初始化表单。
  useEffect(() => {
    if (!formTemplate) {
      setSpec([]);
      setParams({});
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const { template } = await api.getTemplate(formTemplate);
        if (cancelled) return;
        setSpec(template.params);
        setParams(initialParams(template.params));
      } catch (e) {
        if (cancelled) return;
        const err = e as CommandError;
        setStatus({ level: "err", message: `加载模板失败：${err.message}` });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [formTemplate, setStatus]);

  const doSave = async (force: boolean) => {
    setSaving(true);
    try {
      // ★ 硬约束：paramsJson = JSON.stringify(params)（保留下溢守卫）
      const paramsJson = JSON.stringify(params);
      const res = await api.savePreset(
        presetName.trim(),
        formTemplate,
        paramsJson,
        force,
      );
      setStatus({
        level: "ok",
        message: `预设${ACTION_LABEL[res.action] ?? res.action}：${res.name}`,
      });
      await load();
    } catch (e) {
      const err = e as CommandError;
      if (err.kind === "name_conflict") {
        // 同名默认拒绝：确认后带 force=true 覆盖。
        if (window.confirm(`同名预设「${presetName.trim()}」已存在，是否覆盖？`)) {
          setSaving(false);
          await doSave(true);
          return;
        }
        setStatus({ level: "warn", message: `未覆盖同名预设：${presetName.trim()}` });
        return;
      }
      setStatus({
        level: "err",
        message: `保存失败（${err.kind}）：${err.message}`,
      });
    } finally {
      setSaving(false);
    }
  };

  const onSave = () => {
    if (!formTemplate) {
      setStatus({ level: "warn", message: "请先选择模板" });
      return;
    }
    if (!presetName.trim()) {
      setStatus({ level: "warn", message: "请填写预设名" });
      return;
    }
    void doSave(false);
  };

  const onDelete = async (name: string) => {
    if (!window.confirm(`确认删除预设「${name}」？`)) return;
    try {
      const res = await api.deletePreset(name);
      setStatus({ level: "ok", message: `预设${ACTION_LABEL[res.action] ?? res.action}：${name}` });
      await load();
    } catch (e) {
      const err = e as CommandError;
      setStatus({
        level: "err",
        message: `删除失败（${err.kind}）：${err.message}`,
      });
    }
  };

  /** 陈旧标记（resolvable=false → 模板缺失；stale=true → 陈旧 + 明细）。 */
  const staleOf = (
    p: PresetSummary,
  ): { text: string; cls: string } | null => {
    if (!p.resolvable) return { text: "模板缺失（无法检测陈旧）", cls: "sb-err" };
    if (p.stale === true) {
      const extra = [
        p.staleParams && p.staleParams.length
          ? `陈旧参数 ${p.staleParams.join(", ")}`
          : "",
        p.missingRequired && p.missingRequired.length
          ? `缺必选 ${p.missingRequired.join(", ")}`
          : "",
      ]
        .filter(Boolean)
        .join(" · ");
      return { text: `陈旧${extra ? "（" + extra + "）" : ""}`, cls: "sb-warn" };
    }
    return null;
  };

  /**
   * 应用预设 → 写入渲染页参数并切到渲染页。
   *
   * 与 Web UI 的 `loadPreset` 同一口径：
   * - 模板缺失（`resolvable=false`）→ 无法对齐表单，**禁用**（此处再兜一层）；
   * - 陈旧（`stale=true`）→ **允许**应用，但先给警告提示（"载入前你得知道"）；
   * - 参数整体覆盖 `appStore.params`（经 `applyPreset`，与 `RenderPage` 对齐）。
   */
  const onApply = async (p: PresetSummary) => {
    if (!p.resolvable) {
      setStatus({
        level: "warn",
        message: `预设「${p.name}」的模板「${p.template}」不可解析，无法应用`,
      });
      return;
    }
    try {
      await applyPreset(p.template, p.params ?? {});
      setPage("render");
      const st = staleOf(p);
      if (st) {
        setStatus({
          level: "warn",
          message: `已应用预设「${p.name}」：${st.text}——参数已填入，请复核后再生成`,
        });
      } else {
        setStatus({ level: "ok", message: `已应用预设「${p.name}」` });
      }
    } catch (e) {
      const err = e as CommandError;
      setStatus({
        level: "err",
        message: `应用预设失败（${err.kind}）：${err.message}`,
      });
    }
  };

  return (
    <div className="page">
      <div className="toolbar">
        <label style={{ margin: 0 }}>模板过滤</label>
        <select
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          style={{ width: 240 }}
        >
          <option value="">（全部模板）</option>
          {templates.map((t) => (
            <option key={t.name} value={t.name}>
              {t.name}
            </option>
          ))}
        </select>
        <button className="btn" onClick={() => void load()} disabled={loading}>
          ↻ 刷新
        </button>
        <span className="spacer" />
        <span className="path" title={presetPath}>
          {presetPath}
        </span>
      </div>

      <div className="page-body">
        {presetWarnings.length > 0 && (
          <div className="nc-blocked" style={{ marginBottom: ".8rem" }}>
            ⚠ 预设文件降级警告：{presetWarnings.join("；")}
          </div>
        )}
        <div className="split-view preset-split">
          <div className="split-left">
            <h4 className="panel-title">预设列表（{presets.length}）</h4>
            {presets.length === 0 && (
              <div className="muted">
                暂无预设。在右侧选模板、填参数后保存。
              </div>
            )}
            {presets.map((p) => {
              const st = staleOf(p);
              return (
                <div className="preset-item" key={p.name}>
                  <div className="preset-head">
                    <span className="preset-name">{p.name}</span>
                    <span className="preset-actions">
                      <button
                        className="btn btn-primary"
                        onClick={() => void onApply(p)}
                        disabled={!p.resolvable}
                        title={
                          !p.resolvable
                            ? "模板缺失，无法应用"
                            : st
                              ? `陈旧：${st.text}（可应用，请复核）`
                              : `应用预设「${p.name}」`
                        }
                      >
                        应用
                      </button>
                      <button
                        className="btn btn-danger"
                        onClick={() => void onDelete(p.name)}
                      >
                        删除
                      </button>
                    </span>
                  </div>
                  <div className="preset-meta">
                    <span className="tag">{p.template}</span>
                    <span className="muted">{p.paramCount} 参数</span>
                    <span className="muted">{p.createdAt}</span>
                  </div>
                  {st && <div className={`preset-stale ${st.cls}`}>● {st.text}</div>}
                </div>
              );
            })}
          </div>

          <div className="split-divider" />

          <div className="split-right">
            <h4 className="panel-title">保存为新预设</h4>
            <div className="form-row">
              <label>模板</label>
              <select
                value={formTemplate}
                onChange={(e) => setFormTemplate(e.target.value)}
              >
                <option value="">— 请选择模板 —</option>
                {templates.map((t) => (
                  <option key={t.name} value={t.name}>
                    {t.description} — {t.name}
                  </option>
                ))}
              </select>
            </div>
            <div className="form-row">
              <label>预设名</label>
              <input
                type="text"
                value={presetName}
                placeholder="例如 粗加工-Φ12"
                onChange={(e) => setPresetName(e.target.value)}
              />
            </div>

            {spec.length === 0 ? (
              <div className="muted">选择模板后填写参数。</div>
            ) : (
              <>
                {spec.map((s) => (
                  <VarField
                    key={s.name}
                    spec={s}
                    value={params[s.name]}
                    onChange={(n, v) =>
                      setParams((prev) => ({ ...prev, [n]: v }))
                    }
                  />
                ))}
                <button
                  className="btn btn-primary"
                  onClick={onSave}
                  disabled={saving || !formTemplate}
                >
                  💾 保存预设
                </button>
                <div className="muted" style={{ marginTop: ".5rem" }}>
                  同名预设默认拒绝保存，确认后覆盖（force）。
                </div>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
