import { describe, expect, it } from "vitest";
import { DraftNumber, encode, numberText, parseData } from "./data";
import { changeGrooveType, importTurning, initialTurning, turningRequest } from "./turningModel";
describe("专用车削原始输入", () => {
  it("空槽不参与、部分槽保留、FS 不提交 ES 草稿", () => {
    const d = initialTurning("undercut");
    const row = d.sides.right.rows[0]; row.d1 = 80; row.d2 = 85; row.c = 2; row.angle = 45;
    d.sides.right.rows[0] = changeGrooveType(row, "FS");
    const request = turningRequest(d, "undercut", "right");
    expect(request.sides.right.rows[0].d1).toBe(80);
    expect(request.sides.right.rows[0]).not.toHaveProperty("d2");
    expect(request.sides.left).toBeUndefined();
    expect(turningRequest(d, "undercut", "all").sides.left.rows[0]).toEqual({ id: d.sides.left.rows[0].id });
    expect(changeGrooveType(d.sides.right.rows[0], "ES").d2).toBe(85);
  });
  it("不完整数字保留草稿但不得生成", () => {
    const d = initialTurning("sync-turning"); d.sides.right.rows[0].sta_x = new DraftNumber("1e");
    expect(() => turningRequest(d, "sync-turning", "right")).toThrow();
  });
  it("真实 JSON 会话版本和数字无损导入，缺失侧别补空表单", () => {
    const value = parseData('{"format_version":1,"input":{"sync_feed":0.2,"sides":{"right":{"rows":[{"id":"exact","sta_x":60.000000000000001}]}}}}');
    const imported = importTurning(value, "sync-turning");
    expect(numberText(imported.input.sides.right.rows[0].sta_x)).toBe("60.000000000000001");
    expect(imported.input.sides.left.rows).toHaveLength(1);
    expect(() => importTurning(parseData('{"format_version":2,"input":{}}'), "sync-turning")).toThrow();
    expect(() => importTurning({format_version:1,input:{sides:{right:{rows:[null]}}}}, "sync-turning")).toThrow();
  });
  it("V3 导入只带原始尺寸，不引入旧刀数或输出", () => {
    const side = { sync_start: "21", ap1_fixed: "5", ap2_min: "0", ap2_max: "8", steps: [{ sta_x: "60", end_x: "50", sta_z: "0", end_z: "-10", ap_1: "999", ap_2: "5" }], output: "OLD" };
    const imported = importTurning({ feed: "0.5", right: side, left: side }, "sync-turning");
    const request = turningRequest(imported.input, "sync-turning", "all");
    expect(encode(request)).not.toContain("999"); expect(encode(request)).not.toContain("OLD");
    expect(numberText(request.sides.right.rows[0].sta_x)).toBe("60");
  });
});
