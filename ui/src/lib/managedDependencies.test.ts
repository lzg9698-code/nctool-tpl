import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createManagedDependencyChecks,
  managedDependencyRequest,
} from "./managedDependencies";
import { encode, parseData, type Run } from "./data";

function run(replay = { parameter_snapshot: { fingerprint: "first" } }): Run {
  return {
    id: "run-1",
    title: "generation",
    action: "template.render.managed",
    input: {},
    status: "succeeded",
    started_at: 0,
    result: { data: { replay_input: replay }, artifacts: [], diagnostics: [] },
  };
}
function deferred() {
  let resolve!: (value: unknown) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise((ok, fail) => {
    resolve = ok;
    reject = fail;
  });
  return { promise, resolve, reject };
}
const current = { data: { current: true } };
const request = () => managedDependencyRequest(run(), "workspace-a")!;

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("shared managed dependency checks", () => {
  it("shares the initial request and each poll, and survives one subscriber leaving", async () => {
    const check = vi.fn().mockResolvedValue(current);
    const checks = createManagedDependencyChecks(check);
    const input = request();
    const first = vi.fn(),
      second = vi.fn();
    const offFirst = checks.subscribe(input, first);
    const offSecond = checks.subscribe(input, second);
    expect(check).toHaveBeenCalledTimes(1);
    expect(checks.getSnapshot(input.key).stale).toBe(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(checks.getSnapshot(input.key).stale).toBe(false);
    expect(first).toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
    offFirst();
    first.mockClear();
    await vi.advanceTimersByTimeAsync(3000);
    expect(check).toHaveBeenCalledTimes(2);
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(3);
    offSecond();
    expect(vi.getTimerCount()).toBe(0);
    await vi.advanceTimersByTimeAsync(9000);
    expect(check).toHaveBeenCalledTimes(2);
  });

  it("stays stale throughout a slow check and never overlaps a poll", async () => {
    const initial = deferred(),
      next = deferred();
    const check = vi
      .fn()
      .mockReturnValueOnce(initial.promise)
      .mockReturnValueOnce(next.promise);
    const checks = createManagedDependencyChecks(check);
    const input = request();
    const off = checks.subscribe(input, () => {});
    await vi.advanceTimersByTimeAsync(12000);
    expect(check).toHaveBeenCalledTimes(1);
    expect(checks.getSnapshot(input.key).stale).toBe(true);
    initial.resolve(current);
    await vi.advanceTimersByTimeAsync(0);
    expect(checks.getSnapshot(input.key).stale).toBe(false);
    await vi.advanceTimersByTimeAsync(3000);
    expect(check).toHaveBeenCalledTimes(2);
    expect(checks.getSnapshot(input.key).stale).toBe(true);
    await vi.advanceTimersByTimeAsync(12000);
    expect(check).toHaveBeenCalledTimes(2);
    off();
    next.resolve(current);
    await vi.advanceTimersByTimeAsync(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("drops a last-unsubscribe response and rechecks on remount of the same key", async () => {
    const old = deferred(),
      fresh = deferred();
    const check = vi
      .fn()
      .mockReturnValueOnce(old.promise)
      .mockReturnValueOnce(fresh.promise);
    const checks = createManagedDependencyChecks(check);
    const input = request();
    const oldListener = vi.fn(),
      newListener = vi.fn();
    const off = checks.subscribe(input, oldListener);
    off();
    off(); // Cleanup is idempotent and cannot affect the next mount.
    oldListener.mockClear();
    const offNew = checks.subscribe(input, newListener);
    expect(check).toHaveBeenCalledTimes(2);
    expect(checks.getSnapshot(input.key).stale).toBe(true);
    old.resolve(current);
    await vi.advanceTimersByTimeAsync(0);
    expect(checks.getSnapshot(input.key).stale).toBe(true);
    expect(oldListener).not.toHaveBeenCalled();
    expect(newListener).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
    fresh.resolve(current);
    await vi.advanceTimersByTimeAsync(0);
    expect(checks.getSnapshot(input.key).stale).toBe(false);
    offNew();
    expect(checks.getSnapshot(input.key).stale).toBe(true);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("marks errors and non-current checks stale, and allows a later successful check", async () => {
    const check = vi
      .fn()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce({ data: { current: false } })
      .mockResolvedValueOnce(current);
    const checks = createManagedDependencyChecks(check);
    const input = request();
    const off = checks.subscribe(input, () => {});
    await vi.advanceTimersByTimeAsync(0);
    expect(checks.getSnapshot(input.key)).toEqual({
      stale: true,
      message: "无法核对当前参数依赖，请检查资产后重新生成。",
    });
    await vi.advanceTimersByTimeAsync(3000);
    expect(checks.getSnapshot(input.key).stale).toBe(true);
    expect(checks.getSnapshot(input.key).message).toContain("已变化");
    await vi.advanceTimersByTimeAsync(3000);
    expect(checks.getSnapshot(input.key)).toEqual({
      stale: false,
      message: "",
    });
    off();
  });

  it("does not accept malformed current responses as permission to export", async () => {
    const check = vi.fn().mockResolvedValue({ data: { current: "false" } });
    const checks = createManagedDependencyChecks(check);
    const input = request();
    const off = checks.subscribe(input, () => {});
    await vi.advanceTimersByTimeAsync(0);
    expect(checks.getSnapshot(input.key).stale).toBe(true);
    off();
  });

  it("rejects pending checks after the same run receives changed replay input", async () => {
    const old = deferred(),
      fresh = deferred();
    const check = vi
      .fn()
      .mockReturnValueOnce(old.promise)
      .mockReturnValueOnce(fresh.promise);
    const checks = createManagedDependencyChecks(check);
    const original = request();
    const changed = managedDependencyRequest(
      run({ parameter_snapshot: { fingerprint: "new" } }),
      "workspace-a",
    )!;
    expect(changed.key).not.toBe(original.key);
    const off = checks.subscribe(original, () => {});
    off();
    const offChanged = checks.subscribe(changed, () => {});
    old.resolve(current);
    await vi.advanceTimersByTimeAsync(0);
    expect(checks.getSnapshot(changed.key).stale).toBe(true);
    expect(checks.getSnapshot(original.key).stale).toBe(true);
    fresh.reject(new Error("new input failed"));
    await vi.advanceTimersByTimeAsync(0);
    expect(checks.getSnapshot(changed.key).message).toContain("无法核对");
    offChanged();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("isolates identical run IDs and input across workspaces", async () => {
    const check = vi
      .fn()
      .mockResolvedValueOnce(current)
      .mockResolvedValueOnce({ data: { current: false } });
    const checks = createManagedDependencyChecks(check);
    const a = request(),
      b = managedDependencyRequest(run(), "workspace-b")!;
    const offA = checks.subscribe(a, () => {}),
      offB = checks.subscribe(b, () => {});
    await vi.advanceTimersByTimeAsync(0);
    expect(check).toHaveBeenCalledTimes(2);
    expect(checks.getSnapshot(a.key).stale).toBe(false);
    expect(checks.getSnapshot(b.key).stale).toBe(true);
    offA();
    expect(vi.getTimerCount()).toBe(1);
    offB();
  });
});

describe("managed check identity and eligibility", () => {
  it("uses input content instead of Run identity and retains exact numeric snapshot input", () => {
    const input = run();
    expect(managedDependencyRequest({ ...input }, "workspace-a")!.key).toBe(
      request().key,
    );
    const replay = parseData(
      '{"parameter_snapshot":{"value":9007199254740993},"snapshot":{"version":1}}',
    );
    input.result!.data.replay_input = replay;
    const parameter = managedDependencyRequest(input, "workspace-a")!;
    expect(parameter.action).toBe("parameter.dependencies.check");
    expect(parameter.input).toBe(
      '{"snapshot":{"value":9007199254740993},"template_snapshot":{"version":1}}',
    );
    expect(encode(parseData(parameter.input))).toBe(parameter.input);
  });

  it("stops checking for disabled, missing, pending and failed runs and notices later success", () => {
    expect(managedDependencyRequest(undefined, "workspace-a")).toBeUndefined();
    expect(
      managedDependencyRequest(run(), "workspace-a", false),
    ).toBeUndefined();
    for (const status of [
      "queued",
      "running",
      "failed",
      "cancelled",
      "interrupted",
    ])
      expect(
        managedDependencyRequest({ ...run(), status }, "workspace-a"),
      ).toBeUndefined();
    expect(
      managedDependencyRequest({ ...run(), result: undefined }, "workspace-a"),
    ).toBeUndefined();
    expect(managedDependencyRequest(run(), "workspace-a")).toBeDefined();
    expect(
      managedDependencyRequest({ ...run(), id: "next-run" }, "workspace-a")!
        .key,
    ).not.toBe(request().key);
  });

  it("checks generic job and shared parameter snapshots and ignores retired machining snapshots", () => {
    for (const [snapshot, action, expected] of [
      ["job_snapshot", "report.generate", "report.check"],
      ["parameter_snapshot", "template.render.managed", "parameter.dependencies.check"],
    ]) {
      const input = run();
      input.action = action;
      input.result!.data.replay_input = { [snapshot]: { version: 1 } };
      expect(managedDependencyRequest(input, "workspace-a")!.action).toBe(expected);
    }
    for (const [snapshot, action] of [
      ["managed_snapshot", "nc.generate.managed"],
      ["process_snapshot", "process.generate.managed"],
    ]) {
      const input = run();
      input.action = action;
      input.result!.data.replay_input = { [snapshot]: { version: 1 } };
      expect(
        managedDependencyRequest(input, "workspace-a"),
      ).toBeUndefined();
    }
  });
});
