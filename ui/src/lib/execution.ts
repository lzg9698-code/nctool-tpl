import { decode, encode, merge, type Data } from "./json";
import { assetOf, makeDraft, signature, type Draft } from "../state/session";

export const retired = (id: string) =>
  /^(nc|machine|process|sync-turning|undercut)\./.test(id);
export function assertExecutable(asset: Data, action: string) {
  if (
    retired(action) ||
    (asset.metadata &&
      (Object.hasOwn(asset.metadata, "nc") ||
        retired(asset.metadata.execution?.action || "")))
  )
    throw new Error("此资产声明的加工能力已移除，不能作为普通模板执行");
}
export function selectAction(draft: Draft, capabilities: Data): Data {
  const asset = assetOf(draft);
  assertExecutable(asset, draft.action);
  const actions = (capabilities.actions || []).filter(
    (entry: Data) =>
      entry.ui_schema?.document_input?.bindings && !retired(entry.id),
  );
  const requested = draft.action || asset.metadata?.execution?.action;
  const found = actions.find((entry: Data) => entry.id === requested);
  if (!found)
    throw new Error(`生成方式 ${requested || ""} 当前不可用，请检查插件`);
  return found;
}
export function resolveRequest(draft: Draft): Data {
  const asset = assetOf(draft);
  const request: Data = {
    bindings: asset.metadata.parameters?.bindings || {},
    defaults: asset.defaults,
    schema: asset.schema,
    values: decode(draft.parameters),
  };
  const preset =
    draft.preset !== undefined
      ? draft.preset
      : asset.metadata.parameters?.preset_id;
  if (preset !== undefined) request.preset_id = preset;
  if (draft.replay) {
    const replay = decode(draft.replay);
    if (replay.parameter_snapshot && draft.preset === undefined)
      request.snapshot = replay.parameter_snapshot;
  }
  return request;
}
export function inputFor(draft: Draft, entry: Data): Data {
  if (Object.keys(draft.invalidNumbers).length)
    throw new Error("请修正未完成的数值输入");
  const asset = assetOf(draft),
    context = decode(draft.parameters);
  assertExecutable(asset, entry.id);
  const values: Data = {
    source: asset.source,
    id: draft.id || undefined,
    schema: asset.schema,
    defaults: asset.defaults,
    context,
    effective_context: merge(asset.defaults, context),
    extensions: draft.extensions,
    parameter_selection: { preset_id: draft.preset },
    parameter_preset_id:
      draft.preset !== undefined
        ? draft.preset
        : asset.metadata.parameters?.preset_id,
  };
  const input: Data = {};
  for (const [target, binding] of Object.entries(
    entry.ui_schema.document_input.bindings,
  )) {
    if (typeof binding !== "string") throw new Error("插件文档输入声明无效");
    const value = binding.startsWith("metadata.")
      ? binding
          .slice(9)
          .split(".")
          .reduce((node: Data, key) => node?.[key], asset.metadata)
      : values[binding];
    if (!binding.startsWith("metadata.") && !Object.hasOwn(values, binding))
      throw new Error("未知文档输入绑定：" + binding);
    if (value !== undefined) input[target] = value;
  }
  if (
    draft.replay &&
    ["template.render", "template.render.managed"].includes(entry.id)
  ) {
    const replay = decode(draft.replay);
    if (replay.snapshot) {
      input.snapshot = replay.snapshot;
      input.snapshot.asset = asset;
      input.snapshot.templates[input.snapshot.template] = asset.source;
      delete input.source;
      delete input.template;
      for (const key of ["trim_blocks", "lstrip_blocks", "lenient"])
        if (replay[key] !== undefined) input[key] = replay[key];
      if (
        entry.id === "template.render.managed" &&
        replay.parameter_snapshot &&
        draft.preset === undefined
      ) {
        input.parameter_snapshot = replay.parameter_snapshot;
        input.parameter_request = {
          ...replay.parameter_request,
          values: context,
          schema: asset.schema,
          defaults: asset.defaults,
          bindings: asset.metadata.parameters?.bindings || {},
        };
        for (const key of [
          "context",
          "schema",
          "defaults",
          "bindings",
          "preset_id",
        ])
          delete input[key];
      }
    }
  }
  encode(input);
  return input;
}
export function restoreRun(run: Data, capabilities: Data): Draft {
  const input = run.result?.data?.replay_input || run.input;
  assertExecutable(run.input?.snapshot?.asset || {}, run.action);
  assertExecutable(input.snapshot?.asset || {}, run.action);
  if (!capabilities.actions.some((entry: Data) => entry.id === run.action))
    throw new Error("此动作当前不可用");
  if (!["template.render", "template.render.managed"].includes(run.action))
    throw new Error("此动作只能从历史重新执行，不能恢复为模板");
  const asset = {
    ...input.snapshot?.asset,
    source:
      input.snapshot?.templates?.[input.snapshot.template] ??
      input.source ??
      "",
  };
  const request = input.parameter_request;
  asset.schema = asset.schema ?? request?.schema ?? input.schema;
  asset.defaults = request?.defaults ?? input.defaults ?? asset.defaults;
  const draft = makeDraft(asset, null, null, run.action);
  draft.title = run.title + " · 历史输入";
  draft.parameters = encode(request?.values ?? input.context ?? {}, true);
  draft.extensions = input.extensions || [];
  draft.mode = "use";
  draft.replay = input.snapshot ? encode(input) : undefined;
  if (request) {
    const metadata = decode(draft.metadata);
    metadata.parameters = {
      ...metadata.parameters,
      bindings: request.bindings || {},
    };
    delete metadata.parameters.preset_id;
    draft.metadata = encode(metadata, true);
  }
  draft.runId = run.id;
  draft.runSignature = signature(draft);
  return draft;
}
export function deliveryReason(
  draft: Draft | undefined,
  run: Data,
  dependenciesCurrent: boolean,
): string {
  if (!run) return "生成后可查看结果";
  if (run.status !== "succeeded") return "只有成功任务可以导出";
  if (draft && signature(draft) !== draft.runSignature)
    return "输入已修改，请重新生成";
  if (!dependenciesCurrent)
    return "模板或参数依赖尚未确认，或已变化，请重新生成";
  if (!run.result?.artifacts?.length) return "当前任务没有可交付文件";
  return "";
}
