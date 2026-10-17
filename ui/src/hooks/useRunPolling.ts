import { useEffect, useRef } from "react";
import { api } from "../lib/api";
import type { Data, Run } from "../lib/data";

interface RunPollingCallbacks {
  onRuns: (update: (current: Record<string, Run>) => Record<string, Run>) => void;
  onRunList: (runs: Data[]) => void;
  onFailure: (error: Data) => void;
  onRunFailure: (error: Data) => void;
}

export function useRunPolling(runs: Record<string, Run>, callbacks: RunPollingCallbacks) {
  const latestCallbacks = useRef(callbacks);
  latestCallbacks.current = callbacks;

  // Poll lifecycle follows the pending run set, not every result object update.
  // Updating a run therefore cannot cancel the response currently being handled.
  const pending = Object.values(runs).filter((run) =>
    ["queued", "running"].includes(run.status),
  );
  const pendingSignature = pending
    .map((run) => `${run.id}:${run.status}`)
    .sort()
    .join("|");

  useEffect(() => {
    if (!pendingSignature) return;
    let cancelled = false;
    let timeout = 0;

    const poll = async () => {
      const polled: Run[] = [];
      let hadTerminal = false;
      for (const run of pending) {
        try {
          const fresh = await api("runs/" + run.id);
          if (cancelled) return;
          polled.push(fresh);
          if (!["queued", "running"].includes(fresh.status)) hadTerminal = true;
        } catch (error) {
          if (cancelled) return;
          latestCallbacks.current.onFailure(error);
        }
      }
      if (cancelled) return;

      if (hadTerminal) {
        try {
          const list = await api("runs");
          if (cancelled) return;
          latestCallbacks.current.onRunList(list.runs);
        } catch (error) {
          if (cancelled) return;
          latestCallbacks.current.onFailure(error);
        }
      }
      if (cancelled) return;
      for (const fresh of polled) {
        if (fresh.status === "failed")
          latestCallbacks.current.onRunFailure(fresh.error?.message || "任务失败");
      }
      if (polled.length)
        latestCallbacks.current.onRuns((current) => ({
          ...current,
          ...Object.fromEntries(polled.map((run) => [run.id, run])),
        }));

      // Failed GETs leave their runs pending and are retried on the next tick.
      if (pending.some((run) => ["queued", "running"].includes(run.status)))
        timeout = window.setTimeout(poll, 500);
    };

    timeout = window.setTimeout(poll, 500);
    return () => {
      cancelled = true;
      window.clearTimeout(timeout);
    };
  }, [pendingSignature]);
}
