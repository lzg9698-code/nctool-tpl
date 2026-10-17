// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, render } from "@testing-library/react";
import { api } from "../lib/api";
import type { Run } from "../lib/data";
import { useRunPolling } from "./useRunPolling";

vi.mock("../lib/api", () => ({ api: vi.fn() }));
const queued: Run = {
  id: "run-1", title: "task", action: "render", input: {}, status: "queued", started_at: 0,
};

afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe("useRunPolling", () => {
  it.each(["succeeded", "failed", "cancelled", "interrupted"])(
    "refreshes history and stops polling after %s",
    async (status) => {
      vi.useFakeTimers();
      const onRuns = vi.fn();
      const onRunList = vi.fn();
      const onFailure = vi.fn();
      const onRunFailure = vi.fn();
      vi.mocked(api).mockResolvedValueOnce({ ...queued, status }).mockResolvedValueOnce({ runs: [queued] });
      const view = render(
        <StateOwner
          initial={{ [queued.id]: queued }}
          observe={{ onRuns, onRunList, onFailure, onRunFailure }}
        />,
      );
      await act(async () => vi.advanceTimersByTimeAsync(500));
      expect(api).toHaveBeenNthCalledWith(1, "runs/run-1");
      expect(api).toHaveBeenNthCalledWith(2, "runs");
      expect(onRuns).toHaveBeenCalledTimes(1);
      expect(onRunList).toHaveBeenCalledWith([queued]);
      expect(onRunFailure).toHaveBeenCalledTimes(status === "failed" ? 1 : 0);
      expect(vi.mocked(api).mock.calls).toHaveLength(2);
      view.unmount();
    },
  );

  it("refreshes the list and reports failed runs after state updates rerender the owner", async () => {
    vi.useFakeTimers();
    const current = { [queued.id]: queued };
    const owner = render(<StateOwner initial={current} />);
    vi.mocked(api)
      .mockResolvedValueOnce({ ...queued, status: "failed", error: { message: "failed run" } })
      .mockResolvedValueOnce({ runs: [{ ...queued, status: "failed" }] });
    await act(async () => vi.advanceTimersByTimeAsync(500));
    expect(screenText()).toBe("failed run");
    expect(api).toHaveBeenCalledTimes(2);
    owner.unmount();
  });

  it("ignores an in-flight response after unmount", async () => {
    vi.useFakeTimers();
    let release: (value: Run) => void = () => {};
    vi.mocked(api).mockReturnValue(new Promise((resolve) => { release = resolve; }));
    const onRuns = vi.fn();
    const view = render(<Harness runs={{ [queued.id]: queued }} callbacks={{ onRuns, onRunList: vi.fn(), onFailure: vi.fn(), onRunFailure: vi.fn() }} />);
    await act(async () => vi.advanceTimersByTimeAsync(500));
    view.unmount();
    await act(async () => release({ ...queued, status: "succeeded" }));
    expect(onRuns).not.toHaveBeenCalled();
    expect(api).toHaveBeenCalledTimes(1);
  });

  it("retries a failed run fetch on the next polling tick", async () => {
    vi.useFakeTimers();
    const onFailure = vi.fn();
    vi.mocked(api)
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce({ ...queued, status: "succeeded" })
      .mockResolvedValueOnce({ runs: [] });
    const view = render(<Harness runs={{ [queued.id]: queued }} callbacks={{ onRuns: vi.fn(), onRunList: vi.fn(), onFailure, onRunFailure: vi.fn() }} />);
    await act(async () => vi.advanceTimersByTimeAsync(500));
    expect(onFailure).toHaveBeenCalledTimes(1);
    await act(async () => vi.advanceTimersByTimeAsync(500));
    expect(api).toHaveBeenCalledTimes(3);
    view.unmount();
  });
});

function Harness({ runs, callbacks }: { runs: Record<string, Run>; callbacks: Parameters<typeof useRunPolling>[1] }) {
  useRunPolling(runs, callbacks);
  return null;
}

import { useState } from "react";
import { screen } from "@testing-library/react";
function StateOwner({ initial, observe }: { initial: Record<string, Run>; observe?: Parameters<typeof useRunPolling>[1] }) {
  const [runs, setRuns] = useState(initial);
  const [message, setMessage] = useState("");
  useRunPolling(runs, {
    onRuns: (update) => {
      setRuns(update);
      observe?.onRuns(update);
    },
    onRunList: (list) => {
      observe?.onRunList(list);
    },
    onFailure: (error) => observe?.onFailure(error),
    onRunFailure: (error) => {
      setMessage(String(error));
      observe?.onRunFailure(error);
    },
  });
  return <span>{message}</span>;
}
function screenText() { return screen.getByText("failed run").textContent; }
