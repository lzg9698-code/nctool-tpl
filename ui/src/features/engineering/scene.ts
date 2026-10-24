import { numericText, type Data } from "../../lib/json";
import { isLosslessNumber } from "lossless-json";
export interface SceneItem {
  id: string;
  entity: string;
  layer: string;
  kind: "line" | "arc" | "polyline" | "point" | "text";
  points?: [number, number][];
  text?: string;
  start: [number, number];
  end: [number, number];
  center?: [number, number];
  sweep: number;
  length: number;
}
export interface Scene {
  items: SceneItem[];
  path: string[];
  axes: { x: string; y: string; y_scale: number };
}
const number = (v: Data) => {
  if (typeof v !== "number" && !isLosslessNumber(v))
    throw Error("场景数值类型无效");
  const n = Number(numericText(v));
  if (v == null || !Number.isFinite(n) || Math.abs(n) > 1e12)
    throw Error("场景数值无效");
  return n;
};
const point = (v: Data): [number, number] => {
  if (!Array.isArray(v) || v.length !== 2) throw Error("场景坐标无效");
  return [number(v[0]), number(v[1])];
};
export function readScene(value: Data): Scene {
  if (
    !value ||
    number(value.format_version) !== 1 ||
    !Array.isArray(value.items) ||
    value.items.length > 10000 ||
    !Array.isArray(value.path)
  )
    throw Error("二维场景版本或规模无效");
  const ids = new Set<string>();
  let pointCount = 0;
  const items = value.items.map((v: Data): SceneItem => {
    if (
      typeof v.id !== "string" ||
      !v.id ||
      ids.has(v.id) ||
      !["line", "arc", "polyline", "point", "text"].includes(v.kind) ||
      typeof v.entity !== "string" ||
      typeof v.layer !== "string"
    )
      throw Error("场景元素无效");
    ids.add(v.id);
    const points: [number, number][] | undefined =
      v.kind === "polyline" && Array.isArray(v.points)
        ? v.points.map(point)
        : undefined;
    if (v.kind === "polyline" && (!points || points.length < 2))
      throw Error("折线至少需要两个点");
    pointCount += points?.length || 2;
    if (pointCount > 100000) throw Error("场景坐标总量超过限制");
    const annotation = v.kind === "point" || v.kind === "text";
    if (
      v.kind === "text" &&
      (typeof v.text !== "string" || v.text.length > 1024)
    )
      throw Error("场景标注无效");
    const start = points ? points[0] : point(annotation ? v.position : v.start),
      end = points
        ? points[points.length - 1]
        : annotation
          ? start
          : point(v.end),
      center =
        v.kind !== "arc" || v.center == null ? undefined : point(v.center),
      sweep = v.kind === "arc" ? number(v.sweep) : 0,
      length = annotation
        ? 0
        : points
          ? points
              .slice(1)
              .reduce(
                (sum, p, i) =>
                  sum + Math.hypot(p[0] - points[i][0], p[1] - points[i][1]),
                0,
              )
          : number(v.length);
    if (
      length < 0 ||
      (v.kind === "arc" &&
        (!center || Math.abs(sweep) < 1e-10 || Math.abs(sweep) >= 2 * Math.PI))
    )
      throw Error("场景圆弧无效");
    if (
      center &&
      (Math.hypot(start[0] - center[0], start[1] - center[1]) === 0 ||
        Math.abs(
          Math.hypot(start[0] - center[0], start[1] - center[1]) -
            Math.hypot(end[0] - center[0], end[1] - center[1]),
        ) > 1e-6)
    )
      throw Error("场景圆弧半径不一致");
    return {
      id: v.id,
      entity: v.entity,
      layer: v.layer,
      kind: v.kind,
      start,
      end,
      center,
      sweep,
      length,
      ...(points ? { points } : {}),
      ...(v.kind === "text" ? { text: v.text } : {}),
    };
  });
  if (
    value.path.some(
      (id: Data) =>
        typeof id !== "string" ||
        !ids.has(id) ||
        items.some(
          (item: SceneItem) =>
            item.id === id && ["point", "text"].includes(item.kind),
        ),
    ) ||
    new Set(value.path).size !== value.path.length
  )
    throw Error("播放路径无效");
  const axes = value.axes;
  if (
    !axes ||
    typeof axes.x !== "string" ||
    typeof axes.y !== "string" ||
    number(axes.y_scale) <= 0
  )
    throw Error("坐标轴声明无效");
  return {
    items,
    path: value.path,
    axes: { x: axes.x, y: axes.y, y_scale: number(axes.y_scale) },
  };
}
export function pointAt(s: SceneItem, t: number): [number, number] {
  if (s.points) {
    let distance = s.length * t;
    for (let i = 1; i < s.points.length; i++) {
      const a = s.points[i - 1],
        b = s.points[i],
        length = Math.hypot(b[0] - a[0], b[1] - a[1]);
      if (distance <= length)
        return length
          ? [
              a[0] + ((b[0] - a[0]) * distance) / length,
              a[1] + ((b[1] - a[1]) * distance) / length,
            ]
          : a;
      distance -= length;
    }
    return s.end;
  }
  if (!s.center)
    return [
      s.start[0] + (s.end[0] - s.start[0]) * t,
      s.start[1] + (s.end[1] - s.start[1]) * t,
    ];
  const [x, y] = s.center,
    r = Math.hypot(s.start[0] - x, s.start[1] - y),
    a = Math.atan2(s.start[1] - y, s.start[0] - x) + s.sweep * t;
  return [x + r * Math.cos(a), y + r * Math.sin(a)];
}
export function pathData(s: SceneItem): string {
  if (s.points)
    return s.points.map(([x, y], i) => `${i ? "L" : "M"}${x},${-y}`).join(" ");
  const [x, y] = s.start,
    [ex, ey] = s.end;
  if (!s.center) return `M${x},${-y} L${ex},${-ey}`;
  const r = Math.hypot(x - s.center[0], y - s.center[1]);
  return `M${x},${-y} A${r},${r} 0 ${Math.abs(s.sweep) > Math.PI ? 1 : 0} ${s.sweep < 0 ? 1 : 0} ${ex},${-ey}`;
}
export function bounds(scene: Scene): [number, number, number, number] {
  if (!scene.items.length) return [-10, -10, 20, 20];
  const pts = scene.items.flatMap((s) =>
    s.center
      ? [
          s.start,
          s.end,
          [
            s.center[0] -
              Math.hypot(s.start[0] - s.center[0], s.start[1] - s.center[1]),
            s.center[1] -
              Math.hypot(s.start[0] - s.center[0], s.start[1] - s.center[1]),
          ],
          [
            s.center[0] +
              Math.hypot(s.start[0] - s.center[0], s.start[1] - s.center[1]),
            s.center[1] +
              Math.hypot(s.start[0] - s.center[0], s.start[1] - s.center[1]),
          ],
        ]
      : s.points || [s.start, s.end],
  );
  let x = Infinity,
    y = Infinity,
    maxX = -Infinity,
    maxY = -Infinity;
  for (const p of pts) {
    x = Math.min(x, p[0]);
    y = Math.min(y, p[1]);
    maxX = Math.max(maxX, p[0]);
    maxY = Math.max(maxY, p[1]);
  }
  const w = maxX - x,
    h = maxY - y,
    pad = Math.max(w, h, 1) * 0.08;
  return [x - pad, -y - h - pad, w + 2 * pad, h + 2 * pad];
}
