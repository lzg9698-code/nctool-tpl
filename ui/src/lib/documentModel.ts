import {
  clone,
  documentValue,
  newDocument,
  type Asset,
  type Document,
  type Schema,
} from "./data";
import { sameSchema } from "./documentExecution";
import { initialDocumentContext } from "./appDocumentParameters";
import { cloneDraft } from "./drafts";

// Shared document-shape rules. Both the workbench surface and the release
// entry point build, duplicate and inspect documents, so the rules live here
// instead of being re-implemented per surface.

export function normalizeSchema(asset: Asset): Schema {
  return asset.schema && typeof asset.schema === "object"
    ? asset.schema
    : { type: "object", properties: {}, required: [] };
}

// Build the editable document for a stored template. `actionId` decides whether
// a shared-parameter template starts with empty overrides (its definitions and
// presets own the values) or with the schema/defaults pre-filled.
export function prepareDocument(
  id: string,
  asset: Asset,
  fingerprint: string,
  actionId?: string,
): Document {
  const doc = newDocument();
  doc.id = id;
  doc.title = asset.metadata?.title || id;
  doc.asset = asset;
  doc.context = initialDocumentContext(asset, actionId);
  doc.fingerprint = fingerprint;
  doc.mode = "use";
  doc.outputName = asset.metadata?.output_name || id + ".txt";
  doc.saved = documentValue(doc);
  return doc;
}

// A duplicate is a new, unsaved draft. It must not inherit the source's run
// binding: the drafts store replays `lastRunId` on load, so a copied document
// would otherwise present the source run's result as its own.
export function cloneDocumentForCopy(doc: Document, suffix = " 副本"): Document {
  return {
    ...cloneDraft(doc),
    key: crypto.randomUUID(),
    id: null,
    fingerprint: null,
    title: doc.title + suffix,
    saved: "",
    lastRunId: undefined,
    lastRunSignature: undefined,
  };
}

export interface InspectVariable {
  name: string;
  optional?: boolean;
}

// Fold template inspection into the schema the document already has. Existing
// definitions are never overwritten, and a variable the source stopped
// referencing is kept (removal stays an explicit user decision).
export function mergeInspectedParameters(
  asset: Asset,
  variables: InspectVariable[],
): { asset: Asset; added: number; changed: boolean } {
  const schema = clone(normalizeSchema(asset));
  if (!schema.properties) schema.properties = {};
  if (!schema.required) schema.required = [];
  const properties = schema.properties;
  const required = schema.required;
  let added = 0;
  for (const variable of variables) {
    if (!properties[variable.name]) {
      properties[variable.name] = {
        title: variable.name,
        "x-untyped": true,
      };
      added++;
    }
    if (!variable.optional && !required.includes(variable.name))
      required.push(variable.name);
  }
  return {
    asset: { ...asset, schema },
    added,
    changed: added > 0 || !sameSchema(schema, normalizeSchema(asset)),
  };
}
