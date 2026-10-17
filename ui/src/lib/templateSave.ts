import type { Asset, Document } from "./data";
import { documentValue } from "./data";

export function templateSaveAsset(
  doc: Document,
  generationId?: string,
): Asset {
  return {
    ...doc.asset,
    metadata: {
      ...(doc.asset.metadata || {}),
      title: doc.title,
      output_name: doc.outputName,
      ...(generationId
        ? {
            execution: {
              ...(doc.asset.metadata?.execution || {}),
              action: generationId,
            },
          }
        : {}),
    },
  };
}

export function applyTemplateSaveResult(
  current: Document,
  submitted: Document,
  id: string,
  asset: Asset,
  fingerprint: string,
): Document {
  const savedSnapshot = { ...submitted, id, asset, fingerprint };
  const saved = documentValue(savedSnapshot);
  return {
    ...current,
    id,
    fingerprint,
    saved,
    asset:
      documentValue(current) === documentValue(submitted) ? asset : current.asset,
  };
}
