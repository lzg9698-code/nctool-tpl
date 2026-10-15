import {
  AlertTriangle,
  Check,
  ChevronRight,
  Copy,
  Download,
  FileCode2,
  LoaderCircle,
  Square,
} from "lucide-react";
import { Editor } from "../../components/Editor";
import { Empty } from "../../components/WorkbenchUi";
import { api } from "../../lib/api";
import { download, encode, type Data } from "../../lib/data";

import { stateNames, type WorkbenchController } from "./useWorkbenchController";
export function ResultPane({
  controller,
}: {
  controller: WorkbenchController;
}) {
  const {
    submitting,
    resultTab,
    setResultTab,
    doc,
    latest,
    managedDependencies,
    stale,
    notify,
    fail,
    editDoc,
    jumpDiagnostic,
    diagnostics,
    text,
    busy,
    inputError,
    exportReason,
  } = controller;
  if (!doc) return null;
  return (
    <section
      className={"result-pane " + (doc.mode === "edit" ? "author-result" : "")}
    >
      <div className="pane-toolbar">
        <div className="pane-tabs">
          <button
            className={resultTab === "result" ? "selected" : ""}
            onClick={() => setResultTab("result")}
          >
            生成结果
          </button>
          <button
            className={resultTab === "diagnostics" ? "selected" : ""}
            onClick={() => setResultTab("diagnostics")}
          >
            检查信息
            {diagnostics.length > 0 && (
              <span className="count-badge">{diagnostics.length}</span>
            )}
          </button>
          {latest?.result?.data.parameters && (
            <button
              className={resultTab === "parameters" ? "selected" : ""}
              onClick={() => setResultTab("parameters")}
            >
              参数与计算
            </button>
          )}
        </div>
        <div className="run-controls">
          <span
            role="status"
            className={
              "run-status " +
              (latest?.status || "idle") +
              (latest?.status === "succeeded" && (stale || inputError)
                ? " stale"
                : "")
            }
          >
            {busy ? (
              <LoaderCircle size={14} className="spin" />
            ) : (latest?.status === "succeeded" && (stale || inputError)) ||
              latest?.status === "failed" ||
              latest?.status === "interrupted" ? (
              <AlertTriangle size={14} />
            ) : latest?.status === "succeeded" ? (
              <Check size={14} />
            ) : latest?.status === "cancelled" ? (
              <Square size={14} />
            ) : (
              <FileCode2 size={14} />
            )}
            {submitting === doc.key
              ? "提交中"
              : latest?.status === "succeeded" && (stale || inputError)
                ? "结果已过期"
                : latest
                  ? stateNames[latest.status]
                  : "待生成"}
          </span>
          {latest && ["queued", "running"].includes(latest.status) && (
            <button
              className="text-button"
              onClick={() =>
                api("runs/" + latest.id + "/cancel", {}).catch(fail)
              }
            >
              <Square size={13} />
              取消任务
            </button>
          )}
        </div>
      </div>
      {(stale || inputError) && (
        <div className="stale-banner">
          <AlertTriangle size={13} />
          {managedDependencies.message ||
            "结果对应旧输入，请重新生成以应用当前修改。"}
        </div>
      )}
      {resultTab === "parameters" && latest?.result?.data.parameters ? (
        <div className="pane-scroll">
          <pre>
            {encode(
              {
                有效输入: latest?.result?.data.parameters?.values,
                参数来源: latest?.result?.data.parameters?.origins,
                单位: Object.fromEntries(
                  Object.entries(
                    latest?.result?.data.parameters?.definitions || {},
                  ).map(([path, definition]: [string, Data]) => [
                    path,
                    definition.unit || "",
                  ]),
                ),
              },
              true,
            )}
          </pre>
        </div>
      ) : resultTab === "diagnostics" ? (
        <div className="pane-scroll">
          {diagnostics.length ? (
            diagnostics.map((d, i) => (
              <button
                className={"diagnostic " + d.level}
                key={i}
                onClick={() => jumpDiagnostic(d)}
              >
                <AlertTriangle size={15} />
                <span>
                  {d.message}
                  {d.path && <small>{d.path}</small>}
                </span>
                <ChevronRight size={14} />
              </button>
            ))
          ) : (
            <Empty
              title="没有检查问题"
              description="生成时发现的问题会显示在这里。"
            />
          )}
        </div>
      ) : latest?.status === "succeeded" ? (
        <Editor
          value={String(text)}
          readonly
          language="text"
          label="生成结果"
        />
      ) : latest && ["queued", "running"].includes(latest.status) ? (
        <Empty
          icon={<LoaderCircle size={28} className="spin" />}
          title="正在生成文件"
          description="你可以继续处理其他模板，或在结果区顶部取消任务。"
        />
      ) : latest &&
        ["failed", "cancelled", "interrupted"].includes(latest.status) ? (
        <Empty
          icon={<AlertTriangle size={30} />}
          title={
            latest.status === "cancelled"
              ? "任务已取消"
              : latest.status === "interrupted"
                ? "任务已中断"
                : "生成失败"
          }
          description={
            latest.error?.message || "本次任务没有生成可交付文件，请重新生成。"
          }
          action={
            <button
              className="button secondary"
              onClick={() => setResultTab("diagnostics")}
            >
              查看并定位问题
            </button>
          }
        />
      ) : (
        <Empty
          title="结果会显示在这里"
          description="填写参数后点击「生成文件」，查看正文和检查信息。"
        />
      )}
      <div className="export-hint" role="status">
        {exportReason || "当前结果已生成，可复制或导出文件"}
      </div>
      <div className="result-footer">
        <label className="filename-field">
          <span>文件名</span>
          <input
            aria-label="输出文件名"
            value={doc.outputName}
            onChange={(e) => editDoc({ outputName: e.target.value })}
          />
        </label>
        <button
          className="icon-button"
          aria-label="复制结果"
          disabled={!!exportReason}
          title={exportReason || "导出当前结果"}
          onClick={() =>
            navigator.clipboard
              .writeText(String(text))
              .then(() => notify("结果已复制"))
              .catch(fail)
          }
        >
          <Copy size={15} />
        </button>
        <button
          className="button primary"
          disabled={!!exportReason}
          title={exportReason || "导出当前结果"}
          onClick={() =>
            download(
              String(
                latest?.result?.artifacts?.length &&
                  latest.result.artifacts.length > 1
                  ? latest.result.artifacts[0].text
                  : text,
              ),
              doc.outputName || "output.txt",
            )
          }
        >
          <Download size={14} />
          导出文件
        </button>
        {(latest?.result?.artifacts?.length || 0) > 1 &&
          latest!.result!.artifacts!.map((artifact, i) => (
            <button
              key={i}
              className="button secondary"
              disabled={!!exportReason}
              onClick={() => download(artifact.text, artifact.name)}
            >
              导出 {artifact.name}
            </button>
          ))}
      </div>
    </section>
  );
}
