import {
  Children,
  isValidElement,
  useState,
  useEffect,
  useRef,
  type ReactNode,
} from "react";
import * as Dialog from "@radix-ui/react-dialog";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import {
  FileCode2,
  X,
  MoreHorizontal,
  Download,
  AlertTriangle,
} from "lucide-react";
import { Editor } from "./LazyEditor";
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
  const ref = useRef<HTMLDivElement>(null);
  // Callers mount from state, without a Dialog.Trigger. Capture before portal
  // autofocus and before a menu action's item is removed from the document.
  const [trigger] = useState(() => {
    const active = document.activeElement as HTMLElement | null;
    const menuTriggerId = active
      ?.closest('[role="menu"]')
      ?.getAttribute("aria-labelledby");
    return (menuTriggerId && document.getElementById(menuTriggerId)) || active;
  });
  return (
    <Dialog.Root open onOpenChange={(open) => !open && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="modal-overlay" />
        <Dialog.Content
          ref={ref}
          className="modal"
          aria-describedby={undefined}
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            (
              ref.current?.querySelector<HTMLElement>(
                "input:not(:disabled), select:not(:disabled), textarea:not(:disabled), .modal-footer button:not(:disabled)",
              ) ||
              ref.current?.querySelector<HTMLElement>("button:not(:disabled)")
            )?.focus();
          }}
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            if (trigger?.isConnected) trigger.focus();
          }}
        >
          <div className="modal-header">
            <Dialog.Title asChild>
              <h2>{title}</h2>
            </Dialog.Title>
            <Dialog.Close asChild>
              <button
                type="button"
                className="icon-button"
                aria-label="关闭对话框"
              >
                <X size={18} />
              </button>
            </Dialog.Close>
          </div>
          <div className="modal-body">{children}</div>
          {buttons && <div className="modal-footer">{buttons}</div>}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
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
  return (
    <div className={"menu" + (text ? " menu-labeled" : "")}>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger asChild>
          <button
            type="button"
            className={"menu-trigger" + (text ? " nav-item" : "")}
            aria-label={label}
            title={label}
          >
            {icon || <MoreHorizontal size={18} />}
            {text && <span className="nav-label">{text}</span>}
          </button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content
            className="menu-items"
            align="end"
            sideOffset={6}
          >
            {Children.map(children, (child) =>
              isValidElement<{ disabled?: boolean }>(child) &&
              child.type === "button" ? (
                <DropdownMenu.Item asChild disabled={child.props.disabled}>
                  {child}
                </DropdownMenu.Item>
              ) : (
                child
              ),
            )}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </div>
  );
}
interface JsonEditorProps {
  value: Data;
  onChange: (value: Data) => void;
  label: string;
  /** Stable document identity, independent of its value and display label. */
  editorKey?: string;
  onError?: (error: string) => void;
  draft?: string;
  onRawChange?: (text: string) => void;
}
export function JsonEditor(props: JsonEditorProps) {
  // A new document starts with its own raw draft, parse status and undo state.
  // Keep this boundary above local state so an invalid draft cannot leak either.
  return <JsonEditorDraft key={props.editorKey} {...props} />;
}
function JsonEditorDraft({
  value,
  onChange,
  label,
  onError,
  draft,
  onRawChange,
}: JsonEditorProps) {
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
        setError("");
        onError?.("");
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
  rerunReason,
  restoreReason,
}: {
  run: Run;
  restore?: () => void;
  rerun?: () => void;
  stale?: boolean;
  replayCheck?: ReplayCheck;
  rerunReason?: string;
  restoreReason?: string;
}) {
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
          <button
            className="button secondary"
            onClick={rerun}
            disabled={!!rerunReason}
            title={rerunReason}
          >
            按历史输入重新执行
          </button>
        )}
        {restore && (
          <button
            className="button secondary"
            onClick={restore}
            disabled={!!restoreReason}
            title={restoreReason}
          >
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
      {run.status === "succeeded" &&
        (run.result?.artifacts?.length || 0) > 1 && (
          <div className="toolbar">
            {run.result!.artifacts!.map((artifact, i) => (
              <button
                key={i}
                className="button secondary"
                disabled={stale}
                onClick={() => download(artifact.text, artifact.name)}
              >
                导出 {artifact.name}
              </button>
            ))}
          </div>
        )}
      {(rerunReason || restoreReason) && (
        <p className="stale-banner" role="status">
          {rerunReason || restoreReason}
        </p>
      )}
      {run.status === "succeeded" && run.result?.data.parameters && (
        <details className="advanced">
          <summary>参数值与来源</summary>
          <pre>{encode(run.result.data.parameters, true)}</pre>
        </details>
      )}
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
