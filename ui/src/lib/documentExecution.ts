import { encode, mergeDefaults, type Data, type Document } from "./data";

export function documentActions(actions: Data[]): Data[] {
  return actions.filter((entry) => entry.ui_schema?.document_input?.bindings);
}
export function selectDocumentAction(
  doc: Document,
  actions: Data[],
  knownActions: Data[] = actions,
): { entry?: Data; error?: string } {
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
  const values: Record<string, Data> = {
    source: doc.asset.source,
    id: doc.id || undefined,
    context: doc.context,
    effective_context: mergeDefaults(doc.asset.defaults, doc.context),
    schema: doc.asset.schema || { type: "object" },
    defaults: doc.asset.defaults || {},
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
      for (const key of binding.slice(9).split(".")) {
        value = value && Object.hasOwn(value, key) ? value[key] : undefined;
      }
    } else if (Object.hasOwn(values, binding)) value = values[binding];
    else throw new Error("生成方式包含未知输入绑定：" + binding);
    if (value !== undefined) pairs.push([target, value]);
  }
  const input = Object.fromEntries(pairs);
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
      action,
    });
  } catch {
    return "invalid";
  }
}
