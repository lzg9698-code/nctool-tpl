import { useState } from "react";
import {
  Alert,
  App,
  Button,
  Card,
  Form,
  Input,
  Segmented,
  Select,
  Space,
  Splitter,
  Table,
  Tabs,
  Typography,
} from "antd";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useSession, useWorkspace } from "../../app/context";
import { action, ApiError, parameterQuery, queryClient } from "../../lib/api";
import { decode, encode, errorText, pointer, type Data } from "../../lib/json";
import { SchemaFields } from "../../components/SchemaFields";
import { CodeEditor } from "../../components/LazyCodeEditor";
import { DiffDisplay } from "../../components/ResultPanel";

export default function ParametersPage() {
  const { id, store } = useWorkspace(),
    { message, modal } = App.useApp();
  const [group, setGroup] = useState("definitions"),
    [selected, setSelected] = useState("new"),
    [search, setSearch] = useState("");
  const drafts = useSession((state) => state.auxiliary),
    key = `parameters:${group}:${selected}`;
  const catalog = useQuery(parameterQuery(id, group)),
    definitions = useQuery(parameterQuery(id, "definitions"));
  const read = useQuery({
    queryKey: [id, "parameter", group, selected],
    enabled: selected !== "new" && !selected.startsWith("new-"),
    queryFn: ({ signal }) =>
      action("parameter.read", { group, name: selected }, signal),
  });
  const seed = {
    format_version: 1,
    id: "",
    title: "",
    ...(group === "definitions"
      ? { schema: { type: "string" }, unit: "", group: "", description: "" }
      : { values: {} }),
  };
  const base = read.data?.data;
  const raw = drafts[key]?.raw ?? encode(base?.asset || seed, true),
    expected = drafts[key]?.expected ?? base?.fingerprint ?? null;
  const saved = drafts[key]?.saved ?? (base ? encode(base.asset, true) : "");
  const invalid = drafts[key]?.invalid || {};
  const change = (
    raw: string,
    nextInvalid = store.getState().auxiliary[key]?.invalid || {},
  ) =>
    store
      .getState()
      .setAuxiliary(key, { raw, expected, saved, invalid: nextInvalid });
  let asset: Data,
    problem = "";
  try {
    asset = decode(raw);
  } catch (error) {
    problem = errorText(error);
  }
  const patch = (values: Data) => change(encode({ ...asset, ...values }, true));
  const save = useMutation({
    mutationFn: async () => {
      if (problem || Object.keys(invalid).length)
        throw new Error("请先修正当前 JSON 或数值输入");
      if (!asset.id?.trim()) throw new Error("请填写稳定资产 ID");
      const captured = raw,
        capturedKey = key,
        capturedGroup = group,
        name = asset.id;
      try {
        const result = await action("parameter.save", {
          group: capturedGroup,
          name,
          asset,
          expected,
        });
        const latest = store.getState().auxiliary[capturedKey];
        store
          .getState()
          .setAuxiliary(capturedKey, {
            raw: latest?.raw ?? captured,
            invalid: latest?.invalid,
            expected: result.data.fingerprint,
            saved: captured,
          });
        await queryClient.invalidateQueries({
          queryKey: [id, "parameters", capturedGroup],
        });
        message.success("参数资产已保存");
      } catch (error) {
        if (!(error instanceof ApiError) || error.detail.code !== "conflict")
          throw error;
        let disk: Data;
        try {
          disk = (
            await action("parameter.read", { group: capturedGroup, name })
          ).data;
        } catch {
          /* deleted asset */
        }
        const dialog = modal.info({
          title: "参数资产保存冲突",
          width: 720,
          content: (
            <Space vertical style={{ width: "100%" }}>
              <DiffDisplay
                before={disk ? encode(disk.asset, true) : "资产已删除"}
                after={captured}
              />
              <Space wrap size={[12, 8]}>
                <Button
                  disabled={!disk}
                  onClick={() =>
                    modal.confirm({
                      title: "丢弃当前草稿并加载磁盘版本？",
                      okText: "加载磁盘版本",
                      cancelText: "继续编辑",
                      onOk: () => {
                        store
                          .getState()
                          .setAuxiliary(capturedKey, {
                            raw: encode(disk.asset, true),
                            saved: encode(disk.asset, true),
                            expected: disk.fingerprint,
                          });
                        dialog.destroy();
                      },
                    })
                  }
                >
                  加载磁盘版本
                </Button>
                <Button
                  onClick={() => {
                    const latest = decode(
                      store.getState().auxiliary[capturedKey]?.raw || captured,
                    );
                    const copyId =
                      name + "-copy-" + crypto.randomUUID().slice(0, 8);
                    store
                      .getState()
                      .setAuxiliary(`parameters:${capturedGroup}:new`, {
                        raw: encode(
                          {
                            ...latest,
                            id: copyId,
                            title: (latest.title || name) + " 副本",
                          },
                          true,
                        ),
                        saved: "",
                        expected: null,
                      });
                    setGroup(capturedGroup);
                    setSelected("new");
                    dialog.destroy();
                  }}
                >
                  另存副本
                </Button>
              </Space>
            </Space>
          ),
          okText: "继续编辑",
        });
      }
    },
    onError: (error) => message.error(errorText(error)),
  });
  const entries: Data[] = catalog.data?.data?.items || [];
  const controls = (
    <Space wrap size={[12, 8]}>
      <Button
        onClick={() => {
          const fresh = "new-" + crypto.randomUUID();
          setSelected(fresh);
        }}
      >
        新建资产
      </Button>
      <Button
        type="primary"
        loading={save.isPending}
        disabled={
          !!problem ||
          Object.keys(invalid).length > 0 ||
          (selected !== "new" && !selected.startsWith("new-") && !base)
        }
        onClick={() => save.mutate()}
      >
        保存参数资产
      </Button>
      <Button
        danger
        disabled={!expected}
        onClick={() =>
          modal.confirm({
            title: "删除参数资产？",
            content: "删除后不可恢复。已有引用可能因此失效。",
            okText: "删除资产",
            cancelText: "保留",
            onOk: async () => {
              try {
                await action("parameter.remove", {
                  group,
                  name: asset.id,
                  expected,
                });
                await queryClient.invalidateQueries({
                  queryKey: [id, "parameters", group],
                });
                setSelected("new");
              } catch (error) {
                message.error(errorText(error));
                throw error;
              }
            },
          })
        }
      >
        删除资产
      </Button>
    </Space>
  );
  return (
    <div className="page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>参数管理</Typography.Title>
          <Typography.Text type="secondary">
            维护共享定义与共享预置。未保存编辑只保存在新版浏览器草稿中。
          </Typography.Text>
        </div>
        {controls}
      </div>
      <Segmented
        value={group}
        options={[
          { value: "definitions", label: "参数定义" },
          { value: "presets", label: "共享预置" },
        ]}
        onChange={(value) => {
          setGroup(value);
          setSelected("new");
        }}
      />
      <Splitter className="management-splitter">
        <Splitter.Panel defaultSize="30%" min="20%">
          <div className="pane">
            <Input.Search
              aria-label="搜索参数资产"
              placeholder="搜索名称或标识"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
            {catalog.isError && (
              <Alert
                type="error"
                title={errorText(catalog.error)}
                action={<Button onClick={() => catalog.refetch()}>重试</Button>}
              />
            )}
            <Table
              size="small"
              rowKey="id"
              loading={catalog.isFetching}
              pagination={{ pageSize: 20 }}
              dataSource={entries.filter((item) =>
                `${item.id} ${item.title}`
                  .toLowerCase()
                  .includes(search.toLowerCase()),
              )}
              columns={[
                {
                  title: "名称 / 标识",
                  render: (_, item) => (
                    <Button type="link" onClick={() => setSelected(item.id)}>
                      {item.title || item.id}
                      <Typography.Text type="secondary">
                        {" "}
                        · {item.id}
                      </Typography.Text>
                    </Button>
                  ),
                },
              ]}
            />
          </div>
        </Splitter.Panel>
        <Splitter.Panel>
          <div className="pane">
            {read.isError && (
              <Alert
                type="error"
                title={errorText(read.error)}
                action={
                  <Button onClick={() => read.refetch()}>重试读取</Button>
                }
              />
            )}
            {problem && (
              <Alert
                type="error"
                title="资产 JSON 无效"
                description={problem}
              />
            )}
            {Object.keys(invalid).length > 0 && (
              <Alert type="error" title="有未完成数值，修正后才能保存" />
            )}
            <Tabs
              items={[
                {
                  key: "form",
                  label: "结构化编辑",
                  children: !problem && (
                    <Form layout="vertical" component="div">
                      <Form.Item label="稳定资产 ID">
                        <Input
                          aria-label="稳定资产 ID"
                          disabled={!!expected}
                          value={asset.id}
                          onChange={(event) =>
                            patch({ id: event.target.value })
                          }
                        />
                      </Form.Item>
                      <Form.Item label="显示名称">
                        <Input
                          aria-label="参数显示名称"
                          value={asset.title}
                          onChange={(event) =>
                            patch({ title: event.target.value })
                          }
                        />
                      </Form.Item>
                      {group === "definitions" ? (
                        <>
                          <Form.Item label="参数类型">
                            <Select
                              aria-label="共享参数类型"
                              value={asset.schema?.type || "string"}
                              options={[
                                "string",
                                "number",
                                "integer",
                                "boolean",
                                "object",
                                "array",
                                "null",
                              ].map((value) => ({ value, label: value }))}
                              onChange={(type) =>
                                patch({ schema: { ...asset.schema, type } })
                              }
                            />
                          </Form.Item>
                          <Form.Item label="单位">
                            <Input
                              value={asset.unit || ""}
                              onChange={(event) =>
                                patch({ unit: event.target.value })
                              }
                            />
                          </Form.Item>
                          <Form.Item label="分组">
                            <Input
                              value={asset.group || ""}
                              onChange={(event) =>
                                patch({ group: event.target.value })
                              }
                            />
                          </Form.Item>
                          <Form.Item label="说明">
                            <Input
                              value={asset.description || ""}
                              onChange={(event) =>
                                patch({ description: event.target.value })
                              }
                            />
                          </Form.Item>
                          <Form.Item label="默认值">
                            <SchemaFields
                              schema={asset.schema || { type: "string" }}
                              value={asset.default}
                              invalid={invalid}
                              onInvalid={(path, value) => {
                                const next = { ...invalid };
                                if (value === undefined)
                                  delete next[pointer(path)];
                                else next[pointer(path)] = value;
                                change(raw, next);
                              }}
                              onChange={(value) => patch({ default: value })}
                            />
                          </Form.Item>
                          <Card size="small" title="约束与嵌套结构">
                            <Typography.Paragraph type="secondary">
                              在高级 JSON 的 schema 中编辑
                              minimum、maximum、enum、对象属性或数组元素；后端负责完整校验。
                            </Typography.Paragraph>
                          </Card>
                        </>
                      ) : (
                        <SchemaFields
                          schema={{
                            type: "object",
                            properties: Object.fromEntries(
                              (definitions.data?.data?.items || []).map(
                                (entry: Data) => [
                                  entry.id,
                                  {
                                    ...(entry.asset?.schema ||
                                      entry.schema ||
                                      {}),
                                    title: entry.title || entry.id,
                                  },
                                ],
                              ),
                            ),
                          }}
                          value={asset.values || {}}
                          invalid={invalid}
                          onInvalid={(path, value) => {
                            const next = { ...invalid };
                            if (value === undefined) delete next[pointer(path)];
                            else next[pointer(path)] = value;
                            change(raw, next);
                          }}
                          onChange={(values) => patch({ values })}
                        />
                      )}
                    </Form>
                  ),
                },
                {
                  key: "json",
                  label: "高级 JSON",
                  children: (
                    <CodeEditor
                      label="参数资产 JSON"
                      language="json"
                      value={raw}
                      onChange={(raw) => change(raw)}
                    />
                  ),
                },
              ]}
            />
          </div>
        </Splitter.Panel>
      </Splitter>
    </div>
  );
}
