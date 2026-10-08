import { beforeEach, describe, expect, it, vi } from "vitest";
import { action } from "./api";
import {
  generalDocuments,
  loadTemplateCatalog,
  specializedTemplate,
} from "./templateCatalog";
import { decodeDraft, encodeDraft } from "./drafts";
import { newDocument } from "./data";

vi.mock("./api", () => ({ action: vi.fn() }));
const asset = (id: string) => ({
  source: "{{ trajectory_program }}",
  metadata: { nc: { operation: { id } } },
});
beforeEach(() => vi.clearAllMocks());

describe("template presentation ownership", () => {
  it("classifies only the three supported operation IDs", () => {
    expect(specializedTemplate(asset("sync.left"))).toBe("sync-turning");
    expect(specializedTemplate(asset("sync.right"))).toBe("sync-turning");
    expect(specializedTemplate(asset("groove"))).toBe("undercut");
    for (const id of ["sync.other", "groove.finish", "direct", "circlip", ""])
      expect(specializedTemplate(asset(id))).toBeUndefined();
    expect(
      specializedTemplate({
        source: "",
        tags: ["sync.left"],
        metadata: { title: "同步车左侧模板" },
      }),
    ).toBeUndefined();
    expect(specializedTemplate(undefined)).toBeUndefined();
  });

  it("hides specialized drafts without losing their raw input or saved baseline", () => {
    const hidden = {
      ...newDocument(),
      title: "自定义文件名",
      asset: asset("groove"),
      fingerprint: "old-baseline",
    };
    const normal = newDocument();
    const documents = [hidden, normal];
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
              asset:
                input.name === "custom-file"
                  ? asset("sync.left")
                  : { source: "hello" },
              fingerprint: "current-baseline",
            },
          },
    );
    const result = await loadTemplateCatalog();
    expect(
      result.items
        .filter((item) => !specializedTemplate(item.asset))
        .map((item) => item.id),
    ).toEqual(["sync-turning-template"]);
    expect(result.items[0].fingerprint).toBe("current-baseline");
    expect(result.diagnostics).toEqual([]);
  });

  it("retains readable assets and reports failed reads without leaking unclassified entries", async () => {
    vi.mocked(action).mockImplementation(async (id, input) => {
      if (id === "template.catalog")
        return {
          data: { items: [{ id: "ok" }, { id: "broken" }] },
          diagnostics: [{ code: "existing" }],
        };
      if (input.name === "broken") throw new Error("read conflict");
      return { data: { asset: asset("groove"), fingerprint: "current" } };
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
