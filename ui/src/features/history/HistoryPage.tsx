import { useState } from "react";
import {
  Alert,
  App,
  Button,
  Input,
  Pagination,
  Select,
  Space,
  Table,
  Tag,
  Typography,
} from "antd";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useNavigate, useParams } from "react-router-dom";
import { useWorkspace } from "../../app/context";
import { api, queryClient, runQuery, running } from "../../lib/api";
import { assertExecutable, restoreRun } from "../../lib/execution";
import { errorText, encode, type Data } from "../../lib/json";
import { readRunPage, readReplayCheck, readCreated, type RunSummary } from "../../lib/contracts";
import {
  ResultPanel,
  statusNames,
  DiffDisplay,
} from "../../components/ResultPanel";

export default function HistoryPage() {
  const { id, capabilities, store } = useWorkspace(),
    { runId } = useParams(),
    navigate = useNavigate(),
    { message, modal } = App.useApp();
  const [search, setSearch] = useState(""),
    [status, setStatus] = useState<string>(),
    [page, setPage] = useState(1);
  const [compareSearch, setCompareSearch] = useState(""),
    [comparePage, setComparePage] = useState(1),
    [baseline, setBaseline] = useState<string>();
  const query = useQuery({
    queryKey: [id, "history", search, status, page],
    queryFn: ({ signal }) =>
      api(
        "runs/query",
        {
          query: search || undefined,
          status,
          offset: (page - 1) * 25,
          limit: 25,
        },
        signal,
        readRunPage,
      ),
    refetchInterval: 3000,
  });
  const detail = useQuery(runQuery(id, runId));
  const check = useQuery({
    queryKey: [id, "replay-check", runId],
    enabled: !!runId,
    queryFn: ({ signal }) =>
      api(`runs/${encodeURIComponent(runId!)}/replay-check`, undefined, signal, readReplayCheck),
  });
  const comparisons = useQuery({
    queryKey: [id, "history", "comparison", compareSearch, comparePage],
    enabled: !!runId && detail.data?.status === "succeeded",
    queryFn: ({ signal }) =>
      api(
        "runs/query",
        {
          status: "succeeded",
          query: compareSearch || undefined,
          offset: (comparePage - 1) * 25,
          limit: 25,
        },
        signal,
        readRunPage,
      ),
  });
  const before = useQuery(runQuery(id, baseline));
  let reason = "";
  if (detail.data) {
    try {
      const input = detail.data.result?.data?.replay_input || detail.data.input;
      assertExecutable(input?.snapshot?.asset || {}, detail.data.action);
      if (
        !capabilities.actions.some(
          (entry: Data) => entry.id === detail.data.action,
        )
      )
        throw new Error("此动作当前没有可用提供方");
    } catch (error) {
      reason = errorText(error);
    }
  }
  const rerun = useMutation({
    mutationFn: () => api(`runs/${encodeURIComponent(runId!)}/rerun`, {}, undefined, readCreated),
    onSuccess: async (created) => {
      store.getState().track(created.id);
      await queryClient.invalidateQueries({ queryKey: [id, "history"] });
      navigate("/history/" + created.id);
    },
    onError: (error) => message.error(errorText(error)),
  });
  const remove = (run: Pick<RunSummary, "id" | "title">) =>
    modal.confirm({
      title: "删除执行记录？",
      content: "记录及其输入和产物将永久删除。",
      okText: "删除记录",
      cancelText: "保留",
      okButtonProps: { danger: true },
      onOk: async () => {
        try {
          await api(`runs/${encodeURIComponent(run.id)}/remove`, {});
          queryClient.removeQueries({ queryKey: [id, "run", run.id] });
          await queryClient.invalidateQueries({ queryKey: [id, "history"] });
          navigate("/history");
        } catch (error) {
          message.error(errorText(error));
          throw error;
        }
      },
    });
  if (runId)
    return (
      <div className="page">
        <div className="page-heading">
          <div>
            <Button onClick={() => navigate("/history")}>返回记录列表</Button>
            <Typography.Title level={2}>
              {detail.data?.title || "执行记录"}
            </Typography.Title>
          </div>
          <Space wrap size={[12, 8]}>
            <Button
              disabled={
                !!reason ||
                !detail.data ||
                !["template.render", "template.render.managed"].includes(
                  detail.data.action,
                )
              }
              onClick={() => {
                try {
                  const draft = restoreRun(detail.data, capabilities);
                  store.getState().add(draft);
                  navigate("/workbench/" + draft.key);
                } catch (error) {
                  message.error(errorText(error));
                }
              }}
            >
              恢复输入为新草稿
            </Button>
            <Button
              disabled={
                !!reason ||
                !detail.data ||
                running(detail.data.status) ||
                !check.data ||
                check.isError
              }
              loading={rerun.isPending}
              onClick={() =>
                modal.confirm({
                  title: "按历史输入重新执行？",
                  content:
                    check.data?.status === "matching"
                      ? "已记录的信息一致，将创建新记录；不保证生成结果相同。"
                      : "执行环境已变化或信息不完整，请核对下方环境差异。新记录将记载本次环境。",
                  okText: "创建新任务",
                  cancelText: "取消",
                  onOk: () => rerun.mutateAsync(),
                })
              }
            >
              重新执行
            </Button>
            <Button
              danger
              disabled={!detail.data || running(detail.data.status)}
              onClick={() => detail.data && remove(detail.data)}
            >
              删除记录
            </Button>
          </Space>
        </div>
        {reason && <Alert type="warning" title={reason} />}
        {check.isError && (
          <Alert
            type="error"
            title="执行环境检查失败"
            description={errorText(check.error)}
            action={<Button onClick={() => check.refetch()}>重试</Button>}
          />
        )}
        {check.data && (
          <Alert
            type={check.data.status === "matching" ? "success" : "warning"}
            title={
              check.data.status === "matching"
                ? "执行环境一致"
                : check.data.status === "changed"
                  ? "执行环境已变化"
                  : "执行环境信息不完整"
            }
            description={check.data.status === "matching"
              ? "已记录的版本、内容指纹与服务绑定一致。私密配置和包外依赖未比较。"
              : (check.data.differences || []).join("；")}
          />
        )}
        <ResultPanel runId={runId} historical />
        {detail.data?.status === "succeeded" && (
          <section className="comparison">
            <Typography.Title level={4}>比较成功记录</Typography.Title>
            <Space wrap size={[12, 8]}>
              <Input.Search
                aria-label="搜索比较基准记录"
                placeholder="搜索比较基准记录"
                value={compareSearch}
                onChange={(event) => {
                  setCompareSearch(event.target.value);
                  setComparePage(1);
                }}
              />
              <Select
                aria-label="比较历史记录"
                allowClear
                style={{ minWidth: 260 }}
                value={baseline}
                onChange={setBaseline}
                options={(comparisons.data?.runs || [])
                  .filter((run: Data) => run.id !== runId)
                  .map((run: Data) => ({
                    value: run.id,
                    label: `${run.title} · ${run.id}`,
                  }))}
              />
              <Pagination
                size="small"
                current={comparePage}
                pageSize={25}
                total={Number(comparisons.data?.total || 0)}
                onChange={setComparePage}
                showSizeChanger={false}
              />
            </Space>
            {before.data && (
              <DiffDisplay
                before={(before.data.result?.artifacts || [])
                  .map(
                    (artifact: Data) =>
                      `--- ${artifact.name}\n${artifact.text}`,
                  )
                  .join("\n")}
                after={(detail.data.result?.artifacts || [])
                  .map(
                    (artifact: Data) =>
                      `--- ${artifact.name}\n${artifact.text}`,
                  )
                  .join("\n")}
              />
            )}
          </section>
        )}
        {detail.data && (
          <details>
            <summary>插件、实际环境与来源</summary>
            <pre>
              {encode(
                {
                  environment: detail.data.environment,
                  replay: detail.data.replay,
                },
                true,
              )}
            </pre>
          </details>
        )}
      </div>
    );
  return (
    <div className="page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>执行记录</Typography.Title>
          <Typography.Text type="secondary">
            查看实际输入、诊断与产物，或基于历史创建新任务。
          </Typography.Text>
        </div>
      </div>
      <Space wrap size={[12, 8]}>
        <Input.Search
          aria-label="搜索执行记录"
          placeholder="标题、记录标识或动作"
          value={search}
          onChange={(event) => {
            setSearch(event.target.value);
            setPage(1);
          }}
        />
        <Select
          aria-label="筛选任务状态"
          allowClear
          placeholder="全部状态"
          style={{ width: 160 }}
          value={status}
          options={Object.entries(statusNames).map(([value, label]) => ({
            value,
            label,
          }))}
          onChange={(value) => {
            setStatus(value);
            setPage(1);
          }}
        />
      </Space>
      {query.isError && (
        <Alert
          type="error"
          title={errorText(query.error)}
          action={<Button onClick={() => query.refetch()}>重试</Button>}
        />
      )}
      <Table
        rowKey="id"
        loading={query.isFetching}
        dataSource={query.data?.runs || []}
        pagination={{
          current: page,
          pageSize: 25,
          total: Number(query.data?.total || 0),
          showSizeChanger: false,
          onChange: setPage,
        }}
        columns={[
          {
            title: "任务",
            render: (_, run: Data) => (
              <Button
                type="link"
                onClick={() => navigate("/history/" + run.id)}
              >
                {run.title || run.id}
              </Button>
            ),
          },
          { title: "动作", dataIndex: "action" },
          {
            title: "状态",
            render: (_, run: Data) => (
              <Tag color={run.status === "succeeded" ? "success" : "default"}>
                {statusNames[run.status] || run.status}
              </Tag>
            ),
          },
          {
            title: "操作",
            render: (_, run: Data) => (
              <Button
                type="text"
                danger
                disabled={running(run.status)}
                onClick={() => remove(run)}
              >
                删除记录
              </Button>
            ),
          },
        ]}
      />
    </div>
  );
}
