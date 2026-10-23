import { describe, expect, it, vi, afterEach } from "vitest";
import { decode, encode, numeric, merge, overrideAt, getAt } from "./json";
import { api, ApiError } from "./api";
import {
  assetOf,
  assetSignature,
  createSession,
  makeDraft,
  signature,
} from "../state/session";
import {
  assertExecutable,
  deliveryReason,
  inputFor,
  resolveRequest,
  restoreRun,
  selectAction,
} from "./execution";

const plain = {
  id: "template.render",
  ui_schema: {
    document_input: {
      default: true,
      bindings: {
        source: "source",
        template: "id",
        context: "effective_context",
        schema: "schema",
        extensions: "extensions",
      },
    },
  },
};
const managed = {
  id: "template.render.managed",
  ui_schema: {
    document_input: {
      match_metadata: "parameters",
      bindings: {
        source: "source",
        template: "id",
        context: "context",
        schema: "schema",
        defaults: "defaults",
        bindings: "metadata.parameters.bindings",
        preset_id: "parameter_preset_id",
        extensions: "extensions",
      },
    },
  },
};
const capabilities = { actions: [plain, managed] };
afterEach(() => vi.unstubAllGlobals());
describe("无损输入与当前执行契约", () => {
  it.each([
    "9007199254740993",
    "18446744073709551615",
    "-9223372036854775808",
    "1e-400",
    "1.234567890123456789",
  ])("保留数字字面量 %s，数值范围由服务端判断", (raw) => {
    const draft = makeDraft({ source: "{{ n }}" });
    draft.parameters = '{"n":' + raw + "}";
    expect(encode(inputFor(draft, plain))).toContain('"n":' + raw);
    expect(encode(decode(draft.parameters))).toBe(draft.parameters);
  });
  it.each(["-", "1e", "NaN", "Infinity", "01", ""])(
    "拒绝未完成或非法数字 %s",
    (raw) => expect(() => numeric(raw)).toThrow(),
  );
  it("无效 JSON 与未完成表单都不能退回旧值执行", () => {
    const draft = makeDraft({ source: "{{ n }}", defaults: { n: 1 } });
    draft.parameters = '{"n":';
    expect(() => inputFor(draft, plain)).toThrow();
    draft.parameters = '{"n":1}';
    draft.invalidNumbers = { "/n": "1e" };
    expect(() => inputFor(draft, plain)).toThrow();
  });
  it("嵌套合并保留 null、数组及数值原文", () => {
    const next = merge(
      decode('{"x":{"a":1,"b":2},"items":[1]}'),
      decode('{"x":{"a":null},"items":[9007199254740993]}'),
    );
    expect(encode(next)).toBe(
      '{"x":{"a":null,"b":2},"items":[9007199254740993]}',
    );
  });
  it("手动覆盖只包含触碰字段，数组内部改动覆盖整个数组", () => {
    const inherited = decode(
      '{"size":{"width":8,"height":9},"items":[{"n":1},{"n":2}]}',
    );
    const override = overrideAt({}, inherited, ["size", "width"], numeric("8"));
    expect(encode(override)).toBe('{"size":{"width":8}}');
    const result = overrideAt(
      override,
      inherited,
      ["items", 1, "n"],
      numeric("9007199254740993"),
    );
    expect(encode(result)).toContain(
      '"items":[{"n":1},{"n":9007199254740993}]',
    );
    expect(
      overrideAt(result, inherited, ["size", "width"], undefined),
    ).not.toHaveProperty("size");
    expect(encode(getAt(result, ["items", 1, "n"]))).toBe("9007199254740993");
  });
  it("本次不选预置明确发送 null，共享执行只发送手动覆盖", () => {
    const draft = makeDraft({
      source: "{{ size }}",
      defaults: { size: 4 },
      metadata: {
        parameters: { bindings: { "/size": "size" }, preset_id: "saved" },
      },
    });
    draft.preset = null;
    expect(resolveRequest(draft).preset_id).toBeNull();
    expect(inputFor(draft, managed)).toMatchObject({
      context: {},
      preset_id: null,
    });
    expect(encode(inputFor(draft, managed).defaults)).toBe('{"size":4}');
  });
  it("不可用的显式动作不会回退普通渲染", () => {
    const draft = makeDraft({
      source: "x",
      metadata: { execution: { action: "missing.generate" } },
    });
    expect(() => selectAction(draft, capabilities)).toThrow("当前不可用");
  });
  it.each([{ nc: null }, { nc: {} }, { execution: { action: "nc.generate" } }])(
    "拒绝退役执行意图 %j",
    (metadata) => {
      const asset = { source: "x", metadata };
      expect(() => assertExecutable(asset, "template.render")).toThrow();
      expect(() => inputFor(makeDraft(asset), plain)).toThrow();
    },
  );
  it("保留开放资产字段，不在保存时丢失扩展数据", () => {
    const draft = makeDraft({
      source: "x",
      custom: { nested: [1, null] },
      metadata: { description: "描述" },
    });
    expect(encode(assetOf(draft).custom)).toBe('{"nested":[1,null]}');
    expect(assetOf(draft).metadata.description).toBe("描述");
  });
  it("修改输入或执行声明后禁止导出，失败/取消/中断均无当前交付", () => {
    const draft = makeDraft({ source: "{{ n }}" });
    draft.runSignature = signature(draft);
    const run = {
      status: "succeeded",
      result: {
        artifacts: [
          { name: "a", text: "a" },
          { name: "b", text: "b" },
        ],
      },
    };
    expect(deliveryReason(draft, run, true)).toBe("");
    expect(deliveryReason(draft, run, false)).not.toBe("");
    draft.parameters = '{"n":2}';
    expect(deliveryReason(draft, run, true)).not.toBe("");
    for (const status of ["failed", "cancelled", "interrupted", "running"])
      expect(deliveryReason(undefined, { ...run, status }, true)).not.toBe("");
  });
  it("恢复历史保持冻结来源、参数、扩展；改绑定可解除冻结", () => {
    const snapshot = {
      template: "frozen",
      templates: { frozen: "{{ n }}" },
      asset: { source: "{{ n }}", schema: { type: "object" } },
    };
    const run = {
      id: "old-run",
      title: "历史",
      action: managed.id,
      result: {
        data: {
          replay_input: {
            snapshot,
            extensions: ["math"],
            parameter_snapshot: { version: 1, records: [] },
            parameter_request: {
              values: decode('{"n":9007199254740993}'),
              bindings: { "/n": "number" },
            },
          },
        },
      },
    };
    const draft = restoreRun(run, capabilities),
      input = inputFor(draft, managed);
    expect(input.snapshot.templates.frozen).toBe("{{ n }}");
    expect(encode(input.parameter_request.values)).toBe(
      '{"n":9007199254740993}',
    );
    expect(input.extensions).toEqual(["math"]);
    expect(input.context).toBeUndefined();
    draft.replay = undefined;
    expect(inputFor(draft, managed).context).toBeDefined();
  });
});
describe("新版草稿隔离", () => {
  it("只读写新版命名空间，工作区互不覆盖，旧数据逐字不变", () => {
    const entries = new Map<string, string>([
      ["nctool:workbench:w", '{"old":true}'],
    ]);
    const reads: string[] = [];
    const storage = {
      getItem: (key: string) => {
        reads.push(key);
        return entries.get(key) || null;
      },
      setItem: (key: string, value: string) => {
        entries.set(key, value);
      },
      removeItem: (key: string) => entries.delete(key),
    };
    const first = createSession("w", storage),
      draft = makeDraft({ source: "{{ n }}" });
    draft.parameters = '{"n":9007199254740993}';
    draft.invalidNumbers = { "/n": "-" };
    first.getState().add(draft);
    const restored = createSession("w", storage),
      other = createSession("other", storage);
    expect(restored.getState().documents[0]).toMatchObject({
      parameters: draft.parameters,
      invalidNumbers: { "/n": "-" },
    });
    expect(other.getState().documents).toEqual([]);
    expect(reads.every((key) => key.startsWith("nctool:rewrite:v1:"))).toBe(
      true,
    );
    expect(entries.get("nctool:workbench:w")).toBe('{"old":true}');
  });
  it("存储失败明确上报且不会无限递归写入", async () => {
    let writes = 0;
    const store = createSession("w", {
      getItem: () => null,
      setItem: () => {
        writes++;
        throw new Error("quota");
      },
      removeItem: () => {},
    });
    store.getState().add(makeDraft());
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(store.getState().storageError).toContain("写入失败");
    expect(writes).toBeLessThan(5);
  });
  it("保存提交期间继续编辑，推进指纹但不覆盖较新的源码", () => {
    const store = createSession("w", {
        getItem: () => null,
        setItem: () => {},
        removeItem: () => {},
      }),
      draft = makeDraft({ source: "before" });
    store.getState().add(draft);
    const saved = assetSignature(draft);
    store.getState().patch(draft.key, { source: "after" });
    store.getState().patch(draft.key, { saved, fingerprint: "new", id: "id" });
    const current = store.getState().documents[0];
    expect(current.source).toBe("after");
    expect(assetSignature(current)).not.toBe(current.saved);
  });
});
describe("HTTP 契约", () => {
  it("POST 使用无损编码并传递取消信号", async () => {
    const fetcher = vi
      .fn()
      .mockResolvedValue({
        ok: true,
        status: 200,
        text: async () => '{"ok":true,"data":{"n":9007199254740993}}',
      });
    vi.stubGlobal("fetch", fetcher);
    const controller = new AbortController();
    const response = await api(
      "runs",
      decode('{"n":9007199254740993}'),
      controller.signal,
    );
    expect(fetcher.mock.calls[0][1]).toMatchObject({
      body: '{"n":9007199254740993}',
      signal: controller.signal,
    });
    expect(encode(response)).toBe('{"n":9007199254740993}');
  });
  it("冲突错误保留服务端错误码；非 JSON 不当作成功", async () => {
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValue({
          ok: false,
          status: 409,
          text: async () =>
            '{"ok":false,"error":{"code":"conflict","message":"冲突"}}',
        }),
    );
    try {
      await api("x", {});
      expect.fail("应抛出冲突");
    } catch (error) {
      expect(error).toBeInstanceOf(ApiError);
      expect((error as ApiError).detail.code).toBe("conflict");
    }
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValue({
          ok: true,
          status: 200,
          text: async () => "<html>error</html>",
        }),
    );
    await expect(api("x")).rejects.toThrow("服务响应无效");
  });
});
