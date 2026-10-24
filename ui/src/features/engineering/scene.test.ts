import { describe, expect, it } from "vitest";
import { bounds, pathData, pointAt, readScene } from "./scene";
const value = {
  format_version: 1,
  axes: { x: "Z", y: "diameter", y_scale: 2 },
  items: [
    {
      id: "arc",
      entity: "record",
      layer: "path",
      kind: "arc",
      start: [2, 0],
      end: [0, 2],
      center: [0, 0],
      sweep: Math.PI / 2,
      length: Math.PI,
    },
  ],
  path: ["arc"],
};
describe("通用二维场景", () => {
  it("支持折线按长度播放、点和安全文本数据", () => {
    const s = readScene({
      format_version: 1,
      axes: { x: "x", y: "y", y_scale: 1 },
      items: [
        {
          id: "path",
          entity: "row",
          layer: "shape",
          kind: "polyline",
          points: [
            [0, 0],
            [3, 0],
            [3, 4],
          ],
        },
        {
          id: "point",
          entity: "row",
          layer: "annotation",
          kind: "point",
          position: [1, 2],
        },
        {
          id: "label",
          entity: "row",
          layer: "annotation",
          kind: "text",
          position: [1, 2],
          text: "<script>literal text</script>",
        },
      ],
      path: ["path"],
    });
    expect(s.items[0].length).toBe(7);
    expect(pointAt(s.items[0], 0.5)).toEqual([3, 0.5]);
    expect(pathData(s.items[0])).toBe("M0,0 L3,0 L3,-4");
    expect(s.items[2].text).toBe("<script>literal text</script>");
    expect(() =>
      readScene({
        ...value,
        path: ["point"],
        items: [
          {
            id: "point",
            entity: "row",
            layer: "a",
            kind: "point",
            position: [0, 0],
          },
        ],
      }),
    ).toThrow();
  });
  it("保留解析圆弧和数学坐标方向", () => {
    const scene = readScene(value);
    expect(pointAt(scene.items[0], 0.5)[0]).toBeCloseTo(Math.sqrt(2));
    expect(pathData(scene.items[0])).toBe("M2,0 A2,2 0 0 0 0,-2");
    const b = bounds(scene);
    expect(b[0]).toBeLessThan(-2);
    expect(b[2]).toBeGreaterThan(4);
  });
  it("拒绝非法场景、重复身份及坏播放引用", () => {
    expect(() =>
      readScene({ ...value, items: [{ ...value.items[0], start: ["2", 0] }] }),
    ).toThrow();
    expect(() => readScene({ ...value, format_version: 2 })).toThrow();
    expect(() =>
      readScene({ ...value, items: [value.items[0], value.items[0]] }),
    ).toThrow();
    expect(() => readScene({ ...value, path: ["missing"] })).toThrow();
    expect(() =>
      readScene({ ...value, items: [{ ...value.items[0], end: [0, 3] }] }),
    ).toThrow();
    expect(() =>
      readScene({
        ...value,
        items: [{ ...value.items[0], start: [Infinity, 0] }],
      }),
    ).toThrow();
  });
});
