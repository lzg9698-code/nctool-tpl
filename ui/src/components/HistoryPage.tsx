import { ArrowLeft, ChevronRight, FileCode2, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { DiffView } from "./DiffView";
import { Empty, PageTitle, RunDetail } from "./WorkbenchUi";
import { api } from "../lib/api";
import { safeNumber, type Data, type Run, type ReplayCheck } from "../lib/data";

export interface HistoryComparison {
  before: Run;
  after: Run;
}
interface Props {
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
  const selectedId = selectedHistory?.id;
  useEffect(() => {
    setReplayCheck(null);
    setCheckError(null);
    if (!selectedId) return;
    let active = true;
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
    return () => {
      active = false;
    };
  }, [selectedId]);
  return (
    <div className="page-content">
      <PageTitle
        title="执行记录"
        description="查看生成结果，恢复当时的输入。成功记录中的模板快照不随后续编辑改变。"
        action={
          <button
            className="button secondary"
            onClick={() =>
              api("runs")
                .then((r) => setRunList(r.runs))
                .catch(fail)
            }
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
              setSelectedHistory(null);
              setComparison(null);
            }}
          >
            <ArrowLeft size={14} />
            返回记录列表
          </button>
          <RunDetail
            run={selectedHistory}
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
                setRunList((await api("runs")).runs);
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
                <select
                  aria-label="比较历史记录"
                  defaultValue=""
                  onChange={async (event) => {
                    const id = event.target.value;
                    if (id) {
                      try {
                        const before = await api("runs/" + id);
                        setComparison({ before, after: selectedHistory });
                      } catch (error) {
                        fail(error);
                      }
                    }
                  }}
                >
                  <option value="">选择基准记录</option>
                  {runList
                    .filter(
                      (record) =>
                        record.status === "succeeded" &&
                        record.id !== selectedHistory.id,
                    )
                    .map((record) => (
                      <option key={record.id} value={record.id}>
                        {record.title} · {dateText(record.started_at)}
                      </option>
                    ))}
                </select>
              </label>
            </div>
          )}
          {comparison && (
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
      ) : runList.length ? (
        <div className="history-table">
          <div className="history-heading">
            <span>任务</span>
            <span>开始时间</span>
            <span>状态</span>
            <span />
          </div>
          {runList.map((run) => (
            <button
              className="history-row"
              key={run.id}
              onClick={() =>
                api("runs/" + run.id)
                  .then(setSelectedHistory)
                  .catch(fail)
              }
            >
              <span>
                <FileCode2 size={17} />
                <strong>
                  {run.title}
                  <small>
                    {run.action === "template.render" ? "模板生成" : run.action}
                  </small>
                </strong>
              </span>
              <span>{dateText(run.started_at)}</span>
              <span className={"run-status " + run.status}>
                {stateNames[run.status]}
              </span>
              <ChevronRight size={16} />
            </button>
          ))}
        </div>
      ) : (
        <Empty
          title="还没有执行记录"
          description="生成文件后，结果与输入会自动保存在这里。"
        />
      )}
    </div>
  );
}
