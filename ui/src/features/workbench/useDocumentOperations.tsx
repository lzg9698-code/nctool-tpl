import { App, Button, Input, Space } from "antd";
import { useRef } from "react";
import { useMutation } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { action, ApiError, queryClient, api, running } from "../../lib/api";
import { useWorkspace, useStartRun } from "../../app/context";
import {
  assetOf,
  assetSignature,
  makeDraft,
  signature,
  type Draft,
} from "../../state/session";
import { decode, encode, errorText, type Data } from "../../lib/json";
import { inputFor, selectAction } from "../../lib/execution";
import { DiffDisplay } from "../../components/ResultPanel";

const submitting = new WeakMap<object, Set<string>>();
export function useDocumentOperations() {
  const { id, store, capabilities } = useWorkspace(),
    { message, modal } = App.useApp(),
    navigate = useNavigate();
  const start = useStartRun(),
    saving = useRef(new Set<string>());
  const refresh = () =>
    queryClient.invalidateQueries({ queryKey: [id, "templates"] });
  const go = (draft: Draft) => {
    store.getState().add(draft);
    navigate("/workbench/" + draft.key);
  };
  const copy = (draft: Draft) =>
    go({
      ...draft,
      key: crypto.randomUUID(),
      id: null,
      fingerprint: null,
      saved: "",
      title: draft.title + " 副本",
      runId: undefined,
      runSignature: undefined,
    });
  const save = useMutation({
    mutationFn: async (draft: Draft) => {
      if (saving.current.has(draft.key)) return;
      saving.current.add(draft.key);
      try {
        if (!draft.title.trim()) throw new Error("请填写模板名称");
        const name = draft.id || "tpl-" + crypto.randomUUID(),
          asset = assetOf(draft);
        const response = await action("template.save", {
          name,
          asset,
          expected: draft.fingerprint,
        });
        store
          .getState()
          .patch(draft.key, {
            id: name,
            fingerprint: response.data.fingerprint,
            saved: assetSignature(draft),
          });
        await refresh();
        message.success("模板已保存");
      } catch (error) {
        if (error instanceof ApiError && error.detail.code === "conflict") {
          let disk: Data;
          try {
            disk = draft.id
              ? (await action("template.read", { name: draft.id })).data
              : undefined;
          } catch {
            /* a deleted disk asset still permits saving a copy */
          }
          const dialog = modal.info({
            title: "模板已被其他编辑修改",
            width: 720,
            content: (
              <Space vertical style={{ width: "100%" }}>
                <p>
                  当前草稿保留。比较后可加载磁盘版本，或把当前草稿另存为副本。
                </p>
                <DiffDisplay
                  before={disk ? encode(disk.asset, true) : "磁盘资产不存在"}
                  after={encode(
                    assetOf(
                      store
                        .getState()
                        .documents.find((doc) => doc.key === draft.key) ||
                        draft,
                    ),
                    true,
                  )}
                />
                <Space wrap size={[12, 8]}>
                  <Button
                    onClick={() => {
                      const current = store
                        .getState()
                        .documents.find((doc) => doc.key === draft.key);
                      if (current) copy(current);
                      dialog.destroy();
                    }}
                  >
                    另存副本
                  </Button>
                  <Button
                    disabled={!disk}
                    onClick={() =>
                      modal.confirm({
                        title: "丢弃当前草稿并加载磁盘版本？",
                        okText: "加载磁盘版本",
                        cancelText: "继续编辑",
                        onOk: () => {
                          const loaded = makeDraft(
                            disk.asset,
                            draft.id,
                            disk.fingerprint,
                          );
                          store
                            .getState()
                            .patch(draft.key, { ...loaded, key: draft.key });
                          dialog.destroy();
                        },
                      })
                    }
                  >
                    加载磁盘版本
                  </Button>
                </Space>
              </Space>
            ),
            okText: "继续编辑",
          });
        } else throw error;
      } finally {
        saving.current.delete(draft.key);
      }
    },
    onError: (error) => message.error(errorText(error)),
  });
  const generate = useMutation({
    mutationFn: async (draft: Draft) => {
      const locks = submitting.get(store) || new Set<string>();
      submitting.set(store, locks);
      const previous: Data = draft.runId
        ? queryClient.getQueryData([id, "run", draft.runId])
        : undefined;
      if (locks.has(draft.key) || running(previous?.status))
        throw new Error("此文档已有任务正在提交或执行");
      locks.add(draft.key);
      try {
        if (!draft.source.trim()) throw new Error("请先编写模板源码");
        const entry = selectAction(draft, capabilities),
          input = inputFor(draft, entry),
          captured = signature(draft);
        const runId = await start.mutateAsync({
          action: entry.id,
          input,
          title: draft.title,
        });
        store.getState().patch(draft.key, { runId, runSignature: captured });
        message.success("任务已开始");
      } finally {
        locks.delete(draft.key);
      }
    },
    onError: (error) => message.error(errorText(error)),
  });
  function savePreset(draft: Draft) {
    let title = "";
    modal.confirm({
      title: "保存参数预设",
      content: (
        <Input
          aria-label="预设名称"
          onChange={(event) => {
            title = event.target.value;
          }}
        />
      ),
      okText: "保存预设",
      cancelText: "取消",
      onOk: async () => {
        try {
          if (!title.trim()) throw new Error("请输入预设名称");
          if (Object.keys(draft.invalidNumbers).length)
            throw new Error("请先修正输入");
          await action("preset.save", {
            name: "preset-" + crypto.randomUUID(),
            asset: {
              title,
              template: draft.id,
              document_title: draft.title,
              context: decode(draft.parameters),
              parameter_selection:
                draft.preset === undefined ? {} : { preset_id: draft.preset },
            },
            expected: null,
          });
          message.success("参数预设已保存");
        } catch (error) {
          message.error(errorText(error));
          throw error;
        }
      },
    });
  }
  async function loadPreset(draft: Draft) {
    try {
      const listed = await action("preset.list");
      const records = await Promise.all(
        listed.data.names.map(async (name: string) => ({
          name,
          ...(await action("preset.read", { name })).data.asset,
        })),
      );
      const dialog = modal.info({
        title: "选择参数预设",
        content: (
          <Space vertical>
            {records
              .filter((entry: Data) =>
                draft.id
                  ? entry.template === draft.id
                  : entry.document_title === draft.title,
              )
              .map((entry: Data) => (
                <Button
                  key={entry.name}
                  onClick={() => {
                    store
                      .getState()
                      .patch(draft.key, {
                        parameters: encode(entry.context || {}, true),
                        preset: entry.parameter_selection?.preset_id,
                        invalidNumbers: {},
                        replay: undefined,
                      });
                    dialog.destroy();
                  }}
                >
                  {entry.title || entry.name}
                </Button>
              ))}
          </Space>
        ),
        okText: "关闭",
      });
    } catch (error) {
      message.error(errorText(error));
    }
  }
  async function remove(draft: Draft) {
    modal.confirm({
      title: "删除这个模板？",
      content: "删除已保存资产及当前草稿，仍被其他模板引用时将拒绝删除。",
      okText: "删除模板",
      okButtonProps: { danger: true },
      cancelText: "保留模板",
      onOk: async () => {
        try {
          await action("template.remove", {
            name: draft.id,
            expected: draft.fingerprint,
          });
          store.getState().close(draft.key);
          await refresh();
          navigate("/workbench");
        } catch (error) {
          message.error(errorText(error));
          throw error;
        }
      },
    });
  }
  async function inspect(draft: Draft) {
    try {
      const result = await action("template.inspect", {
        source: draft.source,
        ...(draft.id ? { template: draft.id } : {}),
      });
      const current = store
        .getState()
        .documents.find((doc) => doc.key === draft.key);
      if (
        !current ||
        current.source !== draft.source ||
        current.schema !== draft.schema
      ) {
        message.warning("源码或定义已修改，请重新检查");
        return;
      }
      const schema = decode(draft.schema),
        properties = { ...(schema.properties || {}) };
      for (const variable of result.data.variables || []) {
        const key = variable.name;
        if (typeof key === "string" && !Object.hasOwn(properties, key))
          Object.defineProperty(properties, key, {
            value: { title: key, "x-untyped": true },
            enumerable: true,
          });
      }
      store
        .getState()
        .patch(draft.key, {
          schema: encode({ ...schema, type: "object", properties }, true),
          tab: "schema",
        });
      message.success("变量已识别，请核对参数类型");
    } catch (error) {
      message.error(errorText(error));
    }
  }
  return {
    save,
    generate,
    copy,
    go,
    remove,
    inspect,
    savePreset,
    loadPreset,
    refresh,
    start,
    api,
  };
}
