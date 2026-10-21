import {
  clone,
  defaultValue,
  encode,
  mergeDefaults,
  parseData,
  type Asset,
  type Data,
  type Document,
} from "./data";

export function sharedParameterDocument(
  asset: Asset,
  actionId?: string,
): boolean {
  return (
    actionId === "template.render.managed" &&
    !Object.hasOwn(asset.metadata || {}, "nc")
  );
}

export function initialDocumentContext(asset: Asset, actionId?: string): Data {
  const requested =
    actionId ??
    asset.metadata?.execution?.action ??
    (asset.metadata?.parameters ? "template.render.managed" : undefined);
  if (sharedParameterDocument(asset, requested)) return {};
  return asset.metadata?.parameters
    ? clone(asset.defaults || {})
    : mergeDefaults(
        defaultValue(asset.schema || { type: "object" }),
        asset.defaults,
      );
}

export function resetDocumentParameters(
  doc: Document,
  actionId?: string,
): Partial<Document> {
  return {
    context: sharedParameterDocument(doc.asset, actionId)
      ? {}
      : mergeDefaults(
          defaultValue(doc.asset.schema || { type: "object" }),
          doc.asset.defaults,
        ),
    jsonInput: undefined,
  };
}

// Always execute the current raw draft, even before the editor's parsed callback.
// A malformed draft must never fall back to an older context.
export function documentForGeneration(doc: Document): Document {
  const context =
    doc.jsonInput === undefined ? doc.context : parseData(doc.jsonInput);
  encode(context); // Also rejects incomplete numeric form drafts.
  return { ...doc, context };
}

export function templatePresetParameters(doc: Document): Data {
  return {
    context: clone(doc.context),
    parameter_selection: clone(
      doc.parameterSelection ?? doc.asset.metadata?.parameters?.selection ?? {},
    ),
  };
}

export function loadTemplatePresetParameters(preset: Data): Partial<Document> {
  return {
    context: clone(preset.context),
    ...(Object.hasOwn(preset, "parameter_selection")
      ? { parameterSelection: clone(preset.parameter_selection) }
      : {}),
    jsonInput: undefined,
  };
}
