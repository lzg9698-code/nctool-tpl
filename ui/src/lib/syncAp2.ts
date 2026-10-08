import { cloneDraft } from "./drafts";
import { encode, type Data } from "./data";

export function ap2Conditions(row: Data, settings: Data): string {
  try {
    return encode([
      row.sta_x,
      row.end_x,
      settings.ap_1,
      settings.ap2_min,
      settings.ap2_max,
    ]);
  } catch {
    return "invalid";
  }
}

// The plugin computes and validates adopted values; the UI never rounds candidates.
export async function fillSyncAp2(
  draft: Data,
  target: string,
  recommend: (input: Data) => Promise<Data>,
  replace = false,
) {
  const next = cloneDraft(draft),
    recommendations: Data = {};
  const failures: { side: string; index: number; message: string }[] = [];
  let filled = 0;
  for (const side of target === "all" ? ["right", "left"] : [target]) {
    const settings = next.sides[side];
    if (!settings) continue;
    recommendations[side] = {};
    for (let index = 0; index < settings.rows.length; index++) {
      const row = settings.rows[index];
      const empty = (value: Data) => value === undefined || value === "";
      if (
        ["sta_x", "sta_z", "end_x", "end_z", "ap_2"].every((key) =>
          empty(row[key]),
        )
      )
        continue;
      if (!replace && !empty(row.ap_2)) continue;
      let result: Data;
      try {
        result = await recommend({
          sta_x: row.sta_x,
          end_x: row.end_x,
          ap_1: settings.ap_1,
          ap2_min: settings.ap2_min,
          ap2_max: settings.ap2_max,
        });
      } catch (error) {
        result = {
          success: false,
          generatable: false,
          diagnostic: (error as Error).message,
        };
      }
      recommendations[side][row.id] = {
        ...result,
        conditions: ap2Conditions(row, settings),
      };
      if (result.success && result.generatable) {
        row.ap_2 = result.adopted_ap_2;
        filled++;
      } else
        failures.push({
          side,
          index,
          message:
            result.diagnostic ||
            "没有有效的 ap2 推荐值，请调整范围或手动输入。",
        });
    }
  }
  return { draft: next, recommendations, failures, filled };
}
