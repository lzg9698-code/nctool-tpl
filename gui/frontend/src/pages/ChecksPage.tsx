//! 静态检查页（设计稿 §2c）：上块 validate 报告（有级别）+ 下块 lint 发现项（无级别）。
//!
//! **界面硬约束（设计稿 §2c 明确批过）**：
//! - lint 结果**不得**标 ERROR/WARN/INFO、**不得**算"通过率"（后端给不出）；
//! - "存在 Error 时禁用生成"这条规则**只挂 validate**（`report.errors`）；
//! - validate 的 INFO 数需自行从 `issues` 统计（报告无 `infos` 计数字段）。

import { useEffect, useMemo, useState } from "react";
import { api } from "../api/nctool";
import { useAppStore } from "../stores/appStore";
import type { CommandError, LintFinding, ValidationReport } from "../types";

const LEVEL_LABEL: Record<ValidationReport["issues"][number]["level"], string> = {
  error: "ERROR",
  warning: "WARN",
  info: "INFO",
};

export default function ChecksPage() {
  const templates = useAppStore((s) => s.templates);
  const selected = useAppStore((s) => s.selected);
  const storeParams = useAppStore((s) => s.params);
  const setStatus = useAppStore((s) => s.setStatus);

  const [template, setTemplate] = useState<string>("");
  const [source, setSource] = useState<string>("");
  const [report, setReport] = useState<ValidationReport | null>(null);
  const [findings, setFindings] = useState<LintFinding[] | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<{ kind: string; message: string } | null>(
    null,
  );

  useEffect(() => {
    const st = useAppStore.getState();
    if (st.templates.length === 0) void st.loadTemplates();
  }, []);

  // 默认选中渲染页当前模板（切过来即可直接检查）。
  useEffect(() => {
    if (!template && selected) setTemplate(selected);
  }, [selected, template]);

  // 载入模板源码：供 lint 发现项定位与高亮（lint 自带行列）。
  useEffect(() => {
    if (!template) {
      setSource("");
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const { template: detail } = await api.getTemplate(template);
        if (!cancelled) setSource(detail.source);
      } catch {
        if (!cancelled) setSource("");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [template]);

  const runChecks = async () => {
    if (!template) return;
    setRunning(true);
    setError(null);
    try {
      // validate 用渲染页当前参数（仅当模板一致），否则用空参数集。
      const params = template === selected ? storeParams : {};
      const paramsJson = JSON.stringify(params);
      const [v, l] = await Promise.all([
        api.validateTemplate(template, paramsJson),
        api.lintTemplate(template),
      ]);
      setReport((v.report ?? null) as ValidationReport | null);
      setFindings(l);
      setStatus({ level: "ok", message: `检查完成：${template}` });
    } catch (e) {
      const err = e as CommandError;
      setError({ kind: err.kind, message: err.message });
      setReport(null);
      setFindings(null);
      setStatus({ level: "err", message: `检查失败：${err.message}` });
    } finally {
      setRunning(false);
    }
  };

  // 切换模板时清空上一次结果（避免旧结果挂在新模板上）。
  useEffect(() => {
    setReport(null);
    setFindings(null);
    setError(null);
  }, [template]);

  const infoCount = report
    ? report.issues.filter((i) => i.level === "info").length
    : 0;
  // ★ "可生成" 只由 validate 决定（lint 无级别、不参与）。
  const canGenerate = report ? report.errors === 0 : null;

  /** 需要高亮的源码行号集合（来自 lint 发现项）。 */
  const findingLines = useMemo(
    () => new Set((findings ?? []).map((f) => f.line)),
    [findings],
  );
  const sourceLines = useMemo(() => source.split(/\r?\n/), [source]);

  return (
    <div className="page">
      <div className="toolbar">
        <button
          className="btn btn-primary"
          onClick={() => void runChecks()}
          disabled={!template || running}
        >
          ▶ 执行检查
        </button>
        <select
          value={template}
          onChange={(e) => setTemplate(e.target.value)}
          style={{ width: 300 }}
        >
          <option value="">— 请选择模板 —</option>
          {templates.map((t) => (
            <option key={t.name} value={t.name}>
              {t.name}
            </option>
          ))}
        </select>
        <span className="spacer" />
        <span className="path">
          lint: 度制风险 1 条 · validate: 13 类 IssueKind
        </span>
      </div>

      <div className="page-body">
        {error && (
          <div className="nc-error" style={{ marginBottom: ".8rem" }}>
            <div className="nc-error-kind">{error.kind}</div>
            {error.message}
          </div>
        )}

        {!report && !findings && !error && (
          <div className="nc-empty">
            选择模板后点击「执行检查」：上块给出 validate 报告（有级别），下块列出
            lint 发现项（无级别）。
          </div>
        )}

        {report && (
          <>
            <p className="check-section">
              <strong>validate 报告</strong>（级别来自 ValidationLevel）
            </p>
            <div className="card-grid">
              <div className="card card-err">
                <div className="card-title">ERROR</div>
                <div className="card-value" style={{ color: "var(--err)" }}>
                  {report.errors}
                </div>
                <div className="card-desc">必须修复（阻断生成）</div>
              </div>
              <div className="card card-warn">
                <div className="card-title">WARN</div>
                <div className="card-value" style={{ color: "var(--warn)" }}>
                  {report.warnings}
                </div>
                <div className="card-desc">建议修复</div>
              </div>
              <div className="card card-info">
                <div className="card-title">INFO</div>
                <div className="card-value" style={{ color: "var(--info)" }}>
                  {infoCount}
                </div>
                <div className="card-desc">提示信息</div>
              </div>
              <div className="card card-ok">
                <div className="card-title">可生成</div>
                <div
                  className="card-value"
                  style={{ color: canGenerate ? "var(--ok)" : "var(--err)" }}
                >
                  {canGenerate ? "是" : "否"}
                </div>
                <div className="card-desc">
                  <code>has_errors()</code> = {String(report.errors > 0)}
                </div>
              </div>
            </div>

            {report.issues.length === 0 ? (
              <div className="muted" style={{ marginBottom: "1rem" }}>
                validate 通过：无问题。
              </div>
            ) : (
              report.issues.map((it, idx) => (
                <div className="lint-item" key={`v-${idx}`}>
                  <span className={`lvl lvl-${it.level}`}>
                    {LEVEL_LABEL[it.level]}
                  </span>
                  <span className="msg">{it.message}</span>
                  <span className="loc">
                    {it.param ? `param: ${it.param}` : "—"}
                  </span>
                </div>
              ))
            )}
          </>
        )}

        {findings && (
          <>
            <p className="check-section" style={{ marginTop: "1rem" }}>
              <strong>lint 发现项</strong>（LintFinding，<strong>无级别</strong>
              ；仅有 filter 与建议替换）
            </p>
            {findings.length === 0 ? (
              <div className="muted">静态检查通过：未发现问题。</div>
            ) : (
              <>
                {findings.map((f, idx) => (
                  <div className="lint-item" key={`l-${idx}`}>
                    <span className="lvl lvl-finding">FINDING</span>
                    <span className="msg">
                      <code>filter: {f.filter}</code> → 建议{" "}
                      <code>{f.suggestion}</code>；{f.message}
                    </span>
                    <span className="loc">
                      L:{f.line} C:{f.col}
                    </span>
                  </div>
                ))}
                {source && (
                  <div className="lint-snippet">
                    {sourceLines.map((line, i) => {
                      const no = i + 1;
                      const hit = findingLines.has(no);
                      return (
                        <div key={no} className={hit ? "hl" : undefined}>
                          <span className="ln">{no}</span>
                          {line}
                        </div>
                      );
                    })}
                  </div>
                )}
              </>
            )}
          </>
        )}
      </div>
    </div>
  );
}
