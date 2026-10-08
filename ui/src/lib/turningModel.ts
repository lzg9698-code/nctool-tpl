import { DraftNumber, encode, numberText, type Data } from "./data";
import { cloneDraft } from "./drafts";
export type TurningKind = "sync-turning" | "undercut";
export function emptyRow(kind: TurningKind): Data { return { id: crypto.randomUUID(), ...(kind === "undercut" ? { gtype: "ES" } : {}) }; }
export function initialTurning(kind: TurningKind): Data {
  const side = () => ({ rows: [emptyRow(kind)], trajectory: { preset_id: "v3-compatible" }, ...(kind === "sync-turning" ? { start_sync_num: 1, ap_1: 5, ap2_min: 0, ap2_max: 8 } : {}) });
  return { target: "all", sides: { right: side(), left: side() }, ...(kind === "sync-turning" ? { sync_feed: 0.5 } : { safety_dis: 2 }) };
}
export function turningRequest(draft: Data, kind: TurningKind, target: string): Data {
  const next = cloneDraft(draft); next.target = target;
  if (target !== "all") for (const side of Object.keys(next.sides)) if (side !== target) delete next.sides[side];
  delete next.template_origin;
  for (const side of Object.values(next.sides) as Data[]) {
    side.rows = side.rows.map((row: Data) => {
      const raw = { ...row }; delete raw.es_draft;
      for (const k of Object.keys(raw)) if (raw[k] === undefined || raw[k] === "") delete raw[k];
      if (kind === "undercut") {
        if (raw.gtype === "FS") for (const k of ["d2", "c", "angle"]) delete raw[k];
        if (Object.keys(raw).every(k => ["id", "gtype"].includes(k))) return { id: raw.id };
      }
      return raw;
    });
  }
  encode(next); return next;
}
export function changeGrooveType(row: Data, gtype: string): Data {
  const next = { ...row, gtype, std_key: "" };
  if (gtype === "FS") { next.es_draft = Object.fromEntries(["d2", "c", "angle"].map(k => [k, row[k]])); for (const k of ["d2", "c", "angle"]) delete next[k]; }
  else { Object.assign(next, row.es_draft || {}); delete next.es_draft; }
  return next;
}
export function importTurning(value: Data, kind: TurningKind): { input: Data; notes: string[] } {
  if (value?.format_version !== undefined) {
    if (numberText(value.format_version) !== "1" || !value.input?.sides || typeof value.input.sides !== "object") throw new Error("不支持的会话版本或缺少侧别输入");
    const input = { ...initialTurning(kind), ...value.input, sides: { ...initialTurning(kind).sides, ...value.input.sides } };
    for (const side of ["right", "left"]) if (!Array.isArray(input.sides[side]?.rows) || input.sides[side].rows.some((row: Data) => !row || typeof row !== "object" || Array.isArray(row) || typeof row.id !== "string")) throw new Error("会话中的侧别或行结构无效");
    return { input, notes: ["读取专用插件会话；未导入旧生成结果。"] };
  }
  const input = initialTurning(kind), notes = ["V3 原始输入转为会话；未导入旧输出、行内旧刀数或共享默认。"];
  const num = (value: Data) => value === undefined || value === "" ? undefined : new DraftNumber(String(value));
  if (kind === "sync-turning") {
    input.sync_feed = num(value.feed);
    for (const side of ["right", "left"]) {
      const v = value[side]; if (!v) throw new Error("缺少 V3 " + side + " 配置");
      input.sides[side] = { ...input.sides[side], start_sync_num: num(v.sync_start), ap_1: num(v.ap1_fixed), ap2_min: num(v.ap2_min), ap2_max: num(v.ap2_max), rows: (v.steps || []).map((r: Data) => ({ id: crypto.randomUUID(), ...Object.fromEntries(["sta_x", "sta_z", "end_x", "end_z", "ap_2"].map(k => [k, num(r[k])])) })) };
    }
  } else {
    input.raw_diameter = num(value.common?.raw_diameter); input.safety_dis = num(value.common?.safety_dis);
    for (const side of ["right", "left"]) input.sides[side].rows = (value[side]?.grooves || []).map((r: Data) => ({ id: crypto.randomUUID(), gtype: r.type, std_key: r.std, d1: num(r.d1), z: num(r.z), ...(r.type === "ES" ? { d2: num(r.d2), c: num(r.c), angle: num(r.ang) } : {}) }));
  }
  turningRequest(input, kind, "all"); return { input, notes };
}
