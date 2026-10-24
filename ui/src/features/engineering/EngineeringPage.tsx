import { useEffect, useMemo, useRef, useState } from "react";
import { useParams } from "react-router-dom";
import {
  Alert,
  App,
  Button,
  Card,
  Checkbox,
  Empty,
  Form,
  Input,
  Modal,
  Select,
  Space,
  Table,
  Tabs,
  Typography,
} from "antd";
import { useQuery } from "@tanstack/react-query";
import { useSession, useStartRun, useWorkspace } from "../../app/context";
import { action, api, queryClient, runQuery } from "../../lib/api";
import {
  decode,
  encode,
  errorText,
  object,
  saveFile,
  type Data,
} from "../../lib/json";
import { SchemaFields } from "../../components/SchemaFields";
import { CodeEditor } from "../../components/LazyCodeEditor";
import { ResultPanel } from "../../components/ResultPanel";
import { SceneView } from "./SceneView";
import { readScene, type Scene } from "./scene";

interface ProjectDraft {
  format_version: number;
  raw: string;
  invalid: Record<string, string>;
  formats: string[];
  template: string;
}
const clone = (value: Data) => decode(encode(value));
export default function EngineeringPage() {
  const { plugin, viewId } = useParams(),
    { id, capabilities, store } = useWorkspace(),
    { message } = App.useApp(),
    start = useStartRun();
  const view = capabilities.workbenches.find(
    (v) => v.plugin === plugin && v.id === viewId,
  );
  const key = `engineering:${plugin}:${viewId}`,
    saved = useSession((s) => s.auxiliary[key]);
  const fallback = encode({
    format_version: 1,
    raw: encode(view?.example || {}, true),
    invalid: {},
    formats: view?.formats || [],
    template: "",
  });
  const raw = saved?.raw ?? fallback;
  let draft: ProjectDraft = {
      format_version: 1,
      raw: "{}",
      invalid: {},
      formats: [],
      template: "",
    },
    project: Data,
    problem = "";
  try {
    draft = decode(raw);
    project = decode(draft.raw);
    if (!object(project)) throw Error("项目必须为对象");
    if (
      view?.tables.some(
        (table) =>
          project[table.field] !== undefined &&
          (!Array.isArray(project[table.field]) ||
            project[table.field].some((row: Data) => !object(row))),
      )
    )
      throw Error("记录表必须是对象数组，请在项目 JSON 中修正");
  } catch (error) {
    problem = errorText(error);
  }
  const [selected, setSelected] = useState(""),
    [tab, setTab] = useState("form"),
    [busy, setBusy] = useState(false),
    [preview, setPreview] = useState<{ signature: string; data: Data }>(),
    [previewError, setPreviewError] = useState("");
  const [pages, setPages] = useState<Record<string, number>>({});
  const [saving, setSaving] = useState(false);
  const [templateSaving, setTemplateSaving] = useState(false);
  const history = useRef<{ past: string[]; future: string[] }>({
      past: [],
      future: [],
    }),
    file = useRef<HTMLInputElement>(null);
  const [templateEditor, setTemplateEditor] = useState<{
    name: string;
    original: string;
    asset: Data;
    expected: string | null;
  }>();
  const current = useQuery(runQuery(id, saved?.runId));
  const url = `engineering/${encodeURIComponent(plugin || "")}/${encodeURIComponent(viewId || "")}/`;
  const documents = useQuery({
    queryKey: [id, key, "documents"],
    enabled: !!view,
    queryFn: () => api(url + "list", {}),
  });
  const projectId = saved?.documentId || "project";
  const setProjectId = (documentId: string) => {
    const old = store.getState().auxiliary[key];
    store.getState().setAuxiliary(key, {
      ...old,
      raw: old?.raw ?? fallback,
      saved: old?.saved ?? "",
      expected: null,
      documentId,
    });
  };
  const templates = useQuery({
    queryKey: [id, "engineering-templates"],
    enabled: !!view,
    queryFn: () => action("template.catalog"),
    refetchInterval: 3000,
  });
  const commit = (update: (draft: ProjectDraft) => void, record = true) => {
    const old = store.getState().auxiliary[key],
      before = old?.raw ?? fallback,
      next = decode(before);
    update(next);
    const after = encode(next);
    if (after === before) return;
    if (record) {
      history.current.past.push(before);
      history.current.past = history.current.past.slice(-100);
      history.current.future = [];
    }
    store.getState().setAuxiliary(key, {
      ...old,
      raw: after,
      expected: old?.expected ?? null,
      saved: old?.saved ?? "",
    });
  };
  const updateProject = (update: (value: Data) => void) =>
    commit((d) => {
      const value = decode(d.raw);
      update(value);
      d.raw = encode(value, true);
    });
  const invalid = (path: (string | number)[], value: string | undefined) =>
    commit((d) => {
      const pointer =
        "/" +
        path
          .map((x) => String(x).replace(/~/g, "~0").replace(/\//g, "~1"))
          .join("/");
      if (value === undefined) delete d.invalid[pointer];
      else d.invalid[pointer] = value;
    });
  const undo = (forward: boolean) => {
    const source = forward ? history.current.future : history.current.past,
      target = forward ? history.current.past : history.current.future,
      next = source.pop();
    if (next) {
      target.push(raw);
      store.getState().setAuxiliary(key, {
        ...saved,
        raw: next,
        expected: saved?.expected ?? null,
        saved: saved?.saved ?? "",
      });
    }
  };
  useEffect(() => {
    setSelected("");
    setPreview(undefined);
    history.current = { past: [], future: [] };
  }, [key]);
  useEffect(() => {
    if (!view || problem || Object.keys(draft.invalid).length) {
      setPreviewError(problem || "请修正未完成数值");
      return;
    }
    let alive = true,
      started = false;
    const requestId = "preview-" + crypto.randomUUID(),
      controller = new AbortController();
    const timer = setTimeout(() => {
      started = true;
      api(
        url + "preview",
        { input: { project }, request_id: requestId },
        controller.signal,
      )
        .then((result) => {
          if (alive) {
            setPreview({ signature: raw, data: result.data });
            setPreviewError("");
          }
        })
        .catch((error) => {
          if (alive) setPreviewError(errorText(error));
        });
    }, 300);
    return () => {
      alive = false;
      clearTimeout(timer);
      controller.abort();
      if (started) void api("cancel/" + requestId, {}).catch(() => {});
    };
  }, [raw, key, view, problem]);
  const frozen = current.data?.result?.data?.replay_input?.template_snapshot;
  const dependencies = useQuery({
    queryKey: [id, key, "templates", saved?.runId, templates.data],
    enabled: !!frozen && current.data?.status === "succeeded",
    refetchInterval: 3000,
    queryFn: async () => {
      const input = frozen.asset
        ? { template: frozen.template, include_snapshot: true }
        : { source: frozen.templates[frozen.template], include_snapshot: true };
      const latest = await action("template.inspect", input);
      return encode(latest.data.snapshot) === encode(frozen);
    },
  });
  const sceneInput =
    preview?.signature === raw ? preview.data.scene : undefined;
  const parsedScene = useMemo(() => {
    try {
      return {
        scene: sceneInput ? readScene(sceneInput) : undefined,
        error: "",
      };
    } catch (error) {
      return { scene: undefined, error: errorText(error) };
    }
  }, [sceneInput]);
  if (!view) return <Empty description="工作台未安装或未启用" />;
  const activePreview = preview?.signature === raw ? preview.data : undefined;
  const scene: Scene | undefined = parsedScene.scene,
    sceneProblem = parsedScene.error;
  const blocked =
    problem ||
    Object.keys(draft.invalid).length > 0 ||
    !activePreview ||
    activePreview.result?.valid === false;
  const restore = (
    value: Data,
    fingerprint: string | null,
    documentId = projectId,
  ) => {
    if (
      Number(value.format_version) !== 1 ||
      typeof value.raw !== "string" ||
      !object(value.invalid) ||
      !Array.isArray(value.formats) ||
      typeof value.template !== "string"
    )
      throw Error("不支持的工作台项目格式");
    const text = encode(value);
    store.getState().setAuxiliary(key, {
      raw: text,
      expected: fingerprint,
      saved: fingerprint ? text : "",
      documentId,
    });
    history.current = { past: [], future: [] };
    setSelected("");
  };
  const confirmReplace = (callback: () => void) => {
    const latest = store.getState().auxiliary[key];
    if ((latest?.raw ?? fallback) !== latest?.saved)
      Modal.confirm({
        title: "替换当前草稿？",
        content: "当前未保存内容将被替换，可以先保存或导出项目。",
        onOk: callback,
      });
    else callback();
  };
  const saveProject = async (createOnly: boolean) => {
    if (saving) return;
    setSaving(true);
    try {
      const result = await api(url + "save", {
        id: projectId,
        document: decode(raw),
        expected: createOnly ? null : (saved?.expected ?? null),
      });
      const latest = store.getState().auxiliary[key];
      if ((latest?.documentId || "project") === projectId)
        store
          .getState()
          .setAuxiliary(key, {
            ...latest,
            raw: latest?.raw ?? raw,
            expected: result.fingerprint,
            saved: raw,
          });
      await queryClient.invalidateQueries({ queryKey: [id, key, "documents"] });
      message.success(createOnly ? "已另存项目" : "项目已保存");
    } catch (error) {
      message.error(errorText(error));
    } finally {
      setSaving(false);
    }
  };
  const select = (entity: string) => {
    setSelected(entity);
    for (const table of view.tables) {
      const index = project?.[table.field]?.findIndex(
        (row: Data) => row.id === entity,
      );
      if (index >= 0)
        setPages((p) => ({ ...p, [table.field]: Math.floor(index / 20) + 1 }));
    }
    requestAnimationFrame(() => {
      const elements = document.querySelectorAll<HTMLElement>(
        "[data-engineering-entity]",
      );
      for (const element of elements)
        if (element.dataset.engineeringEntity === entity)
          element.scrollIntoView({ block: "nearest", behavior: "smooth" });
    });
  };
  const formSchema = {
    ...view.schema,
    properties: Object.fromEntries(
      Object.entries(view.schema.properties || {}).filter(
        ([name]) => !view.tables.some((t) => t.field === name),
      ),
    ),
  };
  const form = (
    <>
      <Form component="div" layout="vertical">
        <SchemaFields
          schema={formSchema}
          value={Object.fromEntries(
            Object.entries(project || {}).filter(
              ([field]) => !view.tables.some((t) => t.field === field),
            ),
          )}
          invalid={draft.invalid}
          onInvalid={invalid}
          onChange={(value) => updateProject((p) => Object.assign(p, value))}
        />
      </Form>
      {view.tables.map((table) => {
        const rows = Array.isArray(project?.[table.field])
            ? project[table.field].filter(object)
            : [],
          spec = view.schema.properties[table.field].items;
        const mutate = (fn: (rows: Data[]) => void) =>
          updateProject((p) => {
            if (!Array.isArray(p[table.field])) p[table.field] = [];
            fn(p[table.field]);
          });
        return (
          <Card
            key={table.field}
            size="small"
            title={table.title}
            extra={
              <Button
                onClick={() =>
                  mutate((rows) => {
                    const row = clone(table.row);
                    if (Object.hasOwn(row, "id"))
                      row.id =
                        "r" +
                        crypto.randomUUID().replace(/-/g, "").slice(0, 12);
                    rows.push(row);
                  })
                }
              >
                添加记录
              </Button>
            }
          >
            <Table
              pagination={{
                pageSize: 20,
                showSizeChanger: false,
                current: Math.min(
                  pages[table.field] || 1,
                  Math.max(1, Math.ceil(rows.length / 20)),
                ),
                onChange: (page) =>
                  setPages((p) => ({ ...p, [table.field]: page })),
              }}
              size="small"
              scroll={{ x: "max-content" }}
              rowKey={(row) => String(row.__engineeringIndex)}
              dataSource={rows.map((row: Data, index: number) => ({
                ...row,
                __engineeringIndex: index,
              }))}
              onRow={(row) =>
                ({
                  onClick: () => setSelected(row.id || ""),
                  onFocus: () => setSelected(row.id || ""),
                  tabIndex: 0,
                  "data-engineering-entity": row.id || "",
                }) as Data
              }
              rowClassName={(row) =>
                row.id === selected ? "engineering-selected" : ""
              }
              columns={[
                ...Object.entries(spec.properties || {}).map(
                  ([field, schema]) => ({
                    key: field,
                    title: (schema as Data).title || field,
                    width: field === "corner" ? 220 : 140,
                    render: (_value: Data, _row: Data) => {
                      const index = _row.__engineeringIndex;
                      return (
                        <SchemaFields
                          schema={schema as Data}
                          value={rows[index][field]}
                          path={[table.field, index, field]}
                          invalid={draft.invalid}
                          onInvalid={invalid}
                          onChange={(value) =>
                            mutate((rows) => {
                              rows[index][field] = value;
                            })
                          }
                        />
                      );
                    },
                  }),
                ),
                {
                  key: "operations",
                  title: "操作",
                  width: 200,
                  render: (_value: Data, _row: Data) => {
                    const index = _row.__engineeringIndex;
                    return (
                      <Space>
                        <Button
                          aria-label="上移记录"
                          disabled={index === 0}
                          onClick={() =>
                            mutate((rows) => {
                              [rows[index - 1], rows[index]] = [
                                rows[index],
                                rows[index - 1],
                              ];
                            })
                          }
                        >
                          ↑
                        </Button>
                        <Button
                          aria-label="下移记录"
                          disabled={index === rows.length - 1}
                          onClick={() =>
                            mutate((rows) => {
                              [rows[index], rows[index + 1]] = [
                                rows[index + 1],
                                rows[index],
                              ];
                            })
                          }
                        >
                          ↓
                        </Button>
                        <Button
                          onClick={() =>
                            mutate((rows) => {
                              const row = clone(rows[index]);
                              if (row.id)
                                row.id =
                                  "r" +
                                  crypto
                                    .randomUUID()
                                    .replace(/-/g, "")
                                    .slice(0, 12);
                              rows.splice(index + 1, 0, row);
                            })
                          }
                        >
                          复制
                        </Button>
                        <Button
                          danger
                          onClick={() =>
                            mutate((rows) => {
                              rows.splice(index, 1);
                            })
                          }
                        >
                          删除
                        </Button>
                      </Space>
                    );
                  },
                },
              ]}
            />
          </Card>
        );
      })}
    </>
  );
  return (
    <div className="engineering-page">
      <Typography.Title level={2}>{view.title}</Typography.Title>
      <Space wrap>
        <Input
          aria-label="项目标识"
          disabled={saving}
          style={{ width: 180 }}
          value={projectId}
          onChange={(e) => setProjectId(e.target.value)}
        />
        <Select<string>
          disabled={saving}
          aria-label="工作区项目"
          placeholder="打开工作区项目"
          style={{ width: 190 }}
          value={undefined}
          options={(documents.data?.items || []).map((name: string) => ({
            value: name,
            label: name,
          }))}
          onChange={(name) =>
            confirmReplace(() => {
              const before = store.getState().auxiliary[key]?.raw ?? fallback;
              void api(url + "read", { id: name })
                .then((r) => {
                  if (
                    (store.getState().auxiliary[key]?.raw ?? fallback) !==
                    before
                  ) {
                    message.warning("读取期间草稿已修改，未覆盖；请重新打开");
                    return;
                  }
                  restore(r.document, r.fingerprint, name);
                })
                .catch((e) => message.error(errorText(e)));
            })
          }
        />
        <Button loading={saving} onClick={() => void saveProject(false)}>
          保存项目
        </Button>
        <Button disabled={saving} onClick={() => void saveProject(true)}>
          另存项目
        </Button>
        <Button
          danger
          disabled={!saved?.expected || saving}
          onClick={() =>
            Modal.confirm({
              title: "删除当前已保存项目？",
              onOk: async () => {
                await api(url + "remove", {
                  id: projectId,
                  expected: saved?.expected,
                });
                const latest = store.getState().auxiliary[key];
                if ((latest?.documentId || "project") === projectId)
                  store
                    .getState()
                    .setAuxiliary(key, {
                      ...latest,
                      raw: latest?.raw ?? raw,
                      expected: null,
                      saved: "",
                    });
                await queryClient.invalidateQueries({
                  queryKey: [id, key, "documents"],
                });
              },
            })
          }
        >
          删除项目
        </Button>
        <Button
          onClick={() =>
            confirmReplace(() =>
              restore(
                {
                  format_version: 1,
                  raw: encode(view.example, true),
                  invalid: {},
                  formats: view.formats,
                  template: "",
                },
                null,
              ),
            )
          }
        >
          新建示例
        </Button>
        <Button onClick={() => file.current?.click()}>导入项目文件</Button>
        <Button
          onClick={() =>
            saveFile(
              projectId + ".json",
              encode(decode(raw), true),
              "application/json",
            )
          }
        >
          导出项目文件
        </Button>
        <input
          ref={file}
          type="file"
          accept=".json,application/json"
          hidden
          onChange={(e) => {
            const uploaded = e.target.files?.[0];
            e.target.value = "";
            if (!uploaded) return;
            if (uploaded.size > 1048576) {
              message.error("项目文件超过 1 MiB");
              return;
            }
            void uploaded
              .text()
              .then((text) => {
                const value = decode(text);
                confirmReplace(() => {
                  try {
                    restore(value, null);
                  } catch (e) {
                    message.error(errorText(e));
                  }
                });
              })
              .catch((e) => message.error(errorText(e)));
          }}
        />
        <Button
          onClick={() => undo(false)}
          disabled={!history.current.past.length}
        >
          撤销
        </Button>
        <Button
          onClick={() => undo(true)}
          disabled={!history.current.future.length}
        >
          重做
        </Button>
      </Space>
      {(problem || previewError || sceneProblem) && (
        <Alert type="error" title={problem || previewError || sceneProblem} />
      )}
      {documents.isError && (
        <Alert
          type="error"
          title="项目目录读取失败"
          description={errorText(documents.error)}
        />
      )}
      <div className="engineering-grid">
        <Card title="项目参数">
          <Tabs
            activeKey={tab}
            onChange={setTab}
            items={[
              {
                key: "form",
                label: "表格与参数",
                children: !problem ? (
                  form
                ) : (
                  <Empty description="先在 JSON 中修正项目" />
                ),
              },
              {
                key: "json",
                label: "项目 JSON",
                children: (
                  <CodeEditor
                    value={draft.raw}
                    language="json"
                    label="工程项目 JSON"
                    onChange={(value) =>
                      commit((d) => {
                        d.raw = value;
                        d.invalid = {};
                      })
                    }
                  />
                ),
              },
            ]}
          />
        </Card>
        <div>
          <Card title="几何预览">
            {scene ? (
              <SceneView scene={scene} selected={selected} onSelect={select} />
            ) : (
              <Empty description="正在分析当前项目" />
            )}
            <Typography.Paragraph type="secondary">
              预览仅反映当前输入；正式生成会重新计算并校验。
            </Typography.Paragraph>
            {(activePreview?.result?.diagnostics || []).map(
              (issue: Data, index: number) => (
                <Alert
                  key={index}
                  type="error"
                  title={issue.message}
                  action={
                    <Button
                      onClick={() => {
                        const match = String(issue.path).match(
                          /\/features\/(\d+)/,
                        );
                        if (match)
                          select(project.features[Number(match[1])]?.id || "");
                      }}
                    >
                      定位
                    </Button>
                  }
                />
              ),
            )}
          </Card>
          <Card title="生成与导出">
            <Space wrap>
              <Select
                aria-label="生成模板"
                style={{ minWidth: 200 }}
                value={draft.template}
                options={[
                  { value: "", label: "插件默认模板" },
                  ...(templates.data?.data?.items || []).map((t: Data) => ({
                    value: t.id,
                    label: t.title || t.id,
                  })),
                ]}
                onChange={(template) =>
                  commit((d) => {
                    d.template = template;
                  })
                }
              />
              <Checkbox.Group
                options={view.formats.map((format) => ({
                  label: format.toUpperCase(),
                  value: format,
                }))}
                value={draft.formats}
                onChange={(formats) =>
                  commit((d) => {
                    d.formats = formats as string[];
                  })
                }
              />
              {view.template_action && (
                <Button
                  onClick={async () => {
                    try {
                      const loaded = await action(
                        draft.template
                          ? "template.read"
                          : view.template_action!,
                        draft.template ? { name: draft.template } : {},
                      );
                      setTemplateEditor({
                        name: draft.template || "postprocessor-copy",
                        original: draft.template,
                        asset: loaded.data.asset,
                        expected: loaded.data.fingerprint || null,
                      });
                    } catch (e) {
                      message.error(errorText(e));
                    }
                  }}
                >
                  编辑模板副本
                </Button>
              )}
              <Button
                type="primary"
                loading={busy}
                disabled={!!blocked || !draft.formats.length}
                onClick={async () => {
                  setBusy(true);
                  const captured = raw;
                  try {
                    let input: Data = {
                      project,
                      formats: draft.formats,
                      ...(draft.template ? { template: draft.template } : {}),
                    };
                    if (view.prepare_action)
                      input = (await action(view.prepare_action, input)).data
                        .input;
                    const runId = await start.mutateAsync({
                      action: view.generate_action,
                      input,
                      title: view.title,
                    });
                    const latest = store.getState().auxiliary[key];
                    store.getState().setAuxiliary(key, {
                      ...latest,
                      raw: latest?.raw ?? captured,
                      expected: latest?.expected ?? null,
                      saved: latest?.saved ?? "",
                      runId,
                      runSignature: captured,
                    });
                  } catch (e) {
                    message.error(errorText(e));
                  } finally {
                    setBusy(false);
                  }
                }}
              >
                正式生成
              </Button>
            </Space>
            <ResultPanel
              runId={saved?.runId}
              forceReason={
                saved?.runSignature !== raw
                  ? "输入已修改，请重新生成"
                  : frozen && dependencies.data !== true
                    ? "正在检查模板，或模板已变更，请重新生成"
                    : undefined
              }
            />
          </Card>
        </div>
      </div>
      <Modal
        open={!!templateEditor}
        confirmLoading={templateSaving}
        maskClosable={!templateSaving}
        keyboard={!templateSaving}
        closable={!templateSaving}
        title="后处理模板副本"
        width={900}
        destroyOnHidden
        onCancel={() => {
          if (!templateSaving) setTemplateEditor(undefined);
        }}
        onOk={async () => {
          if (!templateEditor || templateSaving) return;
          setTemplateSaving(true);
          try {
            await action("template.save", {
              name: templateEditor.name,
              asset: templateEditor.asset,
              expected:
                templateEditor.name === templateEditor.original
                  ? templateEditor.expected
                  : null,
            });
            commit((d) => {
              d.template = templateEditor.name;
            });
            const active = store.getState().auxiliary[key];
            if (active?.runId)
              store
                .getState()
                .setAuxiliary(key, { ...active, runSignature: undefined });
            setTemplateEditor(undefined);
            await queryClient.invalidateQueries({
              queryKey: [id, "engineering-templates"],
            });
            message.success("模板已保存；正式生成仍会检查输出语义");
          } catch (e) {
            message.error(errorText(e));
          } finally {
            setTemplateSaving(false);
          }
        }}
      >
        {templateEditor && (
          <Space direction="vertical" style={{ width: "100%" }}>
            <Alert
              type="info"
              title="允许修改注释、行号和布局；运动、进给及模态必须保持与校验结果一致。"
            />
            <Input
              aria-label="模板副本标识"
              disabled={templateSaving}
              value={templateEditor.name}
              onChange={(e) =>
                setTemplateEditor({ ...templateEditor, name: e.target.value })
              }
            />
            <CodeEditor
              label="后处理 Jinja 模板"
              language="jinja"
              readOnly={templateSaving}
              value={templateEditor.asset.source}
              onChange={(source) =>
                setTemplateEditor({
                  ...templateEditor,
                  asset: { ...templateEditor.asset, source },
                })
              }
            />
          </Space>
        )}
      </Modal>
    </div>
  );
}
