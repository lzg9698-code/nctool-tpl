import { useState, useRef } from "react";
import {
  Alert,
  App,
  Button,
  Card,
  Form,
  Input,
  Space,
  Switch,
  Table,
  Tabs,
  Tag,
  Typography,
  Upload,
} from "antd";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useWorkspace, useSession } from "../../app/context";
import { api, queryClient } from "../../lib/api";
import {
  decode,
  encode,
  errorText,
  pointer,
  saveFile,
  type Data,
} from "../../lib/json";
import { CodeEditor } from "../../components/LazyCodeEditor";
import { SchemaFields } from "../../components/SchemaFields";

function BundleTools() {
  const { id } = useWorkspace(),
    { message, modal } = App.useApp();
  const [busy, setBusy] = useState(false);
  return (
    <Card title="工作区资产包">
      <Typography.Paragraph type="secondary">
        仅在你明确选择时导入或导出资产集合。启动和重写过程不会迁移任何资产。
      </Typography.Paragraph>
      <Space wrap size={[12, 8]}>
        <Button
          loading={busy}
          onClick={async () => {
            setBusy(true);
            try {
              saveFile(
                "workspace.json",
                encode(await api("bundle"), true),
                "application/json",
              );
            } catch (error) {
              message.error(errorText(error));
            } finally {
              setBusy(false);
            }
          }}
        >
          导出工作区
        </Button>
        <Upload
          accept=".json"
          showUploadList={false}
          beforeUpload={async (file) => {
            try {
              if (file.size > 64 * 1024 * 1024)
                throw new Error("资产包超过 64 MiB");
              const bundle = decode(await file.text()),
                preview = await api("bundle/validate", { bundle });
              modal.confirm({
                title: "确认导入资产集合",
                width: 640,
                content: (
                  <>
                    <p>不覆盖现有资产。标识冲突会阻止整体导入。</p>
                    <pre>{encode(preview, true)}</pre>
                  </>
                ),
                okText: "确认导入",
                cancelText: "取消",
                onOk: async () => {
                  try {
                    await api("bundle/import", { bundle });
                    await queryClient.invalidateQueries({ queryKey: [id] });
                    message.success("资产集合已导入");
                  } catch (error) {
                    message.error(errorText(error));
                    throw error;
                  }
                },
              });
            } catch (error) {
              message.error(errorText(error));
            }
            return false;
          }}
        >
          <Button>选择资产包并检查</Button>
        </Upload>
      </Space>
    </Card>
  );
}
export default function PluginsPage() {
  const { id, store } = useWorkspace(),
    { message, modal } = App.useApp();
  const inventory = useQuery({
    queryKey: [id, "plugins"],
    queryFn: ({ signal }) => api("plugins", undefined, signal),
  });
  const config = useQuery({
    queryKey: [id, "config"],
    queryFn: ({ signal }) => api("config", undefined, signal),
    staleTime: Infinity,
  });
  const stored = useSession((state) => state.auxiliary.config),
    raw =
      stored?.raw ?? (config.data ? encode(config.data.config, true) : "{}");
  const expected = stored?.expected ?? config.data?.fingerprint ?? null;
  const [source, setSource] = useState(""),
    sourceRef = useRef(source);
  sourceRef.current = source;
  const [validation, setValidation] = useState<{ raw: string; result: Data }>();
  const inspect = useQuery({
    queryKey: [id, "inspect", source],
    enabled: false,
    queryFn: ({ signal }) => api("plugins/inspect", { source }, signal),
    retry: false,
  });
  let parsed: Data,
    problem = "";
  try {
    parsed = decode(raw);
  } catch (error) {
    problem = errorText(error);
  }
  if (Object.keys(stored?.invalid || {}).length)
    problem = "请修正未完成的插件配置字段";
  const change = (
    raw: string,
    invalid = store.getState().auxiliary.config?.invalid || {},
  ) => {
    store
      .getState()
      .setAuxiliary("config", {
        raw,
        expected,
        saved: stored?.saved || encode(config.data?.config || {}),
        invalid,
      });
    setValidation(undefined);
  };
  const validate = useMutation({
    mutationFn: async (captured: string) => {
      const result = await api("config/validate", { config: decode(captured) });
      setValidation({ raw: captured, result });
      return result;
    },
    onError: (error) => message.error(errorText(error)),
  });
  const save = useMutation({
    mutationFn: async () => {
      if (Object.keys(stored?.invalid || {}).length)
        throw new Error("请先修正未完成输入");
      const captured = raw;
      await validate.mutateAsync(captured);
      await api("config", { config: decode(captured), expected });
      const saved = await api("config"),
        current = store.getState().auxiliary.config;
      store
        .getState()
        .setAuxiliary("config", {
          raw: current?.raw || captured,
          invalid: current?.invalid,
          expected: saved.fingerprint,
          saved: captured,
        });
      await queryClient.invalidateQueries({ queryKey: [id, "plugins"] });
      message.success("配置已保存，下次启动生效");
    },
    onError: (error) => message.error(errorText(error)),
  });
  const install = useMutation({
    mutationFn: async (directory: string) =>
      api("plugins/install", { source: directory }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: [id, "plugins"] });
      message.success("插件已安装，请配置启用并重启服务");
    },
    onError: (error) => message.error(errorText(error)),
  });
  return (
    <div className="page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>插件与设置</Typography.Title>
          <Typography.Text type="secondary">
            运行能力与下次启动配置分开显示。保存配置后需自行重启服务。
          </Typography.Text>
        </div>
      </div>
      {inventory.data?.restart_required && (
        <Alert type="warning" title="配置已变化，重启服务后生效" />
      )}
      {(inventory.isError || config.isError) && (
        <Alert
          type="error"
          title="配置或插件列表读取失败"
          description={errorText(inventory.error || config.error)}
          action={
            <Button
              onClick={() => {
                inventory.refetch();
                config.refetch();
              }}
            >
              重试
            </Button>
          }
        />
      )}
      <Table
        rowKey="id"
        dataSource={(inventory.data?.plugins || []).filter(
          (plugin: Data) => !plugin.retired,
        )}
        pagination={false}
        columns={[
          { title: "插件", dataIndex: "id" },
          {
            title: "当前状态",
            render: (_, plugin: Data) => (
              <Tag color={plugin.active ? "success" : "default"}>
                {plugin.active
                  ? "运行中"
                  : plugin.available
                    ? "未启用"
                    : "未包含"}
              </Tag>
            ),
          },
          {
            title: "下次启动",
            render: (_, plugin: Data) => (
              <Switch
                aria-label={"启用 " + plugin.id}
                disabled={!config.data || !!problem || !plugin.available}
                checked={
                  (parsed?.enabled || []).includes(plugin.id) ||
                  ((plugin.id === "template" || plugin.id === "parameters") &&
                    !(parsed?.disabled || []).includes(plugin.id))
                }
                onChange={(checked) =>
                  change(
                    encode(
                      {
                        ...parsed,
                        enabled: [
                          ...(parsed.enabled || []).filter(
                            (entry: string) => entry !== plugin.id,
                          ),
                          ...(checked ? [plugin.id] : []),
                        ],
                        disabled: [
                          ...(parsed.disabled || []).filter(
                            (entry: string) => entry !== plugin.id,
                          ),
                          ...(!checked ? [plugin.id] : []),
                        ],
                      },
                      true,
                    ),
                  )
                }
              />
            ),
          },
          {
            title: "操作",
            render: (_, plugin: Data) =>
              !plugin.builtin && (
                <Button
                  danger
                  disabled={plugin.active}
                  onClick={() =>
                    modal.confirm({
                      title: "卸载插件？",
                      content: plugin.id,
                      okText: "卸载",
                      cancelText: "取消",
                      onOk: async () => {
                        try {
                          await api("plugins/uninstall", { id: plugin.id });
                          await queryClient.invalidateQueries({
                            queryKey: [id, "plugins"],
                          });
                        } catch (error) {
                          message.error(errorText(error));
                          throw error;
                        }
                      },
                    })
                  }
                >
                  卸载
                </Button>
              ),
          },
        ]}
      />
      <div className="settings-grid">
        <Card
          title="启动配置"
          extra={
            <Space wrap size={[12, 8]}>
              <Button
                loading={validate.isPending}
                disabled={!config.data || !!problem}
                onClick={() => validate.mutate(raw)}
              >
                检查配置
              </Button>
              <Button
                type="primary"
                loading={save.isPending}
                disabled={!config.data || !!problem}
                onClick={() => save.mutate()}
              >
                验证并保存配置
              </Button>
            </Space>
          }
        >
          {problem && (
            <Alert
              type="error"
              title="启动配置 JSON 无效"
              description={problem}
            />
          )}
          {validation?.raw === raw && (
            <Alert
              type="success"
              title="配置预检查通过"
              description={<pre>{encode(validation.result, true)}</pre>}
            />
          )}
          <Tabs
            items={[
              {
                key: "form",
                label: "插件参数",
                children: parsed && (
                  <Form component="div" layout="vertical">
                    {(inventory.data?.plugins || [])
                      .filter(
                        (plugin: Data) => plugin.descriptor?.config_schema,
                      )
                      .map((plugin: Data) => (
                        <Form.Item key={plugin.id} label={plugin.id}>
                          <SchemaFields
                            schema={plugin.descriptor.config_schema}
                            value={parsed?.plugins?.[plugin.id] || {}}
                            path={[plugin.id]}
                            invalid={stored?.invalid}
                            onChange={(value) =>
                              change(
                                encode(
                                  {
                                    ...parsed,
                                    plugins: {
                                      ...parsed.plugins,
                                      [plugin.id]: value,
                                    },
                                  },
                                  true,
                                ),
                              )
                            }
                            onInvalid={(path, raw) => {
                              const invalid = {
                                ...store.getState().auxiliary.config?.invalid,
                              };
                              if (raw === undefined)
                                delete invalid[pointer(path)];
                              else invalid[pointer(path)] = raw;
                              change(
                                store.getState().auxiliary.config?.raw ||
                                  encode(parsed, true),
                                invalid,
                              );
                            }}
                          />
                        </Form.Item>
                      ))}
                  </Form>
                ),
              },
              {
                key: "json",
                label: "完整配置 JSON",
                children: (
                  <CodeEditor
                    label="启动配置 JSON"
                    language="json"
                    value={raw}
                    onChange={(raw) => change(raw)}
                  />
                ),
              },
            ]}
          />
        </Card>
        <Card title="安装本地插件">
          <Form layout="vertical">
            <Form.Item label="插件目录">
              <Input
                aria-label="插件目录"
                placeholder="本地服务机器上包含 plugin.json 的目录"
                value={source}
                onChange={(event) => setSource(event.target.value)}
              />
            </Form.Item>
          </Form>
          <Button
            disabled={!source.trim()}
            loading={inspect.isFetching}
            onClick={() => inspect.refetch()}
          >
            读取插件信息
          </Button>
          {inspect.isError && (
            <Alert type="error" title={errorText(inspect.error)} />
          )}
          {inspect.data && (
            <>
              <pre>{encode(inspect.data, true)}</pre>
              <Button
                type="primary"
                loading={install.isPending}
                onClick={() => {
                  const captured = sourceRef.current;
                  modal.confirm({
                    title: "安装此插件？",
                    content: "安装会验证并复制文件，不会自动启用。",
                    okText: "安装插件",
                    cancelText: "取消",
                    onOk: () => install.mutateAsync(captured),
                  });
                }}
              >
                安装插件
              </Button>
            </>
          )}
        </Card>
      </div>
      <BundleTools />
    </div>
  );
}
