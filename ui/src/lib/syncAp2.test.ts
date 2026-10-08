import { describe, expect, it, vi } from "vitest";
import { DraftNumber, numberText, parseData } from "./data";
import { fillSyncAp2 } from "./syncAp2";
import { initialTurning } from "./turningModel";

function draft() {
  const value = initialTurning("sync-turning");
  Object.assign(value.sides.right.rows[0], {
    sta_x: 60,
    sta_z: 0,
    end_x: 50,
    end_z: -10,
  });
  return value;
}
const valid = () =>
  Promise.resolve(
    parseData(
      '{"success":true,"generatable":true,"adopted_ap_2":3.00,"cut_num":2}',
    ),
  );

describe("同步 ap2 自动填写", () => {
  it("空值直接采用后端的精确采用值，不修改原草稿与空白行", async () => {
    const value = draft(),
      recommend = vi.fn(valid);
    const result = await fillSyncAp2(value, "all", recommend);
    expect(numberText(result.draft.sides.right.rows[0].ap_2)).toBe("3.00");
    expect(value.sides.right.rows[0].ap_2).toBeUndefined();
    expect(result.filled).toBe(1);
    expect(recommend).toHaveBeenCalledTimes(1);
    expect(result.draft.sides.left.rows[0].ap_2).toBeUndefined();
  });
  it("手填值、零、null 和不完整数字不被静默纠正", async () => {
    for (const ap_2 of [5, 0, null, new DraftNumber("1e")]) {
      const value = draft();
      value.sides.right.rows[0].ap_2 = ap_2;
      const recommend = vi.fn(valid),
        result = await fillSyncAp2(value, "right", recommend);
      expect(recommend).not.toHaveBeenCalled();
      expect(result.draft.sides.right.rows[0].ap_2).toEqual(ap_2);
    }
  });
  it("明确计算 ap2 会覆盖本侧值，另一侧保持原草稿", async () => {
    const value = draft();
    value.sides.right.rows[0].ap_2 = 5;
    const result = await fillSyncAp2(value, "right", valid, true);
    expect(numberText(result.draft.sides.right.rows[0].ap_2)).toBe("3.00");
    expect(result.draft.sides.left).toEqual(value.sides.left);
  });
  it("不可生成的舍入候选与请求失败不自动填写，保留逐行诊断", async () => {
    const value = draft();
    value.sides.right.rows.push({ id: "failure", sta_x: 70, end_x: 60 });
    const recommend = vi
      .fn()
      .mockResolvedValueOnce({
        success: true,
        generatable: false,
        adopted_ap_2: 0,
        diagnostic: "舍入后为零",
      })
      .mockRejectedValueOnce(new Error("范围错误"));
    const result = await fillSyncAp2(value, "right", recommend);
    expect(result.filled).toBe(0);
    expect(result.failures.map((f) => f.message)).toEqual([
      "舍入后为零",
      "范围错误",
    ]);
    expect(result.draft.sides.right.rows[0].ap_2).toBeUndefined();
  });
});
