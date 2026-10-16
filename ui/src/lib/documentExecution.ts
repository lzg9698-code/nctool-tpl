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

// Compatibility recognition only: retired intent must survive a smaller capability list.
export function retiredAction(id: unknown): boolean {
  return (
    typeof id === "string" && /^(nc|machine|process|sync-turning|undercut)\./.test(id)
  );
}
export function retiredTemplate(asset?: Asset): boolean {
  const metadata = asset?.metadata;
  return (
    !!metadata &&
    (Object.hasOwn(metadata, "nc") || retiredAction(metadata.execution?.action))
  );
}
export const retirementMessage =
  "此模板使用已退役的加工能力，可查看和编辑，不能执行或试跑。请另行制作并核对通用模板，显式清除旧执行意图。";
export function documentActions(actions: Data[]): Data[] {
  return actions.filter(
    (entry) =>
      !retiredAction(entry.id) && entry.ui_schema?.document_input?.bindings,
  );
}
export function activePanels(panels: Data[], actions: Data[]): Data[] {
  const ids = new Set(
    actions
      .filter((entry) => !retiredAction(entry.id))
      .map((entry) => entry.id),
  );
  return panels
    .filter(
      (panel) =>
        !retiredAction(panel.view) &&
        !retiredAction(panel.id) &&
        panel.view !== "template-editor" &&
        panel.id !== "presets",
    )
    .map((panel) => ({
      ...panel,
      actions: (panel.actions || []).filter((id: string) => ids.has(id)),
    }))
    .filter(
      (panel) => panel.view === "parameter.library" || panel.actions.length,
    );
}
export function selectDocumentAction(
  doc: Document,
  actions: Data[],
  knownActions: Data[] = actions,
): { entry?: Data; error?: string } {
  if (retiredTemplate(doc.asset) || retiredAction(doc.generationAction))
    return { error: retirementMessage };
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
    retiredTemplate(doc.asset) ||
    retiredAction(doc.generationAction) ||
    retiredAction(entry.id)
  )
    throw new Error(retirementMessage);
  const values: Record<string, Data> = {
    source: doc.asset.source,
    id: doc.id || undefined,
    context: doc.context,
    effective_context: mergeDefaults(doc.asset.defaults, doc.context),
    schema: doc.asset.schema ?? { type: "object" },
    defaults: doc.asset.defaults || {},
    extensions:
      doc.extensions ?? doc.asset.metadata?.execution?.extensions ?? [],
    parameter_selection:
      doc.parameterSelection || doc.asset.metadata?.parameters?.selection,
    // Keep an explicit decline: `?? undefined` or `|| undefined` would drop it
    // and let the backend fall back to the preset saved on the template.
    parameter_preset_id: (() => {
      const selection = doc.parameterSelection ?? doc.asset.metadata?.parameters?.selection;
      return selection && Object.hasOwn(selection, "preset_id")
        ? selection.preset_id
        : doc.asset.metadata?.parameters?.preset_id;
    })(),
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
  // Older capability descriptors may omit this binding; both generic render actions
  // already accept the explicit extension field. Never silently discard a choice.
  if (["template.render", "template.render.managed"].includes(entry.id))
    input.extensions = values.extensions;
  const replay = doc.replayInput;
  if (
    replay?.snapshot &&
    ["template.render", "template.render.managed"].includes(entry.id)
  ) {
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
    if (
      entry.id === "template.render.managed" &&
      replay.parameter_snapshot &&
      !input.preset_id
    ) {
      input.parameter_snapshot = clone(replay.parameter_snapshot);
      input.parameter_request = {
        ...replay.parameter_request,
        values: doc.context,
        schema: replay.parameter_request?.schema ?? input.schema ?? doc.asset.schema ?? true,
        defaults: input.defaults ?? doc.asset.defaults ?? {},
        bindings:
          input.bindings ?? doc.asset.metadata?.parameters?.bindings ?? {},
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
      parameter_selection: doc.parameterSelection,
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
    retiredAction(run.action) ||
    retiredTemplate(input?.snapshot?.asset) ||
    retiredTemplate(run.input?.snapshot?.asset)
  )
    return "此记录使用已退役的加工能力；历史结果仍可查看、比较和导出，不能重执行或恢复为执行文档。";
  if (!actions.some((entry) => entry.id === run.action))
    return `动作 ${run.action} 当前没有可用提供方；历史结果保留，不能重执行或恢复输入。`;
  return "";
}
export function restoreDocument(run: Run, actions: Data[]): Document {
  const reason = historyExecutionReason(run, actions);
  if (reason) throw new Error(reason);
  if (!["template.render", "template.render.managed"].includes(run.action))
    throw new Error("此动作不支持恢复为模板文档，原始输入仍可在历史中查看。");
  const input = clone(run.result?.data?.replay_input || run.input);
  const snapshot = input.snapshot;
  const request =
    run.action === "template.render.managed"
      ? input.parameter_request
      : undefined;
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
    snapshot?.asset?.schema ?? request?.schema ?? input.schema ?? restored.asset.schema;
  restored.asset.defaults =
    request?.defaults ?? input.defaults ?? restored.asset.defaults;
  if (request) {
    restored.asset.metadata = {
      ...restored.asset.metadata,
      parameters: {
        ...restored.asset.metadata?.parameters,
        bindings: request.bindings || {},
      },
    };
    delete restored.asset.metadata.parameters.preset_id;
    delete restored.asset.metadata.parameters.selection;
  }
  restored.context = clone(request?.values ?? input.context ?? {});
  // Preserve a frozen decline: an empty selection would fall back to the preset
  // saved on the template instead of the one this run actually used.
  restored.parameterSelection =
    input.preset_id === undefined
      ? {}
      : { preset_id: input.preset_id ?? null };
  restored.extensions = input.extensions ?? [];
  restored.replayInput = snapshot ? input : undefined;
  restored.mode = "use";
  restored.lastRunId = run.id;
  restored.lastRunSignature = documentSignature(restored, run.action);
  return restored;
}
