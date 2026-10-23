import { useState } from "react";
import { Alert, App, Button, Card, Input, Space, Tag, Typography } from "antd";
import { useNavigate } from "react-router-dom";
import { useWorkspace, useStartRun } from "../../app/context";
import { api, queryClient } from "../../lib/api";
import { errorText, encode, type Data } from "../../lib/json";
import { sampleBundle } from "./examples";
import { ResultPanel } from "../../components/ResultPanel";

const syntax = [
  {
    title: "变量与对象",
    description: "双花括号输出变量，点号访问对象属性。",
    source: "你好，{{ user.name }}！",
    context: { user: { name: "Ada" } },
  },
  {
    title: "条件",
    description: "根据参数选择输出内容。",
    source: "{% if enabled %}已启用{% else %}未启用{% endif %}",
    context: { enabled: true },
  },
  {
    title: "循环",
    description: "遍历数组，生成重复段落。",
    source: "{% for item in items %}{{ loop.index }}. {{ item }}\n{% endfor %}",
    context: { items: ["模板", "参数", "结果"] },
  },
  {
    title: "过滤器",
    description: "用竖线连接格式处理。",
    source: "{{ name | upper }}",
    context: { name: "Ada" },
  },
  {
    title: "数学扩展与角度单位",
    description:
      "需当前服务已启用 math，并在本次执行中显式选择。角度函数按插件定义使用。",
    source: "{{ 9 | sqrt }}",
    context: {},
    extensions: ["math"],
  },
];
export default function HelpPage() {
  const { id, capabilities } = useWorkspace(),
    { message, modal } = App.useApp(),
    start = useStartRun(),
    navigate = useNavigate();
  const [search, setSearch] = useState(""),
    [runId, setRunId] = useState<string>(),
    [saving, setSaving] = useState(false);
  const hasMath = capabilities.actions.some(
    (entry: Data) => entry.id === "math.info",
  );
  const trial = async (
    source: string,
    context: Data,
    extensions: string[] = [],
  ) => {
    try {
      setRunId(
        await start.mutateAsync({
          action: "template.render",
          input: { source, context, extensions },
          title: "语法示例试跑",
        }),
      );
    } catch (error) {
      message.error(errorText(error));
    }
  };
  const saveExample = async (shared: boolean) => {
    setSaving(true);
    try {
      const sample = sampleBundle(shared),
        preview = await api("bundle/validate", { bundle: sample.bundle });
      modal.confirm({
        title: "确认保存整个示例",
        width: 600,
        content: (
          <>
            <p>
              使用新标识保存示例模板{shared ? "、共享定义和共享预置" : ""}。
            </p>
            <pre>{encode(preview, true)}</pre>
          </>
        ),
        okText: "确认保存整个示例",
        cancelText: "取消",
        onOk: async () => {
          try {
            await api("bundle/import", { bundle: sample.bundle });
            await queryClient.invalidateQueries({ queryKey: [id] });
            message.success("示例已保存，请在模板库打开");
            navigate("/workbench");
          } catch (error) {
            message.error(errorText(error));
            throw error;
          }
        },
      });
    } catch (error) {
      message.error(errorText(error));
    } finally {
      setSaving(false);
    }
  };
  return (
    <div className="page">
      <div className="page-heading">
        <div>
          <Tag color="blue">开始使用</Tag>
          <Typography.Title level={2}>模板语法与入门示例</Typography.Title>
          <Typography.Text type="secondary">
            先试跑，再保存。预览不会写入工作区资产。
          </Typography.Text>
        </div>
      </div>
      <div className="example-grid">
        {[false, true].map((shared) => {
          const sample = sampleBundle(shared);
          return (
            <Card
              key={String(shared)}
              title={shared ? "共享参数示例" : "普通文本示例"}
            >
              <pre>{sample.asset.source}</pre>
              <Typography.Paragraph>
                预期结果：{sample.expected}
              </Typography.Paragraph>
              <Space wrap size={[12, 8]}>
                <Button
                  loading={start.isPending}
                  onClick={() => trial(sample.asset.source, sample.context)}
                >
                  预览示例
                </Button>
                <Button loading={saving} onClick={() => saveExample(shared)}>
                  保存{shared ? "共享参数" : "普通文本"}示例
                </Button>
              </Space>
            </Card>
          );
        })}
      </div>
      <Typography.Title level={3}>语法参考</Typography.Title>
      <Input.Search
        aria-label="搜索模板语法"
        placeholder="搜索变量、条件、循环或过滤器"
        value={search}
        onChange={(event) => setSearch(event.target.value)}
      />
      <div className="example-grid">
        {syntax
          .filter((item) =>
            `${item.title} ${item.description} ${item.source}`.includes(search),
          )
          .map((item) => (
            <Card
              key={item.title}
              title={item.title}
              extra={item.extensions && <Tag>math</Tag>}
            >
              <Typography.Paragraph type="secondary">
                {item.description}
              </Typography.Paragraph>
              <pre>{item.source}</pre>
              <Space wrap size={[12, 8]}>
                <Button
                  onClick={() =>
                    navigator.clipboard
                      .writeText(item.source)
                      .then(() => message.success("片段已复制"))
                      .catch((error) => message.error(errorText(error)))
                  }
                >
                  复制片段
                </Button>
                <Button
                  loading={start.isPending}
                  disabled={!!item.extensions && !hasMath}
                  onClick={() =>
                    trial(item.source, item.context, item.extensions)
                  }
                >
                  试跑示例
                </Button>
              </Space>
              {item.extensions && !hasMath && (
                <Alert type="info" title="当前服务未启用 math 扩展" />
              )}
            </Card>
          ))}
      </div>
      <ResultPanel runId={runId} />
    </div>
  );
}
