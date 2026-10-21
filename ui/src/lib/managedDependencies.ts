import { useEffect, useState } from "react";
import { action } from "./api";
import { encode, parseData, type Data, type Run } from "./data";

type State = { stale: boolean; message: string };
interface Request { key: string; action: string; input: string }
const pending: State = { stale: true, message: "正在核对参数依赖…" };
const clear: State = { stale: false, message: "" };

export function managedDependencyRequest(run?: Run, workspaceId = "", enabled = true): Request | undefined {
  if (!enabled || run?.status !== "succeeded") return;
  const replay = run.result?.data.replay_input;
  if (!replay) return;
  const checkAction = replay.job_snapshot && run.action.endsWith(".generate")
    ? run.action.replace(/\.generate$/, ".check")
    : replay.parameter_snapshot ? "parameter.dependencies.check" : undefined;
  if (!checkAction) return;
  const input = encode(checkAction === "parameter.dependencies.check"
    ? { snapshot: replay.parameter_snapshot, template_snapshot: replay.snapshot }
    : replay);
  return { key: encode([workspaceId, run.id, checkAction, input]), action: checkAction, input };
}

// All consumers of the same run share one in-flight check and one poll timer.
export function createManagedDependencyChecks(check: (id: string, input: Data) => Promise<Data>) {
  type Entry = { request: Request; state: State; listeners: Set<() => void>; timer?: ReturnType<typeof setTimeout> };
  const entries = new Map<string, Entry>();
  const notify = (entry: Entry) => entry.listeners.forEach(listener => listener());
  async function poll(entry: Entry) {
    entry.state = pending;
    notify(entry);
    try {
      const result = await check(entry.request.action, parseData(entry.request.input));
      if (entries.get(entry.request.key) !== entry) return;
      entry.state = result?.data?.current === true ? clear : {
        stale: true, message: "参数或模板依赖已变化，请重新生成。",
      };
    } catch {
      if (entries.get(entry.request.key) !== entry) return;
      entry.state = { stale: true, message: "无法核对当前参数依赖，请检查资产后重新生成。" };
    }
    notify(entry);
    entry.timer = setTimeout(() => void poll(entry), 3000);
  }
  return {
    getSnapshot(key: string): State { return entries.get(key)?.state ?? pending; },
    subscribe(request: Request, listener: () => void) {
      let entry = entries.get(request.key);
      if (!entry) {
        entry = { request, state: pending, listeners: new Set([listener]) };
        entries.set(request.key, entry);
        void poll(entry);
      } else { entry.listeners.add(listener); }
      const subscribed = entry;
      let active = true;
      return () => {
        if (!active) return;
        active = false;
        subscribed.listeners.delete(listener);
        if (!subscribed.listeners.size && entries.get(request.key) === subscribed) {
          clearTimeout(subscribed.timer);
          entries.delete(request.key);
        }
      };
    },
  };
}
const checks = createManagedDependencyChecks(action);
export function useManagedDependencies(run?: Run, enabled = true, workspaceId = ""): State {
  const request = managedDependencyRequest(run, workspaceId, enabled);
  const [state, setState] = useState<{ key: string; value: State }>();
  const key = request?.key;
  useEffect(() => {
    if (!request) return;
    const update = () => setState({ key: request.key, value: checks.getSnapshot(request.key) });
    const unsubscribe = checks.subscribe(request, update);
    update();
    return unsubscribe;
  }, [key]);
  return !request ? clear : state?.key === request.key ? state.value : pending;
}
