import { useState, useEffect, useRef, type ReactNode } from "react";
import {
  FileCode2,
  X,
  MoreHorizontal,
  Download,
  AlertTriangle,
} from "lucide-react";
import { Editor } from "./Editor";
import { useManagedDependencies } from "../lib/managedDependencies";
import {
  encode,
  parseData,
  download,
  diagnosticsFrom,
  safeNumber,
  type Data,
  type Run,
  type ReplayCheck,
} from "../lib/data";
const stateNames: Record<string, string> = {
  queued: "等待执行",
  running: "执行中",
  succeeded: "已完成",
  failed: "未完成",
  cancelled: "已取消",
  interrupted: "已中断",
};
function dateText(value: Data) {
  return new Date(safeNumber(value)).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function Empty({
  title,
  description,
  icon,
  action: button,
}: {
  title: string;
  description: string;
  icon?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="empty-state">
      {icon || <FileCode2 size={30} />}
      <strong>{title}</strong>
      <p>{description}</p>
      {button}
    </div>
  );
}
export function PageTitle({
  title,
  description,
  action,
}: {
  title: string;
  description: string;
  action?: ReactNode;
}) {
  return (
    <header className="page-heading">
      <div>
        <p className="eyebrow">WORKSPACE</p>
        <h1>{title}</h1>
        <p>{description}</p>
      </div>
      {action}
    </header>
  );
}
export function Modal({
  title,
  children,
  buttons,
  onClose,
}: {
  title: string;
  children: ReactNode;
  buttons?: ReactNode;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current!;
    const trigger = document.activeElement as HTMLElement | null;
    dialog.showModal();
    (
      dialog.querySelector<HTMLElement>(
        "input, select, textarea, .modal-footer button",
      ) || dialog.querySelector<HTMLElement>("button")
    )?.focus();
    return () => {
      dialog.close();
      if (trigger?.isConnected) trigger.focus();
    };
  }, []);
  return (
    <dialog
      ref={ref}
      className="modal"
      aria-label={title}
      onKeyDown={(event) => {
        if (event.key !== "Tab") return;
        const controls = [
          ...event.currentTarget.querySelectorAll<HTMLElement>(
            'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], [tabindex]:not([tabindex="-1"])',
          ),
        ].filter((element) => element.getClientRects().length > 0);
        const first = controls[0],
          last = controls[controls.length - 1];
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last?.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first?.focus();
        }
      }}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onClick={(event) => {
        if (event.target !== event.currentTarget) return;
        const bounds = event.currentTarget.getBoundingClientRect();
        if (
          event.clientX < bounds.left ||
          event.clientX > bounds.right ||
          event.clientY < bounds.top ||
          event.clientY > bounds.bottom
        )
          onClose();
      }}
    >
      <div className="modal-header">
        <h2>{title}</h2>
        <button
          className="icon-button"
          aria-label="关闭对话框"
          onClick={onClose}
        >
          <X size={18} />
        </button>
      </div>
      <div className="modal-body">{children}</div>
      {buttons && <div className="modal-footer">{buttons}</div>}
    </dialog>
  );
}
export function Menu({
  children,
  label = "更多模板操作",
  icon,
  text,
}: {
  children: ReactNode;
  label?: string;
  icon?: ReactNode;
  text?: string;
}) {
  const ref = useRef<HTMLDetailsElement>(null);
  const [open, setOpen] = useState(false);
  useEffect(() => {
    const close = (event: PointerEvent) => {
      if (ref.current?.open && !ref.current.contains(event.target as Node))
        ref.current.open = false;
    };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, []);
  return (
    <details
      ref={ref}
      className={"menu" + (text ? " menu-labeled" : "")}
      onToggle={(event) => setOpen(event.currentTarget.open)}
      onKeyDown={(event) => {
        if (event.key === "Escape" && event.currentTarget.open) {
          event.preventDefault();
          event.currentTarget.open = false;
          event.currentTarget.querySelector("summary")?.focus();
        }
      }}
      onClick={(event) => {
        if ((event.target as HTMLElement).closest("button")) {
          event.currentTarget.open = false;
          event.currentTarget.querySelector("summary")?.focus();
        }
      }}
    >
      <summary aria-label={label} title={label} aria-expanded={open}>
        {icon || <MoreHorizontal size={18} />}
        {text && <span className="nav-label">{text}</span>}
      </summary>
      <div className="menu-items">{children}</div>
    </details>
  );
}
export function JsonEditor({
  value,
  onChange,
  label,
  onError,
  draft,
  onRawChange,
}: {
  value: Data;
  onChange: (value: Data) => void;
  label: string;
  onError?: (error: string) => void;
  draft?: string;
  onRawChange?: (text: string) => void;
}) {
  const [text, setText] = useState(() => {
    if (draft !== undefined) return draft;
    try {
      return encode(value, true);
    } catch {
      return "{}";
    }
  });
  const [error, setError] = useState("");
  const parsed = useRef("");
  useEffect(() => {
    if (draft !== undefined) {
      setText(draft);
      try {
        parseData(draft);
        setError("");
        onError?.("");
      } catch (e) {
        setError((e as Error).message);
        onError?.((e as Error).message);
      }
      return;
    }
    try {
      const next = encode(value, true);
      if (next !== parsed.current) {
        setText(next);
        parsed.current = next;
      }
    } catch {}
  }, [value, draft]);
  return (
    <div className="json-editor">
      <Editor
        value={text}
        language="json"
        label={label}
        onChange={(raw) => {
          setText(raw);
          onRawChange?.(raw);
          try {
            const next = parseData(raw);
            parsed.current = encode(next, true);
            setError("");
            onError?.("");
            onChange(next);
          } catch (e) {
            const message = (e as Error).message;
            setError(message);
            onError?.(message);
          }
        }}
      />
      {error && (
        <div className="inline-error" role="alert">
          {error}
        </div>
      )}
    </div>
  );
}
export function RunDetail({
  run,
  restore,
  rerun,
  stale = false,
  replayCheck,
}: {
  run: Run;
  restore?: () => void;
  rerun?: () => void;
  stale?: boolean;
  replayCheck?: ReplayCheck;
}) {
  const dependency = useManagedDependencies(run, !restore && !rerun);
  stale = stale || dependency.stale;
  const text =
    run.status === "succeeded"
      ? run.result?.data.text || run.result?.artifacts?.[0]?.text || ""
      : "";
  return (
    <div className="run-detail">
      {replayCheck && (
        <div
          className={
            replayCheck.status === "matching" ? "hint" : "stale-banner"
          }
          data-replay-status={replayCheck.status}
        >
          {replayCheck.status === "matching"
            ? "已记录的插件版本和服务提供方与当前一致。重执行仍使用当前程序和配置。"
            : replayCheck.status === "changed"
              ? "当前插件环境与历史记录不同，重新执行的结果可能变化。原历史文件仍可导出。"
              : "这条历史记录的执行环境信息不完整，重新执行会使用当前插件。"}
          {replayCheck.differences.length > 0 && (
            <ul>
              {replayCheck.differences.map((difference, index) => (
                <li key={index}>{difference}</li>
              ))}
            </ul>
          )}
        </div>
      )}
      {run.replay && (
        <p className="hint" data-replay-origin={run.replay.source_run_id}>
          此任务从历史记录 {run.replay.source_run_id} 重新执行。
          {run.replay.check.status === "changed"
            ? "执行时插件环境已变化。"
            : run.replay.check.status === "unknown"
              ? "原记录的环境信息不完整。"
              : "执行时已记录的环境一致。"}
        </p>
      )}
      {stale && (
        <div className="stale-banner">
          结果对应旧输入，重新执行后才能导出当前文件。
        </div>
      )}
      <div className="run-detail-toolbar">
        <div>
          <h2>{run.title}</h2>
          <span className={"run-status " + run.status}>
            {stateNames[run.status]}
          </span>
          <span>{dateText(run.started_at)}</span>
        </div>
        {rerun && (
          <button className="button secondary" onClick={rerun}>
            按历史输入重新执行
          </button>
        )}
        {restore && (
          <button className="button secondary" onClick={restore}>
            恢复输入
          </button>
        )}
        {text && (
          <button
            className="button primary"
            disabled={stale}
            onClick={() =>
              download(
                String(run.result?.artifacts?.[0]?.text || text),
                run.result?.artifacts?.[0]?.name || "output.txt",
              )
            }
          >
            <Download size={14} />
            导出历史文件
          </button>
        )}
      </div>
      {run.status === "succeeded" && (run.result?.artifacts?.length || 0) > 1 && <div className="toolbar">{run.result!.artifacts!.map((artifact, i) => <button key={i} className="button secondary" disabled={stale} onClick={() => download(artifact.text, artifact.name)}>导出 {artifact.name}</button>)}</div>}
      {run.status === "succeeded" && run.result?.data.prepared && <details className="advanced"><summary>参数来源、工艺派生值与计算诊断</summary><pre>{encode({ values: run.result.data.parameters?.values, origins: run.result.data.parameters?.origins, units: Object.fromEntries(Object.entries(run.result.data.parameters?.definitions || {}).map(([path, definition]: [string, Data]) => [path, definition.unit || ""])), cutting: run.result.data.cutting?.slots, derived: run.result.data.prepared.contexts, algorithm_version: run.result.data.prepared.algorithm_version }, true)}</pre></details>}
      {text ? (
        <div className="history-code">
          <Editor
            value={String(text)}
            language="text"
            readonly
            label="历史生成结果"
          />
        </div>
      ) : run.error ? (
        <div className="inline-error">{run.error.message}</div>
      ) : (
        <p className="hint">任务尚未返回结果。</p>
      )}
      <div className="run-diagnostics">
        {(run.error
          ? diagnosticsFrom(run.error)
          : run.result?.diagnostics || []
        ).map((d, i) => (
          <div className={"diagnostic " + d.level} key={i}>
            <AlertTriangle size={14} />
            {d.message}
          </div>
        ))}
      </div>
      <details className="advanced">
        <summary>查看原始输入与版本信息</summary>
        <pre>
          {encode(
            {
              input: run.input,
              plugins: (run as Data).plugins?.map((p: Data) => ({
                id: p.id,
                version: p.version,
              })),
              environment: run.environment,
              replay: run.replay,
            },
            true,
          )}
        </pre>
      </details>
    </div>
  );
}
