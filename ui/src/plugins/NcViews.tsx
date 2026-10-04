import { useEffect, useState } from "react";
import {
  Play,
  Plus,
  Save,
  Trash2,
  ArrowUp,
  ArrowDown,
  Copy,
  FileCode2,
  AlertTriangle,
  FolderOpen,
} from "lucide-react";
import { action, api } from "../lib/api";
import {
  clone,
  defaultValue,
  encode,
  parseData,
  type Data,
  type Run,
  type Schema,
  type Diagnostic,
} from "../lib/data";
import {
  loadDrafts,
  saveDrafts,
  cloneDraft,
  encodeDraft,
  decodeDraft,
} from "../lib/drafts";
import { Form } from "../components/Form";
import { Editor } from "../components/Editor";
import {
  PageTitle,
  Empty,
  JsonEditor,
  RunDetail,
} from "../components/WorkbenchUi";
export interface ViewProps {
  panel: Data;
  capabilities: Data;
  start: (id: string, input: Data, title: string) => Promise<string | null>;
  runs: Record<string, Run>;
  fail: (error: Data) => void;
  workspaceId: string;
}
interface Template {
  id: string;
  title: string;
  schema?: Schema;
  asset: Data;
}
const generationOptions: Schema = {
  type: "object",
  properties: {
    line_numbers: { type: "boolean", title: "连续行号", default: false },
    line_number_step: { type: "integer", title: "行号步进", default: 10 },
    max_line_number: { type: "integer", title: "最大行号", default: 9999 },
    add_header_comment: {
      type: "boolean",
      title: "生成说明注释",
      default: false,
    },
    strip_blank_lines: { type: "boolean", title: "清理空行", default: false },
    ascii_only: { type: "boolean", title: "仅输出 ASCII", default: false },
  },
};
async function resources() {
  const catalog = await action("template.catalog", {});
  const templates = await Promise.all(
    catalog.data.items.map(async (item: Data) => {
      const read = await action("template.read", { name: item.id });
      return {
        ...item,
        asset: read.data.asset,
        schema: read.data.asset.schema,
      } as Template;
    }),
  );
  const machines = (await action("machine.list", {})).data.names as string[];
  return { templates, machines };
}
function useResources(fail: (e: Data) => void) {
  const [data, setData] = useState<{
    templates: Template[];
    machines: string[];
  }>({ templates: [], machines: [] });
  const reload = () => resources().then(setData).catch(fail);
  useEffect(() => {
    void reload();
  }, []);
  return { ...data, reload };
}
function useDraft<T>(
  workspace: string,
  view: string,
  initial: T,
): [T, (value: T) => void] {
  const key = "nctool:plugin:" + workspace + ":" + view;
  const [value, setValue] = useState<T>(() => {
    try {
      const stored = localStorage.getItem(key);
      if (stored) return decodeDraft<T>(stored);
    } catch {}
    return initial;
  });
  function update(next: T) {
    setValue(next);
    try {
      localStorage.setItem(key, encodeDraft(next));
    } catch {}
  }
  return [value, update];
}
function specsFromSchema(schema: Schema | undefined): Data[] {
  const map: Record<string, string> = {
    number: "number",
    integer: "integer",
    string: "string",
    boolean: "bool",
    array: "list",
  };
  return Object.entries(schema?.properties || {})
    .filter(([, field]) => typeof field.type === "string" && map[field.type])
    .map(([name, field]) => ({
      name,
      kind: map[field.type as string],
      required: schema?.required?.includes(name) || false,
      description: field.description || field.title || name,
      ...(field.minimum !== undefined ? { min: field.minimum } : {}),
      ...(field.maximum !== undefined ? { max: field.maximum } : {}),
      ...(field.default !== undefined ? { default: field.default } : {}),
      ...(field["x-unit"] ? { unit: field["x-unit"] } : {}),
      ...(field.enum ? { options: field.enum } : {}),
    }));
}
function MachineSelect({
  machines,
  value,
  onChange,
  label = "目标机床",
  emptyLabel = "通用语法配置",
}: {
  machines: string[];
  value: string;
  onChange: (id: string) => void;
  label?: string;
  emptyLabel?: string;
}) {
  return (
    <label>
      {label}
      <select
        aria-label={label}
        value={value}
        onChange={(e) => onChange(e.target.value)}
      >
        <option value="">{emptyLabel}</option>
        {machines.map((id) => (
          <option value={id} key={id}>
            {id}
          </option>
        ))}
      </select>
    </label>
  );
}
function NcResult({
  run,
  cancel,
  stale = false,
}: {
  run?: Run;
  cancel?: () => void;
  stale?: boolean;
}) {
  if (!run)
    return (
      <Empty
        title="等待生成 NC 文件"
        description="选择模板、填写加工参数，生成结果会保存在执行记录中。"
      />
    );
  return (
    <>
      <RunDetail run={run} stale={stale} />
      {["queued", "running"].includes(run.status) && cancel && (
        <button className="button secondary" onClick={cancel}>
          取消任务
        </button>
      )}
    </>
  );
}
export function NcGenerate(props: ViewProps) {
  const { templates, machines } = useResources(props.fail);
  const [draft, setDraft] = useDraft<Data>(props.workspaceId, "nc", {
    template: "",
    source: "",
    params: {},
    machine_id: "",
    options: {
      line_numbers: false,
      line_number_step: 10,
      max_line_number: 9999,
    },
    mode: "template",
  });
  const [lastSignature, setLastSignature] = useState("");
  const [runId, setRunId] = useState<string | null>(null),
    [error, setError] = useState("");
  const selected =
    draft.mode === "template"
      ? templates.find((item) => item.id === draft.template)
      : undefined;
  function patch(values: Data) {
    setDraft({ ...draft, ...values });
    setError("");
  }
  async function execute() {
    try {
      if (draft.mode === "template" && !selected)
        throw new Error("请选择一个已保存的模板");
      if (draft.mode === "source" && !draft.source.trim())
        throw new Error("请填写 NC 模板源码");
      const input: Data = {
        ...(draft.mode === "template"
          ? { template: draft.template }
          : { source: draft.source }),
        params: draft.params,
        options: draft.options,
        ...(draft.machine_id ? { machine_id: draft.machine_id } : {}),
      };
      if (selected?.schema && !selected.asset.metadata?.nc?.specs)
        input.specs = specsFromSchema(selected.schema);
      const signature = encodeDraft(draft);
      const id = await props.start(
        "nc.generate",
        input,
        selected?.title || "NC 临时程序",
      );
      if (id) {
        setRunId(id);
        setLastSignature(signature);
      }
    } catch (e) {
      setError((e as Error).message);
    }
  }
  return (
    <div className="page-content nc-page">
      <PageTitle
        title="G 代码生成"
        description="选择加工模板和目标机床，填写参数并检查生成结果。"
      />
      <div className="nc-columns">
        <section className="panel">
          <div className="panel-header">
            <strong>程序输入</strong>
            <div className="pane-tabs">
              <button
                className={draft.mode === "template" ? "selected" : ""}
                onClick={() => patch({ mode: "template" })}
              >
                已保存模板
              </button>
              <button
                className={draft.mode === "source" ? "selected" : ""}
                onClick={() => patch({ mode: "source" })}
              >
                临时源码
              </button>
            </div>
          </div>
          <div className="panel-content">
            {draft.mode === "template" ? (
              <label>
                加工模板
                <select
                  aria-label="加工模板"
                  value={draft.template}
                  onChange={(e) => {
                    const template = templates.find(
                      (t) => t.id === e.target.value,
                    );
                    patch({
                      template: e.target.value,
                      params: clone(
                        template?.asset.defaults ||
                          defaultValue(template?.schema || {}) ||
                          {},
                      ),
                    });
                  }}
                >
                  <option value="">选择模板</option>
                  {templates.map((template) => (
                    <option key={template.id} value={template.id}>
                      {template.title}
                    </option>
                  ))}
                </select>
              </label>
            ) : (
              <div className="nc-source">
                <Editor
                  value={draft.source}
                  onChange={(source) => patch({ source })}
                  label="NC 模板源码"
                />
              </div>
            )}
            <MachineSelect
              machines={machines}
              value={draft.machine_id}
              onChange={(machine_id) => patch({ machine_id })}
            />
            <h3>加工参数</h3>
            <Form
              schema={selected?.schema || { type: "object" }}
              value={draft.params}
              onChange={(params) => patch({ params })}
            />
            <details className="advanced">
              <summary>行号与输出格式</summary>
              <Form
                schema={generationOptions}
                value={draft.options}
                onChange={(options) => patch({ options })}
              />
            </details>
            {error && <div className="inline-error">{error}</div>}
          </div>
          <div className="panel-footer">
            <button className="button primary" onClick={() => void execute()}>
              <Play size={15} />
              生成 NC 文件
            </button>
          </div>
        </section>
        <section className="panel">
          <NcResult
            run={runId ? props.runs[runId] : undefined}
            stale={!!runId && encodeDraft(draft) !== lastSignature}
            cancel={() =>
              api("runs/" + runId + "/cancel", {}).catch(props.fail)
            }
          />
        </section>
      </div>
    </div>
  );
}
const machineFields: Schema = {
  type: "object",
  properties: {
    line_number_prefix: { type: "string", title: "行号前缀" },
    line_number_digits: { type: "string", title: "行号位宽" },
    program_prefix: { type: "string", title: "程序号前缀" },
    max_spindle_rpm: { type: "string", title: "主轴转速上限" },
  },
};
export function Machines(props: ViewProps) {
  const { machines, reload } = useResources(props.fail);
  const [draft, setDraft] = useDraft<Data>(props.workspaceId, "machines", {
    name: "",
    machine: {
      id: "",
      vendor: "",
      model: "",
      config: {
        line_number_prefix: "N",
        line_number_digits: "4",
        program_prefix: "O",
      },
    },
  });
  const fingerprint = draft.expected || null;
  const loaded = draft.saved_name || null;
  const [message, setMessage] = useState("");
  async function open(id: string) {
    try {
      const result = await action("machine.read", { name: id });
      setDraft({
        name: id,
        machine: result.data.machine,
        saved_name: id,
        expected: result.data.fingerprint,
      });
      setMessage("");
    } catch (e) {
      props.fail(e);
    }
  }
  async function save() {
    try {
      if (!draft.name.trim()) throw new Error("请输入机床标识");
      await action("machine.save", {
        name: draft.name,
        machine: { ...draft.machine, id: draft.name },
        expected: loaded === draft.name ? fingerprint : null,
      });
      await reload();
      await open(draft.name);
      setMessage("机床配置已保存");
    } catch (e) {
      props.fail(e);
    }
  }
  return (
    <div className="page-content">
      <PageTitle
        title="机床配置"
        description="维护目标机床的编程约定与限制。修改已有配置需要匹配读取时的版本。"
        action={
          <button
            className="button secondary"
            onClick={() => {
              setDraft({
                name: "",
                machine: {
                  id: "",
                  vendor: "",
                  model: "",
                  config: {
                    line_number_prefix: "N",
                    line_number_digits: "4",
                    program_prefix: "O",
                  },
                },
              });
            }}
          >
            <Plus size={14} />
            新建机床
          </button>
        }
      />
      <div className="machine-columns">
        <section className="panel machine-list">
          {machines.map((id) => (
            <button
              key={id}
              className={loaded === id ? "selected" : ""}
              onClick={() => void open(id)}
            >
              <FileCode2 size={16} />
              {id}
            </button>
          ))}
          {!machines.length && (
            <Empty
              title="还没有机床配置"
              description="创建配置后，NC 和工序页面可直接选择它。"
            />
          )}
        </section>
        <section className="panel">
          <div className="panel-header">
            <strong>{loaded ? "编辑机床" : "新机床"}</strong>
          </div>
          <div className="panel-content">
            <div className="two-fields">
              <label>
                机床标识
                <input
                  aria-label="机床标识"
                  value={draft.name}
                  readOnly={!!loaded}
                  onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                />
              </label>
              <label>
                厂商
                <input
                  aria-label="机床厂商"
                  value={draft.machine.vendor}
                  onChange={(e) =>
                    setDraft({
                      ...draft,
                      machine: { ...draft.machine, vendor: e.target.value },
                    })
                  }
                />
              </label>
            </div>
            <label>
              型号
              <input
                aria-label="机床型号"
                value={draft.machine.model}
                onChange={(e) =>
                  setDraft({
                    ...draft,
                    machine: { ...draft.machine, model: e.target.value },
                  })
                }
              />
            </label>
            <h3>编程约定</h3>
            <Form
              schema={machineFields}
              value={draft.machine.config}
              onChange={(values) =>
                setDraft({
                  ...draft,
                  machine: {
                    ...draft.machine,
                    config: { ...draft.machine.config, ...values },
                  },
                })
              }
            />
            <details className="advanced">
              <summary>其他配置键</summary>
              <Form
                schema={{ type: "object" }}
                value={draft.machine.config}
                onChange={(config) =>
                  setDraft({ ...draft, machine: { ...draft.machine, config } })
                }
              />
            </details>
            {message && (
              <p role="status" className="success-note">
                {message}
              </p>
            )}
          </div>
          <div className="panel-footer">
            {loaded && (
              <button
                className="button secondary danger"
                onClick={async () => {
                  if (!confirm("删除这个机床配置？")) return;
                  try {
                    await action("machine.remove", {
                      name: loaded,
                      expected: fingerprint,
                    });
                    setDraft({ ...draft, saved_name: null, expected: null });
                    await reload();
                  } catch (e) {
                    props.fail(e);
                  }
                }}
              >
                <Trash2 size={14} />
                删除配置
              </button>
            )}
            <button className="button primary" onClick={() => void save()}>
              <Save size={14} />
              保存机床
            </button>
          </div>
        </section>
      </div>
    </div>
  );
}
interface Operation {
  key: string;
  template: string;
  source: string;
  mode: "template" | "source";
  params: Data;
  machine_id: string;
  title: string;
}
export function ProcessEditor(props: ViewProps) {
  const { templates, machines } = useResources(props.fail);
  const [draft, setDraft] = useDraft<Data>(props.workspaceId, "process", {
    name: "",
    params: {},
    machine_id: "",
    options: {
      line_numbers: true,
      line_number_step: 10,
      max_line_number: 9999,
    },
    ops: [],
  });
  const [lastSignature, setLastSignature] = useState("");
  const [runId, setRunId] = useState<string | null>(null),
    [error, setError] = useState("");
  const run = runId ? props.runs[runId] : undefined;
  function patch(value: Data) {
    setDraft({ ...draft, ...value });
    setError("");
  }
  function edit(index: number, value: Partial<Operation>) {
    patch({
      ops: draft.ops.map((op: Operation, i: number) =>
        i === index ? { ...op, ...value } : op,
      ),
    });
  }
  function move(index: number, delta: number) {
    const operations = [...draft.ops];
    const target = index + delta;
    if (target < 0 || target >= operations.length) return;
    [operations[index], operations[target]] = [
      operations[target],
      operations[index],
    ];
    patch({ ops: operations });
  }
  async function generate() {
    try {
      if (!draft.ops.length) throw new Error("请至少添加一道工序");
      const ops = draft.ops.map((op: Operation, index: number) => {
        if (op.mode === "template" && !op.template)
          throw new Error(`第 ${index + 1} 道工序尚未选择模板`);
        if (op.mode === "source" && !op.source.trim())
          throw new Error(`第 ${index + 1} 道工序没有源码`);
        const template =
          op.mode === "template"
            ? templates.find((t) => t.id === op.template)
            : undefined;
        return {
          ...(op.mode === "template"
            ? { template: op.template }
            : { source: op.source }),
          params: op.params,
          ...(op.machine_id ? { machine_id: op.machine_id } : {}),
          ...(template?.schema && !template.asset.metadata?.nc?.specs
            ? { specs: specsFromSchema(template.schema) }
            : {}),
        };
      });
      const signature = encodeDraft(draft);
      const id = await props.start(
        "process.generate",
        {
          name: draft.name || "program",
          params: draft.params,
          options: draft.options,
          ops,
          ...(draft.machine_id ? { machine_id: draft.machine_id } : {}),
        },
        draft.name || "多工序程序",
      );
      if (id) {
        setRunId(id);
        setLastSignature(signature);
      }
    } catch (e) {
      setError((e as Error).message);
    }
  }
  const failed = (index: number) =>
    run?.error?.diagnostics?.some(
      (d: Diagnostic) =>
        d.level === "error" && d.path?.startsWith(`ops[${index}]`),
    );
  return (
    <div className="page-content process-page">
      <PageTitle
        title="多工序程序"
        description="公共参数可以由工序覆盖。按列表顺序生成并连续编号，全部工序成功后交付完整文件。"
      />
      <div className="nc-columns">
        <section className="panel">
          <div className="panel-content">
            <label>
              程序名称
              <input
                aria-label="程序名称"
                value={draft.name}
                onChange={(e) => patch({ name: e.target.value })}
                placeholder="例如：轴类零件程序"
              />
            </label>
            <MachineSelect
              machines={machines}
              value={draft.machine_id}
              onChange={(machine_id) => patch({ machine_id })}
              label="默认机床"
            />
            <details className="advanced">
              <summary>公共参数与输出选项</summary>
              <Form
                schema={{ type: "object" }}
                value={draft.params}
                onChange={(params) => patch({ params })}
              />
              <Form
                schema={generationOptions}
                value={draft.options}
                onChange={(options) => patch({ options })}
              />
            </details>
            <div className="operations-heading">
              <h3>工序列表 · {draft.ops.length} 道</h3>
              <button
                className="button secondary"
                onClick={() =>
                  patch({
                    ops: [
                      ...draft.ops,
                      {
                        key: crypto.randomUUID(),
                        title: "工序 " + (draft.ops.length + 1),
                        template: "",
                        source: "",
                        mode: "template",
                        params: {},
                        machine_id: "",
                      },
                    ],
                  })
                }
              >
                <Plus size={14} />
                添加工序
              </button>
            </div>
            {draft.ops.map((op: Operation, index: number) => (
              <details
                key={op.key}
                className={"operation-card " + (failed(index) ? "failed" : "")}
                open
              >
                <summary>
                  <span className="operation-number">{index + 1}</span>
                  <strong>{op.title}</strong>
                  {failed(index) && <AlertTriangle size={14} />}
                </summary>
                <div className="operation-body">
                  <div className="operation-tools">
                    <button
                      className="icon-button"
                      aria-label={"上移工序" + (index + 1)}
                      disabled={index === 0}
                      onClick={() => move(index, -1)}
                    >
                      <ArrowUp size={14} />
                    </button>
                    <button
                      className="icon-button"
                      aria-label={"下移工序" + (index + 1)}
                      disabled={index === draft.ops.length - 1}
                      onClick={() => move(index, 1)}
                    >
                      <ArrowDown size={14} />
                    </button>
                    <button
                      className="icon-button"
                      aria-label={"复制工序" + (index + 1)}
                      onClick={() => {
                        const operations = [...draft.ops];
                        operations.splice(index + 1, 0, {
                          ...cloneDraft(op),
                          key: crypto.randomUUID(),
                          title: op.title + " 副本",
                        });
                        patch({ ops: operations });
                      }}
                    >
                      <Copy size={14} />
                    </button>
                    <button
                      className="icon-button danger"
                      aria-label={"删除工序" + (index + 1)}
                      onClick={() =>
                        patch({
                          ops: draft.ops.filter(
                            (_: Operation, i: number) => i !== index,
                          ),
                        })
                      }
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                  <label>
                    工序名称
                    <input
                      value={op.title}
                      onChange={(e) => edit(index, { title: e.target.value })}
                    />
                  </label>
                  <div className="pane-tabs">
                    <button
                      className={op.mode === "template" ? "selected" : ""}
                      onClick={() => edit(index, { mode: "template" })}
                    >
                      模板
                    </button>
                    <button
                      className={op.mode === "source" ? "selected" : ""}
                      onClick={() => edit(index, { mode: "source" })}
                    >
                      临时源码
                    </button>
                  </div>
                  {op.mode === "template" ? (
                    <label>
                      工序模板
                      <select
                        aria-label={"工序模板" + (index + 1)}
                        value={op.template}
                        onChange={(e) => {
                          const template = templates.find(
                            (t) => t.id === e.target.value,
                          );
                          edit(index, {
                            template: e.target.value,
                            params: clone(template?.asset.defaults || {}),
                          });
                        }}
                      >
                        <option value="">选择模板</option>
                        {templates.map((t) => (
                          <option key={t.id} value={t.id}>
                            {t.title}
                          </option>
                        ))}
                      </select>
                    </label>
                  ) : (
                    <div className="nc-source">
                      <Editor
                        editorKey={op.key}
                        label={"工序源码" + (index + 1)}
                        value={op.source}
                        onChange={(source) => edit(index, { source })}
                      />
                    </div>
                  )}
                  <MachineSelect
                    label="工序机床（留空继承默认机床）"
                    emptyLabel="继承默认机床"
                    machines={machines}
                    value={op.machine_id}
                    onChange={(machine_id) => edit(index, { machine_id })}
                  />
                  <Form
                    schema={
                      (op.mode === "template"
                        ? templates.find((t) => t.id === op.template)?.schema
                        : undefined) || { type: "object" }
                    }
                    value={op.params}
                    onChange={(params) => edit(index, { params })}
                  />
                  {failed(index) && (
                    <div className="inline-error">
                      {run?.error?.diagnostics
                        ?.filter((d: Diagnostic) =>
                          d.path?.startsWith(`ops[${index}]`),
                        )
                        .map((d: Diagnostic, i: number) => (
                          <p key={i}>{d.message}</p>
                        ))}
                    </div>
                  )}
                </div>
              </details>
            ))}
            {!draft.ops.length && (
              <Empty
                title="添加第一道工序"
                description="模板与参数可以逐道设置，公用参数只需填写一次。"
              />
            )}
            {error && <div className="inline-error">{error}</div>}
          </div>
          <div className="panel-footer">
            <button className="button primary" onClick={() => void generate()}>
              <Play size={15} />
              生成完整程序
            </button>
          </div>
        </section>
        <section className="panel">
          <NcResult
            run={run}
            stale={!!runId && encodeDraft(draft) !== lastSignature}
            cancel={() =>
              api("runs/" + runId + "/cancel", {}).catch(props.fail)
            }
          />
        </section>
      </div>
    </div>
  );
}
export const ncViews: Record<string, import("react").ComponentType<ViewProps>> =
  {
    "nc.generate": NcGenerate,
    "nc.machines": Machines,
    "nc.process": ProcessEditor,
  };
