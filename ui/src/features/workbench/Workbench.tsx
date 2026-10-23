import { useEffect, useState } from "react";
import {
  App,
  Alert,
  Button,
  Card,
  Empty,
  Form,
  Grid,
  Input,
  Segmented,
  Select,
  Space,
  Slider,
  Splitter,
  Tabs,
  Tag,
  Typography,
} from "antd";
import {
  PlayCircleOutlined,
  SaveOutlined,
  CopyOutlined,
  DownloadOutlined,
  DeleteOutlined,
  SearchOutlined,
} from "@ant-design/icons";
import { useNavigate, useParams } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { useSession, useWorkspace } from "../../app/context";
import { assetOf, dirty, makeDraft, type Draft } from "../../state/session";
import { action, parameterQuery, runQuery, running } from "../../lib/api";
import {
  decode,
  encode,
  merge,
  overrideAt,
  pointer,
  saveFile,
  errorText,
  type Data,
} from "../../lib/json";
import {
  selectAction,
  resolveRequest,
  assertExecutable,
} from "../../lib/execution";
import { CodeEditor } from "../../components/LazyCodeEditor";
import { SchemaFields, SchemaDesigner } from "../../components/SchemaFields";
import { ResultPanel } from "../../components/ResultPanel";
import { useDocumentOperations } from "./useDocumentOperations";

