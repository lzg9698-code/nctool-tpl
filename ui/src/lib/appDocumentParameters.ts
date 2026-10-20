import {
  defaultValue,
  encode,
  mergeDefaults,
  parseData,
  type Asset,
  type Data,
  type Document,
} from "./data";

export function initialDocumentContext(asset: Asset): Data {
  return mergeDefaults(
    defaultValue(asset.schema || { type: "object" }),
    asset.defaults,
  );
}

export function resetDocumentParameters(doc: Document): Partial<Document> {
  return {
    context: initialDocumentContext(doc.asset),
    jsonInput: undefined,
  };
}

// SchemaEditor updates the schema and defaults separately. Apply this patch to
// the current document so its callbacks cannot resurrect a removed field.
export function removeDocumentParameter(
  doc: Document,
  name: string,
): Partial<Document> {
  function withoutField(value: Data): Data {
    return value && Object.hasOwn(value, name)
      ? Object.fromEntries(Object.entries(value).filter(([key]) => key !== name))
      : value;
  }
  return {
    asset: { ...doc.asset, defaults: withoutField(doc.asset.defaults) },
    context: withoutField(doc.context),
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
