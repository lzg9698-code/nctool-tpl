import { describe, expect, it } from "vitest";
import {
  catalogTags,
  filterCatalogItems,
  type CatalogItemLike,
} from "./workbenchDerived";

// `fingerprint` is not part of the shared shape; the helpers must keep the
// caller's item type (and its extra fields) intact.
interface Row extends CatalogItemLike {
  fingerprint: string;
}

const rows: Row[] = [
  {
    id: "alpha",
    title: "同步粗车",
    description: "面板示例",
    tags: ["演示", "面板"],
    fingerprint: "f1",
  },
  {
    id: "beta",
    title: "汇总",
    description: "参数轨迹",
    tags: ["轨迹"],
    fingerprint: "f2",
  },
  {
    id: "gamma",
    title: "PHOTO",
    description: "大小写",
    tags: ["photo"],
    fingerprint: "f3",
  },
];

describe("filterCatalogItems", () => {
  it("matches title, description, id and tags case-insensitively, preserving order and identity", () => {
    expect(filterCatalogItems(rows, "面板", "").map((r) => r.id)).toEqual([
      "alpha",
    ]);
    expect(filterCatalogItems(rows, "参数", "").map((r) => r.id)).toEqual([
      "beta",
    ]);
    expect(filterCatalogItems(rows, "alpha", "").map((r) => r.id)).toEqual([
      "alpha",
    ]);
    expect(filterCatalogItems(rows, "photo", "").map((r) => r.id)).toEqual([
      "gamma",
    ]);
    expect(filterCatalogItems(rows, "", "")).toEqual(rows);
    expect(filterCatalogItems(rows, "", "").map((r) => r.fingerprint)).toEqual([
      "f1",
      "f2",
      "f3",
    ]);
  });

  it("combines the exact tag filter with the query", () => {
    expect(filterCatalogItems(rows, "面板", "演示").map((r) => r.id)).toEqual([
      "alpha",
    ]);
    expect(filterCatalogItems(rows, "面板", "轨迹")).toEqual([]);
    expect(filterCatalogItems(rows, "", "轨迹").map((r) => r.id)).toEqual([
      "beta",
    ]);
  });

  it("returns an empty list when nothing matches", () => {
    expect(filterCatalogItems(rows, "不存在", "")).toEqual([]);
    expect(filterCatalogItems([], "x", "")).toEqual([]);
  });
});

describe("catalogTags", () => {
  it("deduplicates tags in first-seen order", () => {
    expect(catalogTags(rows)).toEqual(["演示", "面板", "轨迹", "photo"]);
  });

  it("returns an empty list for no items", () => {
    expect(catalogTags([])).toEqual([]);
  });
});