function JsonDraft({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  let problem = "";
  try {
    decode(value);
  } catch (error) {
    problem = errorText(error);
  }
  return (
    <section className="editor-section">
      <Typography.Text strong>{label}</Typography.Text>
      {problem && (
        <Alert
          type="error"
          title="JSON 尚未完成，修正后才能保存或执行"
          description={problem}
        />
      )}
      <CodeEditor
        label={label}
        language="json"
        value={value}
        onChange={onChange}
      />
    </section>
  );
}
function Bindings({ draft }: { draft: Draft }) {
  const { id, store } = useWorkspace(),
    { message } = App.useApp();
  const definitions = useQuery(parameterQuery(id, "definitions")),
    presets = useQuery(parameterQuery(id, "presets"));
  const [path, setPath] = useState(""),
    [definition, setDefinition] = useState<string>();
  let asset: Data;
  try {
    asset = assetOf(draft);
  } catch (error) {
    return <Alert type="error" title={errorText(error)} />;
  }
  const parameters = asset.metadata.parameters || {},
    bindings = parameters.bindings || {};
  const update = (next: Data) =>
    store.getState().patch(draft.key, {
      metadata: encode(
        { ...asset.metadata, parameters: { ...parameters, ...next } },
        true,
      ),
      replay: undefined,
    });
  return (
    <Space vertical style={{ width: "100%" }}>
      <Typography.Paragraph type="secondary">
        绑定把模板 JSON
        路径关联到共享定义。绑定随模板保存；共享定义与预置在参数管理页维护。
      </Typography.Paragraph>
      {(definitions.isError || presets.isError) && (
        <Alert
          type="error"
          title="共享资产列表读取失败"
          action={
            <Button
              onClick={() => {
                definitions.refetch();
                presets.refetch();
              }}
            >
              重试
            </Button>
          }
        />
      )}
      <Form layout="vertical">
        <Form.Item label="模板默认共享预置">
          <Select
            aria-label="模板默认共享预置"
            allowClear
            value={parameters.preset_id}
            options={(presets.data?.data?.items || []).map((item: Data) => ({
              value: item.id,
              label: item.title || item.id,
            }))}
            onChange={(preset) => {
              const next = { ...parameters, preset_id: preset };
              if (!preset) delete next.preset_id;
              store.getState().patch(draft.key, {
                metadata: encode({ ...asset.metadata, parameters: next }, true),
                replay: undefined,
              });
            }}
          />
        </Form.Item>
      </Form>
      {Object.entries(bindings).map(([fieldPath, binding]) => (
        <Card
          size="small"
          key={fieldPath}
          title={fieldPath}
          extra={
            <Button
              danger
              type="text"
              onClick={() => {
                const next = { ...bindings };
                delete next[fieldPath];
                update({ bindings: next });
              }}
            >
              解除绑定
            </Button>
          }
        >
          <Typography.Text code>
            {typeof binding === "string" ? binding : encode(binding)}
          </Typography.Text>
        </Card>
      ))}
      <Form layout="vertical">
        <Form.Item label="参数路径">
          <Input
            aria-label="绑定参数路径"
            placeholder="/report/name"
            value={path}
            onChange={(event) => setPath(event.target.value)}
          />
        </Form.Item>
        <Form.Item label="共享定义">
          <Select
            aria-label="绑定共享定义"
            value={definition}
            options={(definitions.data?.data?.items || []).map(
              (item: Data) => ({
                value: item.id,
                label: item.title || item.id,
              }),
            )}
            onChange={setDefinition}
          />
        </Form.Item>
        <Button
          disabled={!definition || !path.startsWith("/")}
          onClick={() => {
            update({
              bindings: Object.fromEntries([
                ...Object.entries(bindings),
                [path, definition],
              ]),
            });
            setPath("");
          }}
        >
          添加绑定
        </Button>
      </Form>
      <Button
        onClick={async () => {
          try {
            const result = await action(
              "parameter.resolve",
              resolveRequest(draft),
            );
            message.success("共享绑定检查通过");
            message.info(encode(result.data.origins || {}));
          } catch (error) {
            message.error(errorText(error));
          }
        }}
      >
        检查共享绑定
      </Button>
    </Space>
  );
}
function Parameters({ draft }: { draft: Draft }) {
  const { id, store } = useWorkspace();
  const presets = useQuery(parameterQuery(id, "presets"));
  const managed = draft.action === "template.render.managed";
  let asset: Data,
    values: Data,
    request: Data,
    problem = "";
  try {
    asset = assetOf(draft);
    values = decode(draft.parameters);
    if (managed) request = resolveRequest(draft);
  } catch (error) {
    problem = errorText(error);
  }
  const preview = useQuery({
    queryKey: [
      id,
      "resolve",
      managed ? encode(request || {}) : "",
      draft.replay,
    ],
    enabled: managed && !problem && !Object.keys(draft.invalidNumbers).length,
    queryFn: ({ signal }) => action("parameter.resolve", request, signal),
    retry: false,
  });
  const effective = managed
    ? (preview.data?.data?.values ?? merge(asset?.defaults || {}, values || {}))
    : values;
  const patch = (values: Partial<Draft>) =>
    store.getState().patch(draft.key, values);
  const operations = useDocumentOperations();
  return (
    <div className="parameters">
      <div className="section-toolbar">
        <Typography.Title level={5}>输入参数</Typography.Title>
        <Segmented
          aria-label="参数输入方式"
          value={draft.parameterMode}
          options={[
            { value: "form", label: "表单" },
            { value: "json", label: "JSON" },
          ]}
          onChange={(value) =>
            patch({ parameterMode: value as Draft["parameterMode"] })
          }
        />
      </div>
      {managed && (
        <Form layout="vertical">
          <Form.Item label="本次共享预置">
            <Select
              aria-label="本次共享预置"
              value={
                draft.preset === undefined
                  ? "__default"
                  : draft.preset === null
                    ? "__none"
                    : draft.preset
              }
              options={[
                {
                  value: "__default",
                  label: draft.replay ? "历史冻结参数" : "使用模板默认预置",
                },
                { value: "__none", label: "不使用共享预置" },
                ...(presets.data?.data?.items || []).map((item: Data) => ({
                  value: item.id,
                  label: item.title || item.id,
                })),
              ]}
              onChange={(value) =>
                patch({
                  preset:
                    value === "__default"
                      ? undefined
                      : value === "__none"
                        ? null
                        : value,
                  replay: undefined,
                })
              }
            />
          </Form.Item>
        </Form>
      )}
      {problem && (
        <Alert type="error" title="当前输入无效" description={problem} />
      )}
      {preview.isError && (
        <Alert
          type="warning"
          title="参数解析失败，当前不能生成可交付结果"
          description={errorText(preview.error)}
          action={<Button onClick={() => preview.refetch()}>重试</Button>}
        />
      )}
      {Object.keys(draft.invalidNumbers).length > 0 && (
        <Alert
          type="error"
          title="有未完成输入，请修正后生成"
          description={Object.keys(draft.invalidNumbers).join("、")}
        />
      )}
      {draft.parameterMode === "json" ? (
        <>
          <Typography.Paragraph type="secondary">
            {managed
              ? "JSON 仅保存本次手动覆盖；未填写的值继续跟随共享定义、预置与模板默认值。"
              : "编辑本次完整输入。无效 JSON 不会使用上一份有效参数执行。"}
          </Typography.Paragraph>
          {Object.keys(draft.invalidNumbers).length > 0 && (
            <Alert
              type="warning"
              title="请返回表单修正未完成字段。JSON 不会覆盖这些尚未完成的输入。"
            />
          )}
          <JsonDraft
            label="参数 JSON"
            value={draft.parameters}
            onChange={(parameters) => patch({ parameters })}
          />
        </>
      ) : (
        !problem && (
          <Form layout="vertical" component="div">
            <SchemaFields
              schema={asset.schema}
              value={effective}
              origins={preview.data?.data?.origins}
              invalid={draft.invalidNumbers}
              onInvalid={(path, raw) => {
                const invalidNumbers = {
                  ...store
                    .getState()
                    .documents.find((doc) => doc.key === draft.key)!
                    .invalidNumbers,
                };
                if (raw === undefined) delete invalidNumbers[pointer(path)];
                else invalidNumbers[pointer(path)] = raw;
                patch({ invalidNumbers });
              }}
              onChange={(value) => patch({ parameters: encode(value, true) })}
              onField={
                managed
                  ? (path, value) =>
                      patch({
                        parameters: encode(
                          overrideAt(values, effective, path, value),
                          true,
                        ),
                      })
                  : undefined
              }
            />
          </Form>
        )
      )}
      <div className="section-toolbar">
        <Space wrap size={[12, 8]}>
          <Button onClick={() => operations.savePreset(draft)}>
            保存参数预设
          </Button>
          <Button onClick={() => operations.loadPreset(draft)}>
            加载参数预设
          </Button>
        </Space>
        <Button
          onClick={() =>
            patch({
              parameters: managed ? "{}" : encode(asset?.defaults || {}, true),
              invalidNumbers: {},
            })
          }
        >
          重置参数
        </Button>
      </div>
    </div>
  );
}
export default function Workbench() {
  const { key } = useParams(),
    navigate = useNavigate(),
    { id, store, capabilities } = useWorkspace(),
    { modal, message } = App.useApp();
  const documents = useSession((state) => state.documents),
    active = useSession((state) => state.active);
  const draft = documents.find((doc) => doc.key === (key || active));
  const operations = useDocumentOperations(),
    screens = Grid.useBreakpoint(),
    [line, setLine] = useState<number>(),
    [column, setColumn] = useState<number>(),
    [mobileTab, setMobileTab] = useState("input");
  const [paneRatio, setPaneRatio] = useState(52);
  const latest = useQuery(runQuery(id, draft?.runId));
  useEffect(() => {
    if (key && documents.some((doc) => doc.key === key))
      store.getState().activate(key);
  }, [key, documents, store]);
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || !draft || event.isComposing)
        return;
      if (event.key.toLowerCase() === "s") {
        event.preventDefault();
        operations.save.mutate(draft);
      }
      if (event.key === "Enter") {
        event.preventDefault();
        operations.generate.mutate(draft);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  });
  const close = (closing: Draft) => {
    const perform = () => {
      store.getState().close(closing.key);
      navigate("/workbench/" + (store.getState().active || ""));
    };
    if (dirty(closing))
      modal.confirm({
        title: "关闭未保存的模板？",
        content: `「${closing.title}」尚未保存。`,
        okText: "丢弃草稿并关闭",
        cancelText: "继续编辑",
        onOk: perform,
      });
    else perform();
  };
  if (!draft)
    return (
      <div className="welcome">
        <Tag color="blue">模板 · 参数 · 结果</Tag>
        <Typography.Title>从一个模板开始</Typography.Title>
        <Typography.Paragraph type="secondary">
          在模板库打开已有模板，或新建模板编写你的第一份输出。
        </Typography.Paragraph>
        <Space wrap size={[12, 8]}>
          <Button
            type="primary"
            size="large"
            onClick={() => operations.go(makeDraft())}
          >
            新建模板
          </Button>
          <Button size="large" onClick={() => navigate("/help")}>
            查看入门示例
          </Button>
        </Space>
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description="尚未打开文档"
        />
      </div>
    );
  let problem = "";
  try {
    assertExecutable(assetOf(draft), draft.action);
    selectAction(draft, capabilities);
    decode(draft.parameters);
  } catch (error) {
    problem = errorText(error);
  }
  const patch = (values: Partial<Draft>) =>
    store.getState().patch(draft.key, values);
  const input = (
    <div className="workbench-input">
      <Parameters key={draft.key} draft={draft} />
      {draft.mode === "edit" && (
        <Tabs
          activeKey={draft.tab}
          onChange={(tab) => patch({ tab })}
          items={[
            {
              key: "source",
              label: "模板源码",
              children: (
                <>
                  <div className="section-toolbar">
                    <Typography.Text type="secondary">
                      Jinja 模板
                    </Typography.Text>
                    <Button
                      icon={<SearchOutlined aria-hidden />}
                      onClick={() => operations.inspect(draft)}
                    >
                      检查变量
                    </Button>
                  </div>
                  <CodeEditor
                    key={draft.key}
                    value={draft.source}
                    onChange={(source) => patch({ source })}
                    line={line}
                    column={column}
                  />
                </>
              ),
            },
            {
              key: "schema",
              label: "参数定义",
              children: (
                <Tabs
                  items={[
                    {
                      key: "designer",
                      label: "结构化编辑",
                      children: (
                        <SchemaDesigner
                          raw={draft.schema}
                          onChange={(schema) => patch({ schema })}
                        />
                      ),
                    },
                    {
                      key: "raw",
                      label: "高级 JSON",
                      children: (
                        <JsonDraft
                          label="参数定义 JSON"
                          value={draft.schema}
                          onChange={(schema) => patch({ schema })}
                        />
                      ),
                    },
                  ]}
                />
              ),
            },
            {
              key: "defaults",
              label: "默认值",
              children: (
                <JsonDraft
                  label="默认值 JSON"
                  value={draft.defaults}
                  onChange={(defaults) => patch({ defaults })}
                />
              ),
            },
            {
              key: "bindings",
              label: "共享绑定",
              children: <Bindings key={draft.key} draft={draft} />,
            },
            {
              key: "metadata",
              label: "元数据",
              children: (
                <>
                  <JsonDraft
                    label="元数据 JSON"
                    value={draft.metadata}
                    onChange={(metadata) =>
                      patch({ metadata, replay: undefined })
                    }
                  />
                  <JsonDraft
                    label="标签 JSON"
                    value={draft.tags}
                    onChange={(tags) => patch({ tags })}
                  />
                </>
              ),
            },
          ]}
        />
      )}
    </div>
  );
  const output = (
    <div className="workbench-output">
      <div className="section-toolbar">
        <Typography.Title level={5}>结果与诊断</Typography.Title>
        <Input
          aria-label="输出文件名"
          value={draft.outputName}
          onChange={(event) => patch({ outputName: event.target.value })}
          style={{ maxWidth: 240 }}
        />
      </div>
      <ResultPanel
        runId={draft.runId}
        draft={draft}
        onDiagnostic={(diagnostic) => {
          const fieldPath = String(diagnostic.path || "").replace(
            /^\/context/,
            "",
          );
          const target = [
            ...document.querySelectorAll<HTMLElement>("[data-field-path]"),
          ].find((element) => element.dataset.fieldPath === fieldPath);
          if (target) {
            target.scrollIntoView({ block: "center" });
            target.querySelector<HTMLElement>("input,textarea")?.focus();
          } else {
            const rootTemplate = draft.replay ? decode(draft.replay).snapshot?.template : undefined;
            if (diagnostic.template && ![draft.id, "__inline__", rootTemplate].includes(diagnostic.template)) {
              message.info(`问题位于模板 ${diagnostic.template}，请打开该模板查看`);
            } else if (diagnostic.line) {
              patch({ mode: "edit", tab: "source" });
              setLine(diagnostic.line);
              setColumn(diagnostic.column);
              setMobileTab("input");
            }
          }
        }}
      />
    </div>
  );
  return (
    <div className="workbench">
      <Tabs
        className="document-tabs"
        type="editable-card"
        activeKey={draft.key}
        onChange={(key) => navigate("/workbench/" + key)}
        onEdit={(target, operation) =>
          operation === "add"
            ? operations.go(makeDraft())
            : close(documents.find((doc) => doc.key === target)!)
        }
        items={documents.map((doc) => ({
          key: doc.key,
          label: doc.title + (dirty(doc) ? " •" : ""),
        }))}
      />
      <div className="document-header">
        <Space wrap size={[12, 8]}>
          <Input
            aria-label="模板名称"
            variant="borderless"
            value={draft.title}
            onChange={(event) => patch({ title: event.target.value })}
          />
          <Tag>{dirty(draft) ? "未保存" : "已保存"}</Tag>
        </Space>
        <Space wrap size={[12, 8]}>
          <Button
            icon={<CopyOutlined aria-hidden />}
            onClick={() => operations.copy(draft)}
          >
            复制模板
          </Button>
          <Button
            icon={<DownloadOutlined aria-hidden />}
            onClick={() => {
              try {
                saveFile(
                  (draft.id || "template") + ".json",
                  encode(assetOf(draft), true),
                  "application/json",
                );
              } catch (error) {
                message.error(errorText(error));
              }
            }}
          >
            导出模板
          </Button>
          <Button
            danger
            disabled={!draft.id}
            icon={<DeleteOutlined aria-hidden />}
            onClick={() => operations.remove(draft)}
          >
            删除模板
          </Button>
          <Button
            icon={<SaveOutlined aria-hidden />}
            loading={operations.save.isPending}
            onClick={() => operations.save.mutate(draft)}
          >
            保存模板
          </Button>
        </Space>
      </div>
      <div className="document-controls">
        <Space wrap size={[12, 8]}>
          {screens.xl && (
            <Space wrap size={[12, 8]}>
              <Typography.Text type="secondary">输入宽度</Typography.Text>
              <Slider
                ariaLabelForHandle="输入区域宽度"
                style={{ width: 90 }}
                min={25}
                max={75}
                value={paneRatio}
                onChange={setPaneRatio}
                tooltip={{ formatter: (value) => `${value}%` }}
              />
            </Space>
          )}
          <Segmented
            aria-label="模板模式"
            value={draft.mode}
            options={[
              { value: "use", label: "使用模板" },
              { value: "edit", label: "编辑模板" },
            ]}
            onChange={(mode) => patch({ mode: mode as Draft["mode"] })}
          />
          <Select
            aria-label="生成方式"
            value={draft.action}
            style={{ minWidth: 180 }}
            options={(capabilities.actions || [])
              .filter(
                (entry: Data) => entry.ui_schema?.document_input?.bindings,
              )
              .map((entry: Data) => ({
                value: entry.id,
                label: entry.ui_schema.document_input.title || entry.title,
              }))}
            onChange={(action) => patch({ action, replay: undefined })}
          />
          <Select
            aria-label="启用扩展"
            mode="multiple"
            placeholder="启用扩展"
            value={draft.extensions}
            style={{ minWidth: 140 }}
            options={
              capabilities.actions.some(
                (entry: Data) => entry.id === "math.info",
              )
                ? [{ value: "math", label: "math" }]
                : []
            }
            onChange={(extensions) => patch({ extensions })}
          />
        </Space>
        <Button
          type="primary"
          icon={<PlayCircleOutlined aria-hidden />}
          loading={operations.generate.isPending}
          disabled={
            !!problem ||
            Object.keys(draft.invalidNumbers).length > 0 ||
            running(latest.data?.status)
          }
          onClick={() => operations.generate.mutate(draft)}
        >
          生成文件
        </Button>
      </div>
      {problem && <Alert type="error" title={problem} />}
      {screens.xl ? (
        <Splitter
          className="workbench-splitter"
          onResize={(sizes) =>
            setPaneRatio((sizes[0] / (sizes[0] + sizes[1])) * 100)
          }
        >
          <Splitter.Panel size={`${paneRatio}%`} min="25%">
            {input}
          </Splitter.Panel>
          <Splitter.Panel min="25%">{output}</Splitter.Panel>
        </Splitter>
      ) : (
        <Tabs
          activeKey={mobileTab}
          onChange={setMobileTab}
          items={[
            { key: "input", label: "输入与编辑", children: input },
            { key: "output", label: "结果与诊断", children: output },
          ]}
        />
      )}
    </div>
  );
}
