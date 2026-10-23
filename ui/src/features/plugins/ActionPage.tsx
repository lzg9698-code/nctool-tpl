import {
  Alert,
  App,
  Button,
  Form,
  Select,
  Space,
  Tabs,
  Typography,
} from "antd";
import { useParams } from "react-router-dom";
import { useSession, useStartRun, useWorkspace } from "../../app/context";
import {
  decode,
  encode,
  errorText,
  object,
  pointer,
  type Data,
} from "../../lib/json";
import { CodeEditor } from "../../components/LazyCodeEditor";
import { SchemaFields } from "../../components/SchemaFields";
import { ResultPanel } from "../../components/ResultPanel";
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { running, runQuery } from "../../lib/api";

export default function ActionPage() {
  const { panelId } = useParams(),
    { id, store, capabilities } = useWorkspace(),
    { message } = App.useApp(),
    start = useStartRun();
  const panel = capabilities.panels.find((entry: Data) => entry.id === panelId);
  const entries = capabilities.actions.filter((entry: Data) =>
    panel?.actions?.includes(entry.id),
  );
  const [selected, setSelected] = useState<string>();
  const entry =
    entries.find((entry: Data) => entry.id === selected) || entries[0];
  const key = "action:" + panelId + ":" + entry?.id,
    saved = useSession((state) => state.auxiliary[key]);
  const raw = saved?.raw ?? encode(entry?.example || {}, true),
    invalid = saved?.invalid || {};
  const current = useQuery(runQuery(id, saved?.runId));
  let value: Data,
    problem = "";
  try {
    value = decode(raw);
    if (object(entry?.input_schema) && entry.input_schema.type === "object" && !object(value))
      throw new Error("此动作的输入必须是 JSON 对象");
  } catch (error) {
    problem = errorText(error);
  }
  const change = (
    raw: string,
    invalid: Record<string, string> = store.getState().auxiliary[key]
      ?.invalid || {},
  ) =>
    store
      .getState()
      .setAuxiliary(key, {
        ...store.getState().auxiliary[key],
        raw,
        expected: null,
        saved: "",
        invalid,
      });
  if (!panel || !entry)
    return <Alert type="warning" title="插件页面或动作当前不可用" />;
  return (
    <div className="page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>{panel.title}</Typography.Title>
          <Typography.Text type="secondary">
            由当前已启用插件声明的操作。
          </Typography.Text>
        </div>
      </div>
      <Space wrap size={[12, 8]}>
        <Select
          aria-label="选择插件操作"
          value={entry.id}
          style={{ minWidth: 240 }}
          options={entries.map((entry: Data) => ({
            value: entry.id,
            label: entry.title,
          }))}
          onChange={setSelected}
        />
        <Button
          type="primary"
          loading={start.isPending}
          aria-busy={start.isPending}
          disabled={
            start.isPending || !!problem ||
            Object.keys(invalid).length > 0 ||
            running(current.data?.status)
          }
          onClick={async () => {
            const capturedKey = key,
              capturedRaw = raw;
            try {
              const runId = await start.mutateAsync({
                action: entry.id,
                input: decode(capturedRaw),
                title: entry.title,
              });
              const latest = store.getState().auxiliary[capturedKey];
              store
                .getState()
                .setAuxiliary(capturedKey, {
                  ...latest,
                  raw: latest?.raw || capturedRaw,
                  expected: null,
                  saved: "",
                  runId,
                  runSignature: capturedRaw,
                });
            } catch (error) {
              message.error(errorText(error));
            }
          }}
        >
          执行操作
        </Button>
      </Space>
      {problem && (
        <Alert type="error" title="当前 JSON 无效" description={problem} />
      )}
      {Object.keys(invalid).length > 0 && (
        <Alert type="error" title="请修正未完成输入" />
      )}
      <Tabs
        items={[
          {
            key: "form",
            label: "表单",
            children: !problem && (
              <Form component="div" layout="vertical">
                <SchemaFields
                  schema={entry.input_schema}
                  value={value}
                  invalid={invalid}
                  onChange={(value) => change(encode(value, true))}
                  onInvalid={(path, raw) => {
                    const next = { ...invalid };
                    if (raw === undefined) delete next[pointer(path)];
                    else next[pointer(path)] = raw;
                    change(saved?.raw || encode(value, true), next);
                  }}
                />
              </Form>
            ),
          },
          {
            key: "json",
            label: "JSON",
            children: (
              <CodeEditor
                label="插件输入 JSON"
                language="json"
                value={raw}
                onChange={(raw) => change(raw)}
              />
            ),
          },
        ]}
      />
      <ResultPanel
        runId={saved?.runId}
        forceReason={
          saved?.runId &&
          (raw !== saved.runSignature || Object.keys(invalid).length)
            ? "输入已修改，请重新执行"
            : undefined
        }
      />
    </div>
  );
}
