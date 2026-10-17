import { ArrowLeft, ChevronLeft, ChevronRight, FileCode2, RefreshCw, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { DiffView } from "./DiffView";
import { Empty, Modal, PageTitle, RunDetail } from "./WorkbenchUi";
import { api } from "../lib/api";
import { historyExecutionReason } from "../lib/documentExecution";
import { safeNumber, type Data, type Run, type ReplayCheck } from "../lib/data";

export interface HistoryComparison {
  before: Run;
  after: Run;
}
interface Props {
  actions: Data[];
  runList: Data[];
  selectedHistory: Run | null;
  comparison: HistoryComparison | null;
  setRunList: (runs: Data[]) => void;
  setSelectedHistory: (run: Run | null) => void;
  setComparison: (comparison: HistoryComparison | null) => void;
  restoreRun: (run: Run) => Promise<void>;
  onCreated: (id: string, original: Run) => void;
  notify: (message: string) => void;
  fail: (error: Data) => void;
}
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
export function HistoryPage({
  actions,
  runList,
  selectedHistory,
  comparison,
  setRunList,
  setSelectedHistory,
  setComparison,
  restoreRun,
  onCreated,
  notify,
  fail,
}: Props) {
  const [replayCheck, setReplayCheck] = useState<{
    id: string;
    check: ReplayCheck;
  } | null>(null);
  const [checkError, setCheckError] = useState<{
    id: string;
    message: string;
  } | null>(null);
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState("");
  const [offset, setOffset] = useState(0);
  const [page, setPage] = useState<{ runs: Run[]; total: number }>({ runs: [], total: 0 });
  const [pageLoading, setPageLoading] = useState(false);
  const [deletePending, setDeletePending] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Run | null>(null);
  const [deleteError, setDeleteError] = useState("");
  const deleteBusy = useRef(false);
  const listRevision = useRef(0);
  const detailRevision = useRef(0);
  const compareListRevision = useRef(0);
  const [compareRuns, setCompareRuns] = useState<Run[]>([]);
  const [compareOffset, setCompareOffset] = useState(0);
  const [compareTotal, setCompareTotal] = useState(0);
  const [compareQuery, setCompareQuery] = useState("");
  const [compareSelectedId, setCompareSelectedId] = useState("");
  // Monotonic request id for comparison loads. Only the newest selection may
  // publish a comparison; switching the selected run or unmounting bumps it so
  // an older in-flight reply can never re-populate the diff.
  const compareRevision = useRef(0);
  const selectedId = selectedHistory?.id;
  useEffect(() => {
    if (selectedHistory) {
      listRevision.current += 1;
      return;
    }
    const revision = ++listRevision.current;
    setPageLoading(true);
    api("runs/query", { query: query || undefined, status: status || undefined, offset, limit: 25 })
      .then((result) => {
        if (revision !== listRevision.current) return;
        const total = safeNumber(result.total);
        if (offset > 0 && offset >= total) {
          setOffset(Math.max(0, Math.floor(Math.max(0, total - 1) / 25) * 25));
          return;
        }
        setPage({ runs: result.runs || [], total });
        setRunList(result.runs || []);
      })
      .catch((error) => {
        if (revision === listRevision.current) fail(error);
      })
      .finally(() => {
        if (revision === listRevision.current) setPageLoading(false);
      });
    return () => {
      if (revision === listRevision.current) listRevision.current += 1;
    };
  }, [query, status, offset, selectedId]);
  useEffect(() => {
    if (!selectedHistory?.id || selectedHistory.status !== "succeeded") {
      setCompareRuns([]);
      return;
    }
    const revision = ++compareListRevision.current;
    api("runs/query", { status: "succeeded", query: compareQuery || undefined, offset: compareOffset, limit: 25 })
      .then((result) => {
        if (revision === compareListRevision.current) {
          const total = safeNumber(result.total);
          setCompareRuns(result.runs || []);
          setCompareTotal(total);
          if (compareOffset > 0 && compareOffset >= total) setCompareOffset(Math.max(0, compareOffset - 25));
        }
      })
      .catch((error) => {
        if (revision === compareListRevision.current) fail(error);
      });
    return () => { compareListRevision.current += 1; };
  }, [selectedId, selectedHistory?.status, compareOffset, compareQuery]);
  useEffect(() => {
    return () => { detailRevision.current += 1; };
  }, []);
  useEffect(() => {
    setReplayCheck(null);
    setCheckError(null);
    let active = true;
    if (selectedId) {
      api("runs/" + selectedId + "/replay-check")
        .then((check) => {
          if (active) setReplayCheck({ id: selectedId, check });
        })
        .catch(() => {
          if (active)
            setCheckError({
              id: selectedId,
              message: "无法核对当前插件环境；重新执行时会记录实际使用的环境。",
            });
        });
    }
    return () => {
      active = false;
      compareRevision.current += 1;
    };
  }, [selectedId]);
  // Retired machining history stays read-only: the machining capability is gone,
  // so a record carrying machining intent must never rerun or restore, even if a
  // same-named provider were somehow present in the current capabilities.
  const executionReason = selectedHistory
    ? historyExecutionReason(selectedHistory, actions)
    : "";
  const refreshPage = () => {
    const revision = ++listRevision.current;
    setPageLoading(true);
    api("runs/query", { query: query || undefined, status: status || undefined, offset, limit: 25 })
      .then((result) => {
        if (revision !== listRevision.current) return;
        const total = safeNumber(result.total);
        if (offset > 0 && offset >= total) {
          setOffset(Math.max(0, Math.floor(Math.max(0, total - 1) / 25) * 25));
          return;
        }
        setPage({ runs: result.runs || [], total });
        setRunList(result.runs || []);
      })
      .catch((error) => { if (revision === listRevision.current) fail(error); })
      .finally(() => { if (revision === listRevision.current) setPageLoading(false); });
  };
  const deleteRun = async (run: Run) => {
    if (run.status === "queued" || run.status === "running" || deleteBusy.current) return;
    deleteBusy.current = true;
    setDeletePending(true);
    try {
      setDeleteError("");
      await api("runs/" + encodeURIComponent(run.id) + "/remove", {});
      listRevision.current += 1;
      const remaining = page.runs.filter((item) => item.id !== run.id);
      const newTotal = Math.max(0, page.total - 1);
      const nextOffset = offset > 0 && offset >= newTotal
        ? Math.max(0, Math.floor(Math.max(0, newTotal - 1) / 25) * 25)
        : offset;
      setPage({ runs: remaining, total: newTotal });
      setRunList(runList.filter((item) => item.id !== run.id));
      setOffset(nextOffset);
      if (nextOffset === offset) refreshPage();
      if (compareRuns.some((item) => item.id === run.id)) {
        compareListRevision.current += 1;
        setCompareRuns((items) => items.filter((item) => item.id !== run.id));
        setCompareTotal((total) => Math.max(0, total - 1));
      }
      if (selectedHistory?.id === run.id) setSelectedHistory(null);
      if (compareSelectedId === run.id) setCompareSelectedId("");
      if (comparison?.before.id === run.id || comparison?.after.id === run.id) setComparison(null);
      setDeleteTarget(null);
      notify("已删除历史记录");
    } catch (error) {
      const message = error instanceof Error ? error.message : "删除失败，请重试。";
      setDeleteError(message);
      fail(error);
    } finally {
      deleteBusy.current = false;
      setDeletePending(false);
    }
  };
  return (
    <div className="page-content">
      <PageTitle
        title="执行记录"
        description="查看生成结果，恢复当时的输入。成功记录中的模板快照不随后续编辑改变。"
        action={
          <button
            className="button secondary"
            onClick={refreshPage}
          >
            <RefreshCw size={14} />
            刷新
          </button>
        }
      />
      {selectedHistory ? (
        <>
          <button
            className="text-button"
            onClick={() => {
              detailRevision.current += 1;
              setSelectedHistory(null);
              setComparison(null);
            }}
          >
            <ArrowLeft size={14} />
            返回记录列表
          </button>
          <RunDetail
            run={selectedHistory}
            rerunReason={executionReason || undefined}
            restoreReason={executionReason || undefined}
            replayCheck={
              replayCheck && replayCheck.id === selectedId
                ? replayCheck.check
                : undefined
            }
            restore={() => void restoreRun(selectedHistory)}
            rerun={async () => {
              try {
                const created = await api(
                  "runs/" + selectedHistory.id + "/rerun",
                  {},
                );
                const refreshed = await api("runs/query", {
                  query: query || undefined,
                  status: status || undefined,
                  offset,
                  limit: 25,
                });
                setPage({ runs: refreshed.runs || [], total: refreshed.total || 0 });
                setRunList(refreshed.runs || []);
                onCreated(created.id, selectedHistory);
                notify("已使用历史输入创建新的任务");
              } catch (error) {
                fail(error);
              }
            }}
          />
          {checkError && checkError.id === selectedId && (
            <p className="hint">{checkError.message}</p>
          )}
          {selectedHistory.status === "succeeded" && (
            <div className="history-compare">
              <label>
                与另一条成功记录比较
                <input aria-label="搜索比较基准记录" value={compareQuery} onChange={(event) => { setCompareOffset(0); setCompareQuery(event.target.value); }} />
                <select
                  aria-label="比较历史记录"
                  value={compareSelectedId}
                  onChange={async (event) => {
                    const id = event.target.value;
                    if (!id) {
                      compareRevision.current += 1;
                      setCompareSelectedId("");
                      setComparison(null);
                      return;
                    }
                    setCompareSelectedId(id);
                    const revision = ++compareRevision.current;
                    const after = selectedHistory;
                    try {
                      const before = await api("runs/" + id);
                      if (revision !== compareRevision.current) return;
                      setComparison({ before, after });
                    } catch (error) {
                      if (revision === compareRevision.current) fail(error);
                    }
                  }}
                >
                  <option value="">选择基准记录</option>
                  {[...new Map([...runList, ...page.runs, ...compareRuns].map((record) => [record.id, record])).values()]
                    .filter(
                      (record) =>
                        record.status === "succeeded" &&
                        (record.id !== selectedHistory.id || record.id === compareSelectedId),
                    )
                    .map((record) => (
                      <option key={record.id} value={record.id}>
                        {record.title} · {dateText(record.started_at)}
                      </option>
                    ))}
                </select>
                <div className="history-compare-pagination">
                  <button className="button secondary" disabled={compareOffset === 0} onClick={() => setCompareOffset(Math.max(0, compareOffset - 25))}>上一页</button>
                  <span>{compareTotal ? `${compareOffset + 1}-${Math.min(compareOffset + 25, compareTotal)} / ${compareTotal}` : "0 / 0"}</span>
                  <button className="button secondary" disabled={compareOffset + 25 >= compareTotal} onClick={() => setCompareOffset(compareOffset + 25)}>下一页</button>
                </div>
              </label>
            </div>
          )}
          {comparison && comparison.after?.id === selectedId && (
            <DiffView
              before={
                comparison.before.result?.data.text ||
                comparison.before.result?.artifacts?.[0]?.text ||
                ""
              }
              after={
                comparison.after.result?.data.text ||
                comparison.after.result?.artifacts?.[0]?.text ||
                ""
              }
              beforeTitle={comparison.before.title}
              afterTitle={comparison.after.title}
            />
          )}
        </>
      ) : (
        <>
        <div className="toolbar history-controls">
          <label className="field">
            <span>搜索 title / ID / action</span>
            <input aria-label="搜索历史记录" value={query} onChange={(event) => { setOffset(0); setQuery(event.target.value); }} />
          </label>
          <label className="field">
            <span>状态</span>
            <select aria-label="历史记录状态" value={status} onChange={(event) => { setOffset(0); setStatus(event.target.value); }}>
              <option value="">全部状态</option>
              {Object.entries(stateNames).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
            </select>
          </label>
        </div>
        {page.runs.length ? <div className="history-table">
          <div className="history-heading">
            <span>任务</span><span>开始时间</span><span>状态</span><span />
          </div>
          {page.runs.map((run) => (
            <div className="history-row" key={run.id}>
              <button className="text-button" disabled={pageLoading || !!deleteTarget} onClick={() => {
                const revision = ++detailRevision.current;
                api("runs/" + encodeURIComponent(run.id)).then((detail) => {
                  if (revision === detailRevision.current) setSelectedHistory(detail);
                }).catch((error) => { if (revision === detailRevision.current) fail(error); });
              }}>
                <FileCode2 size={17} /><strong>{run.title}<small>{run.action === "template.render" ? "模板生成" : run.action}</small></strong>
              </button>
              <span>{dateText(run.started_at)}</span>
              <span className={"run-status " + run.status}>{stateNames[run.status] || run.status}</span>
              <button className="button secondary history-delete" aria-label={`删除记录 ${run.title}`} disabled={deletePending || run.status === "queued" || run.status === "running"} onClick={() => { detailRevision.current += 1; setDeleteTarget(run); }}><Trash2 size={15} /></button>
            </div>
          ))}
        </div> : pageLoading ? <p className="hint">正在加载记录…</p> : <Empty title={query || status ? "没有匹配的执行记录" : "还没有执行记录"} description={query || status ? "调整搜索词或状态后重试。" : "生成文件后，结果与输入会自动保存在这里。"} />}
        <div className="history-pagination">
          <button className="button secondary" disabled={offset === 0 || pageLoading} onClick={() => setOffset(Math.max(0, offset - 25))}><ChevronLeft size={14} />上一页</button>
          <span>{page.total ? `${offset + 1}-${Math.min(offset + page.runs.length, page.total)} / ${page.total}` : "0 / 0"}</span>
          <button className="button secondary" disabled={offset + 25 >= page.total || pageLoading} onClick={() => setOffset(offset + 25)}>下一页<ChevronRight size={14} /></button>
        </div>
        </>
      )}
      {deleteTarget && (
        <Modal
          title="确认删除执行记录"
          onClose={() => { if (!deleteBusy.current) setDeleteTarget(null); }}
          buttons={
            <>
              <button className="button secondary" disabled={deletePending} onClick={() => setDeleteTarget(null)}>取消</button>
              <button className="button danger" disabled={deletePending} onClick={() => void deleteRun(deleteTarget)}>{deletePending ? "正在删除…" : "删除记录"}</button>
            </>
          }
        >
          <p>删除“{deleteTarget.title}”后，历史输入与产物不可恢复。</p>
          {deleteError && <p className="field-message" role="alert">{deleteError}</p>}
        </Modal>
      )}
    </div>
  );
}
