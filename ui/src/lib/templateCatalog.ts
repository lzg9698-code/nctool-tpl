import { action } from "./api";
import type { Asset, Data, Document } from "./data";

// Presentation ownership only; storage and rendering APIs remain domain-neutral.
export function specializedTemplate(
  asset: Asset | undefined,
): "sync-turning" | "undercut" | undefined {
  const operation = asset?.metadata?.nc?.operation?.id;
  if (operation === "sync.left" || operation === "sync.right")
    return "sync-turning";
  if (operation === "groove") return "undercut";
}

export function generalDocuments(documents: Document[]): Document[] {
  return documents.filter((document) => !specializedTemplate(document.asset));
}

export async function loadTemplateCatalog() {
  const catalog = await action("template.catalog", {});
  const diagnostics: Data[] = [...(catalog.diagnostics || [])];
  const reads = await Promise.allSettled(
    catalog.data.items.map(async (item: Data) => {
      const read = await action("template.read", { name: item.id });
      return {
        ...item,
        asset: read.data.asset,
        fingerprint: read.data.fingerprint,
        schema: read.data.asset.schema,
      };
    }),
  );
  const items: Data[] = [];
  reads.forEach((read, index) => {
    if (read.status === "fulfilled") items.push(read.value);
    else
      diagnostics.push({
        level: "error",
        code: "invalid_asset",
        path: catalog.data.items[index].id,
        message: read.reason?.message || "模板读取失败",
      });
  });
  return { items, diagnostics };
}
