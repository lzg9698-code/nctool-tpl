import { App, Alert, Button, Empty, Space, Tabs, Tag, Typography } from "antd";
import {
  CopyOutlined,
  DownloadOutlined,
  StopOutlined,
} from "@ant-design/icons";
import { useQuery, useMutation } from "@tanstack/react-query";
import { api, queryClient, running, runQuery } from "../lib/api";
import { checkResultDependencies } from "../lib/dependencies";
import { useWorkspace } from "../app/context";
import { deliveryReason } from "../lib/execution";
import { encode, errorText, saveFile, type Data } from "../lib/json";
import type { Draft } from "../state/session";
import type { Diagnostic } from "../lib/contracts";
import { diffLines } from "diff";

export const statusNames: Record<string, string> = {
  queued: "等待执行",
  running: "执行中",
  succeeded: "已完成",
  failed: "生成失败",
  cancelled: "已取消",
  interrupted: "已中断",
};
export function DiffDisplay({
  before,
  after,
}: {
  before: string;
  after: string;
}) {
  return (
    <pre className="diff-output">
      {diffLines(before, after).map((part, index) => (
        <span
          key={index}
          className={
            part.added
              ? "diff-added"
              : part.removed
                ? "diff-removed"
                : undefined
          }
        >
          {part.value}
        </span>
      ))}
    </pre>
  );
}
export function ResultPanel({
  runId,
  draft,
  historical = false,
  onDiagnostic,
  forceReason,
}: {
  runId?: string;
  draft?: Draft;
  historical?: boolean;
  onDiagnostic?: (diagnostic: Diagnostic) => void;
  forceReason?: string;
}) {
  const { id } = useWorkspace(),
    { message } = App.useApp();
  const run = useQuery(runQuery(id, runId));
  const replay = run.data?.result?.data?.replay_input;
  const needsCheck =
    !historical && !!(replay?.parameter_snapshot || replay?.snapshot);
  const dependencies = useQuery({
    queryKey: [id, "dependencies", runId],
    enabled: needsCheck && run.data?.status === "succeeded",
    queryFn: ({ signal }) => checkResultDependencies(replay, signal),
    refetchInterval: 3000,
    refetchIntervalInBackground: true,
  });
  const cancel = useMutation({
    mutationFn: () => api(`runs/${encodeURIComponent(runId!)}/cancel`, {}),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: [id, "run", runId] }),
    onError: (error) => message.error(errorText(error)),
  });
  const reason =
    forceReason ||
    deliveryReason(
      historical ? undefined : draft,
      run.data,
      historical ||
        !needsCheck ||
        (dependencies.data === true && !dependencies.isError),
    );
  if (!runId) return <Empty description="填写参数并生成，结果会显示在这里" />;
  if (run.isError)
    return (
      <Alert
        type="error"
        title="任务读取失败"
        description={errorText(run.error)}
        action={<Button onClick={() => run.refetch()}>重试</Button>}
      />
    );
  const details = run.data;
  const diagnostics = [
    ...(details?.result?.diagnostics || []),
    ...(details?.error?.diagnostics || []),
  ];
  return (
    <div className="result-panel">
      <div className="section-toolbar">
        <Space wrap size={[12, 8]}>
          <Tag
            color={
              details?.status === "succeeded"
                ? "success"
                : details?.status === "failed"
                  ? "error"
                  : "processing"
            }
          >
            {statusNames[details?.status || ""] || "正在读取"}
          </Tag>
          <Typography.Text type="secondary">{runId}</Typography.Text>
        </Space>
        {running(details?.status) && (
          <Button
            icon={<StopOutlined aria-hidden />}
            loading={cancel.isPending}
            onClick={() => cancel.mutate()}
          >
            取消任务
          </Button>
        )}
      </div>
      {details?.error && <Alert type="error" title={details.error.message} />}
      {reason && !running(details?.status) && (
        <Alert type="info" title={reason} />
      )}
      <Tabs
        defaultActiveKey="result"
        items={[
          {
            key: "result",
            label: "生成结果",
            children: details?.result?.artifacts?.length ? (
              <Space vertical style={{ width: "100%" }}>
                {details.result.artifacts.map(
                  (artifact: Data, index: number) => (
                    <section className="artifact" key={index}>
                      <div className="section-toolbar">
                        <Typography.Text strong>
                          {artifact.name}
                        </Typography.Text>
                        <Space wrap size={[12, 8]}>
                          <Button
                            aria-label={"复制 " + artifact.name}
                            icon={<CopyOutlined aria-hidden />}
                            disabled={!!reason}
                            onClick={() =>
                              navigator.clipboard
                                .writeText(artifact.text)
                                .then(() => message.success("已复制"))
                                .catch((error) =>
                                  message.error(errorText(error)),
                                )
                            }
                          >
                            复制结果
                          </Button>
                          <Button
                            type="primary"
                            icon={<DownloadOutlined aria-hidden />}
                            disabled={!!reason}
                            onClick={() =>
                              saveFile(
                                !historical && index === 0
                                  ? draft?.outputName || artifact.name
                                  : artifact.name,
                                artifact.text,
                                artifact.media_type,
                              )
                            }
                          >
                            导出文件
                          </Button>
                        </Space>
                      </div>
                      <pre className="artifact-text">{artifact.text}</pre>
                    </section>
                  ),
                )}
              </Space>
            ) : (
              <Empty
                description={
                  running(details?.status) ? "任务正在执行" : "没有生成文件"
                }
              />
            ),
          },
          {
            key: "diagnostics",
            label: `检查信息 (${diagnostics.length})`,
            children: diagnostics.length ? (
              <Space vertical style={{ width: "100%" }}>
                {diagnostics.map((entry, index: number) => (
                  <Alert
                    key={index}
                    type={entry.level === "error" ? "error" : "warning"}
                    title={entry.message}
                    description={entry.template
                      ? `${entry.template}${entry.line ? `:${entry.line}${entry.column ? `:${entry.column}` : ""}` : ""}`
                      : entry.path}
                    action={
                      onDiagnostic && (entry.path !== undefined || entry.line !== undefined) && (
                        <Button onClick={() => onDiagnostic(entry)}>
                          定位问题
                        </Button>
                      )
                    }
                  />
                ))}
              </Space>
            ) : (
              <Empty description="没有检查信息" />
            ),
          },
          {
            key: "input",
            label: "实际输入",
            children: (
              <pre className="json-output">
                {details &&
                  encode(
                    details.result?.data?.replay_input || details.input,
                    true,
                  )}
              </pre>
            ),
          },
        ]}
      />
    </div>
  );
}
