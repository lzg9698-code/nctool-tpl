import { useEffect, useState } from "react";
import { action } from "./api";
import type { Data, Run } from "./data";

export function useManagedDependencies(run?: Run, enabled = true): { stale: boolean; message: string } {
  const [state, setState] = useState({ id: "", stale: false, message: "" });
  const replay = run?.result?.data.replay_input;
  const checkAction = replay?.job_snapshot ? run?.action.replace(/\.generate$/, ".check") : replay?.managed_snapshot ? "nc.managed.check" : replay?.process_snapshot ? "process.managed.check" : replay?.parameter_snapshot ? "parameter.dependencies.check" : undefined;
  useEffect(() => {
    if (!enabled || !run || run.status !== "succeeded" || !checkAction) return;
    let alive = true;
    async function check() {
      try {
        const input: Data = checkAction === "parameter.dependencies.check" ? { snapshot: replay.parameter_snapshot, template_snapshot: replay.snapshot } : replay;
        const r = await action(checkAction!, input);
        if (alive) setState({ id: run!.id, stale: !r.data.current, message: r.data.current ? "" : "参数、刀具或模板依赖已变化，请重新生成。" });
      } catch { if (alive) setState({ id: run!.id, stale: true, message: "无法核对当前参数依赖，请检查资产后重新生成。" }); }
    }
    void check(); const timer = window.setInterval(() => void check(), 3000);
    return () => { alive = false; window.clearInterval(timer); };
  }, [run?.id, run?.status, checkAction, enabled]);
  if (!enabled || !checkAction || run?.status !== "succeeded") return { stale: false, message: "" };
  return state.id === run.id ? state : { stale: true, message: "正在核对参数依赖…" };
}
