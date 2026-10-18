import { beforeEach, describe, expect, it, vi } from "vitest";
import { action } from "./api";
import { generalDocuments, loadTemplateCatalog } from "./templateCatalog";
import { decodeDraft, encodeDraft } from "./drafts";
import { domainTemplate } from "./documentExecution";
import { newDocument, type Asset } from "./data";

vi.mock("./api", () => ({ action: vi.fn() }));

// Retired machining templates are recognised by their metadata, not by file or
// template name: `nc` (even a bare null) or a retired execution action.
const retired = (nc: unknown = { operation: { id: "groove" } }): Asset => ({
  source: "{{ trajectory_program }}",
  metadata: { nc },
});
const plain: Asset = { source: "hello" };

beforeEach(() => vi.clearAllMocks());

describe("template presentation ownership", () => {
  it("hides retired machining drafts, null metadata included, without losing raw input or baseline", () => {
    const hidden = {
      ...newDocument(),
      title: "自定义文件名",
      asset: retired(),
      fingerprint: "old-baseline",
    };
    const hiddenNull = { ...newDocument(), asset: retired(null) };
    const normal = newDocument();
    const documents = [hidden, hiddenNull, normal];
    expect(generalDocuments(documents)).toEqual([normal]);
    expect(decodeDraft(encodeDraft(documents))).toEqual(documents);
    expect(documents[0]).toBe(hidden);
  });

  it("reads metadata independent of file names and preserves read fingerprints", async () => {
    vi.mocked(action).mockImplementation(async (id, input) =>
      id === "template.catalog"
        ? {
            data: {
              items: [{ id: "custom-file" }, { id: "sync-turning-template" }],
            },
          }
        : {
            data: {
              asset: input.name === "custom-file" ? retired() : plain,
              fingerprint: "current-baseline",
            },
          },
    );
    const result = await loadTemplateCatalog();
    expect(
      result.items
        .filter((item) => !domainTemplate(item.asset))
        .map((item) => item.id),
    ).toEqual(["sync-turning-template"]);
    expect(result.items[0].fingerprint).toBe("current-baseline");
    expect(result.diagnostics).toEqual([]);
  });

  it("keeps every asset's full metadata and read fingerprint, in catalog order", async () => {
    vi.mocked(action).mockImplementation(async (id, input) =>
      id === "template.catalog"
        ? { data: { items: [{ id: "a" }, { id: "b" }] } }
        : {
            data: {
              asset: {
                source: "x",
                metadata: { title: "标题 " + input.name, tags: ["t"] },
              },
              fingerprint: "fp-" + input.name,
            },
          },
    );
    const result = await loadTemplateCatalog();
    // Order is the catalog order, not the read completion order.
    expect(result.items.map((item) => item.id)).toEqual(["a", "b"]);
    // The whole asset (metadata included) survives so callers can classify it.
    expect(result.items[0].asset.metadata.title).toBe("标题 a");
    expect(result.items[0].asset.metadata.tags).toEqual(["t"]);
    // The read fingerprint is the baseline the save path compares against.
    expect(result.items.map((item) => item.fingerprint)).toEqual([
      "fp-a",
      "fp-b",
    ]);
  });

  it("retains readable assets and reports failed reads without leaking unclassified entries", async () => {
    vi.mocked(action).mockImplementation(async (id, input) => {
      if (id === "template.catalog")
        return {
          data: { items: [{ id: "ok" }, { id: "broken" }] },
          diagnostics: [{ code: "existing" }],
        };
      if (input.name === "broken") throw new Error("read conflict");
      return { data: { asset: retired(), fingerprint: "current" } };
    });
    const result = await loadTemplateCatalog();
    expect(result.items.map((item) => item.id)).toEqual(["ok"]);
    expect(result.diagnostics).toHaveLength(2);
    expect(result.diagnostics[1]).toMatchObject({
      path: "broken",
      message: "read conflict",
    });
  });
});
