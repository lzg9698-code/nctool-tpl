import {
  clone,
  encode,
  mergeDefaults,
  newDocument,
  type Asset,
  type Data,
  type Document,
  type Run,
  type Schema,
} from "./data";

// Legacy machining recognition only. The machining capability has been removed,
// so a template carrying old machining metadata or a retired execution action
// must never downgrade to plain or shared rendering. The UI refuses it and asks
// the user to remove the machining intent; availability always comes from the
// current capabilities.
export const machiningRemovedMessage =
  "此模板带有已移除的加工意图，不能按普通模板渲染。请移除加工元数据或旧的加工执行动作后再生成。";
export function domainAction(id: unknown): boolean {
  return (
    typeof id === "string" && /^(nc|machine|process|sync-turning|undercut)\./.test(id)
  );
}
export function domainTemplate(asset?: Asset): boolean {
  const metadata = asset?.metadata;
  return (
    !!metadata &&
    (Object.hasOwn(metadata, "nc") || domainAction(metadata.execution?.action))
  );
}
export function documentActions(actions: Data[]): Data[] {
  return actions.filter(
    (entry) =>
      !domainAction(entry.id) &&
      entry.id !== "template.render.managed" &&
      entry.ui_schema?.document_input?.bindings,
  );
}
export function activePanels(panels: Data[], actions: Data[]): Data[] {
  const ids = new Set(
    actions.filter((entry) => !domainAction(entry.id)).map((entry) => entry.id),
  );
  return panels
    .filter(
      (panel) =>
        !domainAction(panel.view) &&
        !domainAction(panel.id) &&
        panel.view !== "template-editor" &&
        panel.id !== "presets",
    )
    .map((panel) => ({
      ...panel,
      actions: (panel.actions || []).filter((id: string) => ids.has(id)),
    }))
    .filter((panel) => panel.actions.length);
}
export function selectDocumentAction(
  doc: Document,
  actions: Data[],
  knownActions: Data[] = actions,
): { entry?: Data; error?: string } {
  if (domainTemplate(doc.asset) || domainAction(doc.generationAction))
    return { error: machiningRemovedMessage };
  const choices = documentActions(actions);
  const requested =
    doc.generationAction || doc.asset.metadata?.execution?.action;
  if (requested) {
    const entry = choices.find((item) => item.id === requested);
    return entry
      ? { entry }
      : {
          error: `生成方式 ${requested} 当前不可用，请启用相应插件并重启，或明确选择其他生成方式。`,
        };
  }
  const matched = documentActions(knownActions).filter((item) => {
    const key = item.ui_schema.document_input.match_metadata;
    return key && Object.hasOwn(doc.asset.metadata || {}, key);
  });
  if (matched.length === 1) {
    const entry = choices.find((item) => item.id === matched[0].id);
    return entry
      ? { entry }
      : {
          error: `生成方式 ${matched[0].id} 当前不可用，请启用相应插件并重启，或明确选择其他生成方式。`,
        };
  }
  if (matched.length > 1)
    return { error: "多个插件支持这个模板，请明确选择生成方式。" };
  const defaults = choices.filter(
    (item) => item.ui_schema.document_input.default === true,
  );
  return defaults.length === 1
    ? { entry: defaults[0] }
    : { error: "请选择当前可用的生成方式。" };
}
export function documentInput(doc: Document, entry: Data): Data {
  if (
    domainTemplate(doc.asset) ||
    domainAction(doc.generationAction) ||
    domainAction(entry.id)
  )
    throw new Error(machiningRemovedMessage);
  const values: Record<string, Data> = {
    source: doc.asset.source,
    id: doc.id || undefined,
    context: doc.context,
    effective_context: mergeDefaults(doc.asset.defaults, doc.context),
    schema: doc.asset.schema ?? { type: "object" },
    defaults: doc.asset.defaults || {},
    extensions:
      doc.extensions ?? doc.asset.metadata?.execution?.extensions ?? [],
  };
  const pairs: [string, Data][] = [];
  for (const [target, binding] of Object.entries(
    entry.ui_schema.document_input.bindings,
  )) {
    if (typeof binding !== "string")
      throw new Error("生成方式的文档输入声明无效");
    let value: Data;
    if (binding.startsWith("metadata.")) {
      value = doc.asset.metadata;
      for (const key of binding.slice(9).split("."))
        value = value && Object.hasOwn(value, key) ? value[key] : undefined;
    } else if (Object.hasOwn(values, binding)) value = values[binding];
    else throw new Error("生成方式包含未知输入绑定：" + binding);
    if (value !== undefined) pairs.push([target, value]);
  }
  const input = Object.fromEntries(pairs);
  // Older capability descriptors may omit this binding; the generic render
  // action already accepts the explicit extension field. Never silently discard it.
  if (entry.id === "template.render") input.extensions = values.extensions;
  const replay = doc.replayInput;
  if (replay?.snapshot && entry.id === "template.render") {
    input.snapshot = clone(replay.snapshot);
    input.snapshot.templates[input.snapshot.template] = doc.asset.source;
    input.snapshot.asset = clone(doc.asset);
    delete input.source;
    delete input.template;
    for (const key of [
      "extensions",
      "trim_blocks",
      "lstrip_blocks",
      "lenient",
    ]) {
      if (input[key] === undefined && replay[key] !== undefined)
        input[key] = clone(replay[key]);
    }
  }
  encode(input); // Incomplete numeric drafts must not become previous valid values.
  return input;
}
export function documentSignature(doc: Document, action?: string): string {
  try {
    return encode({
      id: doc.id,
      source: doc.asset.source,
      schema: doc.asset.schema,
      defaults: doc.asset.defaults,
      metadata: doc.asset.metadata,
      context: doc.context,
      extensions: doc.extensions,
      replay_input: doc.replayInput,
      action,
    });
  } catch {
    return "invalid";
  }
}
// Schema equality for late-response guards. Normalises the optional container
// fields so that `{}` and `{properties:{},required:[]}` are not treated as
// different versions, and never throws on unencodable drafts.
export function sameSchema(a: Schema, b: Schema): boolean {
  try {
    return signatureOfSchema(a) === signatureOfSchema(b);
  } catch {
    return false;
  }
}
function signatureOfSchema(schema: Schema): string {
  return encode({
    type: schema.type,
    properties: schema.properties || {},
    required: schema.required || [],
    items: schema.items,
    enum: schema.enum,
  });
}
export function historyExecutionReason(run: Run, actions: Data[]): string {
  const input = run.result?.data?.replay_input || run.input;
  if (
    domainAction(run.action) ||
    domainTemplate(input?.snapshot?.asset) ||
    domainTemplate(run.input?.snapshot?.asset)
  )
    return "此记录带有已移除的加工意图，不能重执行或恢复为模板文档；历史结果仍可查看、比较和导出。";
  if (!actions.some((entry) => entry.id === run.action))
    return `动作 ${run.action} 当前没有可用提供方；历史结果保留，不能重执行或恢复输入。`;
  return "";
}
export function restoreDocument(run: Run, actions: Data[]): Document {
  const reason = historyExecutionReason(run, actions);
  if (reason) throw new Error(reason);
  if (run.action !== "template.render")
    throw new Error("此动作不支持恢复为模板文档，原始输入仍可在历史中查看。");
  const input = clone(run.result?.data?.replay_input || run.input);
  const snapshot = input.snapshot;
  const restored = newDocument();
  restored.title = run.title + " · 历史输入";
  restored.generationAction = run.action;
  restored.asset = snapshot?.asset || {
    source: "",
    schema: { type: "object", properties: {} },
    defaults: {},
    metadata: {},
  };
  restored.asset.source =
    snapshot?.templates?.[snapshot.template] ??
    input.source ??
    restored.asset.source;
  restored.asset.schema =
    snapshot?.asset?.schema ?? input.schema ?? restored.asset.schema;
  restored.asset.defaults = input.defaults ?? restored.asset.defaults;
  restored.context = clone(input.context ?? {});
  restored.extensions = input.extensions ?? [];
  restored.replayInput = snapshot ? input : undefined;
  restored.mode = "use";
  restored.lastRunId = run.id;
  restored.lastRunSignature = documentSignature(restored, run.action);
  return restored;
}
