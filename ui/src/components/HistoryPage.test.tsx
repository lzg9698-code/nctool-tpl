// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { HistoryPage, type HistoryComparison } from "./HistoryPage";
import { api } from "../lib/api";
import type { Data, Run } from "../lib/data";

vi.mock("../lib/api", () => ({ api: vi.fn(), action: vi.fn() }));
vi.mock("./LazyEditor", () => ({
  Editor: ({ value }: { value: string }) => <pre>{value}</pre>,
}));
vi.mock("./DiffView", () => ({
  DiffView: ({
    beforeTitle,
    afterTitle,
  }: {
    beforeTitle: string;
    afterTitle: string;
  }) => (
    <div>
      历史比较：{beforeTitle}→{afterTitle}
    </div>
  ),
}));

// A replay-check that never settles keeps the effect from mutating state after
// render, so the interaction tests stay free of act() warnings.
const pendingReplayCheck = new Promise<Data>(() => {});

function makeRun(id: string, overrides: Partial<Run> = {}): Run {
  return {
    id,
    title: "记录 " + id,
    action: "template.render",
    input: {},
    status: "succeeded",
    started_at: 0,
    result: {
      data: { text: "text-" + id },
      artifacts: [
        { name: "a.txt", media_type: "text/plain", text: "text-" + id },
      ],
      diagnostics: [],
    },
    ...overrides,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function markup(action: string, available: string[]) {
  const run: Run = {
    id: "r1",
    title: "旧记录",
    action,
    input: {},
    status: "succeeded",
    started_at: 0,
    result: {
      data: { text: "saved output" },
      artifacts: [
        { name: "saved.txt", media_type: "text/plain", text: "saved output" },
        { name: "second.txt", media_type: "text/plain", text: "second output" },
      ],
      diagnostics: [],
    },
  };
  return renderToStaticMarkup(
    <HistoryPage
      actions={available.map((id) => ({ id }))}
      runList={[run, { ...run, id: "r0" }]}
      selectedHistory={run}
      comparison={null}
      setRunList={() => {}}
      setSelectedHistory={() => {}}
      setComparison={() => {}}
      restoreRun={vi.fn()}
      onCreated={() => {}}
      notify={() => {}}
      fail={() => {}}
    />,
  );
}

beforeEach(() => {
  vi.mocked(api).mockReset();
  vi.mocked(api).mockImplementation(async (path) => {
    if (path === "runs/query") return { runs: [], total: 0, offset: 0, limit: 25 };
    if (path.endsWith("/replay-check")) return pendingReplayCheck;
    throw new Error(`Unexpected API call: ${path}`);
  });
});
afterEach(cleanup);

function Harness({
  runs,
  selected,
  fail = () => {},
}: {
  runs: Run[];
  selected: Run;
  fail?: (error: Data) => void;
}) {
  const [runList, setRunList] = useState<Data[]>(runs);
  const [selectedHistory, setSelectedHistory] = useState<Run | null>(selected);
  const [comparison, setComparison] = useState<HistoryComparison | null>(null);
  return (
    <HistoryPage
      actions={[{ id: "template.render" }]}
      runList={runList}
      selectedHistory={selectedHistory}
      comparison={comparison}
      setRunList={setRunList}
      setSelectedHistory={setSelectedHistory}
      setComparison={setComparison}
      restoreRun={vi.fn()}
      onCreated={() => {}}
      notify={() => {}}
      fail={fail}
    />
  );
}

describe("history controls follow current providers", () => {
  it("disables rerun and restore while retaining historical text, artifacts and comparison", () => {
    const html = markup("external.render", []);
    expect(html).toMatch(
      /<button[^>]*disabled=""[^>]*>按历史输入重新执行<\/button>/,
    );
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>恢复输入<\/button>/);
    expect(html).toContain("saved output");
    expect(html).toContain("当前没有可用提供方");
    expect(html).toContain("比较历史记录");
    expect(html).toMatch(/<button[^>]*>[^<]*导出 second.txt<\/button>/);
    expect(html).not.toMatch(
      /<button[^>]*disabled=""[^>]*>[^<]*导出 second.txt/,
    );
  });
  it("keeps retired machining history read-only even when its action is available", () => {
    const html = markup("nc.generate", ["nc.generate"]);
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>恢复输入<\/button>/);
    expect(html).toMatch(
      /<button[^>]*disabled=""[^>]*>按历史输入重新执行<\/button>/,
    );
    expect(html).toContain("已移除的加工");
  });
  it("disables missing external actions too, and enables generic managed recovery", () => {
    expect(markup("external.render", [])).toContain("当前没有可用提供方");
    const html = markup("template.render.managed", ["template.render.managed"]);
    expect(html).not.toMatch(/<button[^>]*disabled=""[^>]*>恢复输入<\/button>/);
    expect(html).not.toMatch(
      /<button[^>]*disabled=""[^>]*>按历史输入重新执行<\/button>/,
    );
  });
});

describe("history query and deletion", () => {
  it("requests searchable pages across the 100 record boundary and filters status", async () => {
    const runs = Array.from({ length: 25 }, (_, index) => makeRun("r" + (100 + index)));
    const apiCalls: { path: string; body?: Data }[] = [];
    vi.mocked(api).mockImplementation(async (path, body) => {
      apiCalls.push({ path, body });
      if (path === "runs/query") return { runs, total: 125, offset: body.offset, limit: 25 };
      if (path.endsWith("/replay-check")) return pendingReplayCheck;
      throw new Error(`Unexpected API call: ${path}`);
    });
    render(<Harness runs={[]} selected={makeRun("selected")} />);
    fireEvent.click(screen.getByRole("button", { name: /返回记录列表/ }));
    await waitFor(() => expect(apiCalls.some((call) => call.path === "runs/query")).toBe(true));
    fireEvent.change(screen.getByLabelText("搜索历史记录"), { target: { value: "target-action" } });
    await waitFor(() => expect(apiCalls.some((call) => call.body?.query === "target-action")).toBe(true));
    fireEvent.change(screen.getByLabelText("历史记录状态"), { target: { value: "failed" } });
    await waitFor(() => expect(apiCalls.some((call) => call.body?.status === "failed")).toBe(true));
  });

  it("ignores an older page reply after a newer search", async () => {
    const older = deferred<Data>();
    const apiCalls: { body?: Data }[] = [];
    vi.mocked(api).mockImplementation(async (path, body) => {
      if (path === "runs/query") {
        apiCalls.push({ body });
        return body.query === "old" ? older.promise : { runs: [makeRun("new")], total: 1 };
      }
      if (path.endsWith("/replay-check")) return pendingReplayCheck;
      throw new Error(`Unexpected API call: ${path}`);
    });
    render(<Harness runs={[]} selected={makeRun("selected")} />);
    fireEvent.click(screen.getByRole("button", { name: /返回记录列表/ }));
    fireEvent.change(screen.getByLabelText("搜索历史记录"), { target: { value: "old" } });
    fireEvent.change(screen.getByLabelText("搜索历史记录"), { target: { value: "new" } });
    await screen.findByText("记录 new");
    await act(async () => { older.resolve({ runs: [makeRun("old")], total: 1 }); });
    expect(screen.getByText("记录 new")).toBeTruthy();
    expect(screen.queryByText("记录 old")).toBeNull();
  });

  it("confirms one terminal deletion and preserves selection when deletion fails", async () => {
    const fail = vi.fn();
    vi.mocked(api).mockImplementation(async (path) => {
      if (path === "runs/query") return { runs: [makeRun("delete-me")], total: 1 };
      if (path === "runs/delete-me/remove") throw new Error("cannot delete");
      if (path.endsWith("/replay-check")) return pendingReplayCheck;
      throw new Error(`Unexpected API call: ${path}`);
    });
    render(<HistoryPage actions={[]} runList={[makeRun("delete-me")]} selectedHistory={null} comparison={null} setRunList={() => {}} setSelectedHistory={() => {}} setComparison={() => {}} restoreRun={vi.fn()} onCreated={() => {}} notify={vi.fn()} fail={fail} />);
    fireEvent.click(await screen.findByRole("button", { name: "删除记录 记录 delete-me" }));
    expect(await screen.findByText(/历史输入与产物不可恢复/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "删除记录" }));
    await waitFor(() => expect(fail).toHaveBeenCalled());
    expect(screen.getByText("记录 delete-me")).toBeTruthy();
    expect(screen.getByText(/历史输入与产物不可恢复/)).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("cannot delete");
  });

  it("does not issue a delete for a running record", async () => {
    const running = makeRun("busy", { status: "running" });
    vi.mocked(api).mockImplementation(async (path) => {
      if (path === "runs/query") return { runs: [running], total: 1 };
      if (path.endsWith("/replay-check")) return pendingReplayCheck;
      throw new Error(`Unexpected API call: ${path}`);
    });
    render(<HistoryPage actions={[]} runList={[running]} selectedHistory={null} comparison={null} setRunList={() => {}} setSelectedHistory={() => {}} setComparison={() => {}} restoreRun={vi.fn()} onCreated={() => {}} notify={vi.fn()} fail={vi.fn()} />);
    expect((await screen.findByRole("button", { name: "删除记录 记录 busy" }) as HTMLButtonElement).disabled).toBe(true);
    expect(vi.mocked(api).mock.calls.some(([path]) => String(path).endsWith("/remove"))).toBe(false);
  });
});

describe("history comparison request ordering", () => {
  it("renders the diff for the selected baseline record", async () => {
    const runs = [makeRun("r0"), makeRun("r2")];
    vi.mocked(api).mockImplementation(async (path) => {
      if (path === "runs/query") return { runs, total: runs.length };
      if (path === "runs/r0") return runs[0];
      if (path.endsWith("/replay-check")) return pendingReplayCheck;
      throw new Error(`Unexpected API call: ${path}`);
    });
    render(<Harness runs={runs} selected={makeRun("r1")} />);
    fireEvent.change(screen.getByLabelText("比较历史记录"), {
      target: { value: "r0" },
    });
    expect(await screen.findByText("历史比较：记录 r0→记录 r1")).toBeTruthy();
  });

  it("keeps the last chosen baseline when replies arrive out of order", async () => {
    const runs = [makeRun("r0"), makeRun("r2")];
    const older = deferred<Run>();
    const newer = deferred<Run>();
    vi.mocked(api).mockImplementation(async (path) => {
      if (path === "runs/query") return { runs, total: runs.length };
      if (path === "runs/r0") return older.promise;
      if (path === "runs/r2") return newer.promise;
      if (path.endsWith("/replay-check")) return pendingReplayCheck;
      throw new Error(`Unexpected API call: ${path}`);
    });
    render(<Harness runs={runs} selected={makeRun("r1")} />);
    const select = screen.getByLabelText("比较历史记录");
    fireEvent.change(select, { target: { value: "r0" } });
    fireEvent.change(select, { target: { value: "r2" } });
    // The newer choice settles first; the abandoned one settles last.
    await act(async () => {
      newer.resolve(runs[1]);
    });
    await act(async () => {
      older.resolve(runs[0]);
    });
    expect(screen.getByText("历史比较：记录 r2→记录 r1")).toBeTruthy();
    expect(screen.queryByText("历史比较：记录 r0→记录 r1")).toBeNull();
  });

  it("reports a failed baseline load and shows no diff", async () => {
    const fail = vi.fn();
    const runs = [makeRun("r0"), makeRun("r2")];
    vi.mocked(api).mockImplementation(async (path) => {
      if (path === "runs/query") return { runs, total: runs.length };
      if (path === "runs/r0") throw new Error("boom");
      if (path.endsWith("/replay-check")) return pendingReplayCheck;
      throw new Error(`Unexpected API call: ${path}`);
    });
    render(<Harness runs={runs} selected={makeRun("r1")} fail={fail} />);
    fireEvent.change(screen.getByLabelText("比较历史记录"), {
      target: { value: "r0" },
    });
    await waitFor(() => expect(fail).toHaveBeenCalled());
    expect(screen.queryByText(/历史比较：/)).toBeNull();
  });

  it("does not revive a stale comparison after switching records", async () => {
    const runs = [makeRun("r0"), makeRun("r2"), makeRun("r3")];
    const pending = deferred<Run>();
    vi.mocked(api).mockImplementation(async (path) => {
      if (path === "runs/query") return { runs, total: runs.length };
      if (path === "runs/r0") return pending.promise;
      if (path === "runs/r3") return runs[2];
      if (path.endsWith("/replay-check")) return pendingReplayCheck;
      throw new Error(`Unexpected API call: ${path}`);
    });
    render(<Harness runs={runs} selected={makeRun("r1")} />);
    fireEvent.change(screen.getByLabelText("比较历史记录"), {
      target: { value: "r0" },
    });
    // Leave the detail view, then open a different record.
    fireEvent.click(screen.getByRole("button", { name: /返回记录列表/ }));
    fireEvent.click((await screen.findAllByRole("button", { name: /记录 r3/ }))[0]);
    await waitFor(() => expect(screen.getByText("记录 r3")).toBeTruthy());
    await act(async () => {
      pending.resolve(runs[0]);
    });
    expect(screen.queryByText(/历史比较：/)).toBeNull();
  });

  it("ignores a comparison reply that arrives after unmount", async () => {
    const setComparison = vi.fn();
    const pending = deferred<Run>();
    vi.mocked(api).mockImplementation(async (path) => {
      if (path === "runs/query") return { runs: [makeRun("r0"), makeRun("r2")], total: 2 };
      if (path === "runs/r0") return pending.promise;
      if (path.endsWith("/replay-check")) return pendingReplayCheck;
      throw new Error(`Unexpected API call: ${path}`);
    });
    const { unmount } = render(
      <HistoryPage
        actions={[{ id: "template.render" }]}
        runList={[makeRun("r0"), makeRun("r2")]}
        selectedHistory={makeRun("r1")}
        comparison={null}
        setRunList={() => {}}
        setSelectedHistory={() => {}}
        setComparison={setComparison}
        restoreRun={vi.fn()}
        onCreated={() => {}}
        notify={() => {}}
        fail={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText("比较历史记录"), {
      target: { value: "r0" },
    });
    unmount();
    await act(async () => {
      pending.resolve(makeRun("r0"));
    });
    expect(setComparison).not.toHaveBeenCalled();
  });
});

describe("history comparison control structure", () => {
  it("keeps the baseline search, select and pagination out of one shared label", () => {
    render(<Harness runs={[makeRun("r0"), makeRun("r2")]} selected={makeRun("r1")} />);
    const search = screen.getByLabelText("搜索比较基准记录");
    const select = screen.getByLabelText("比较历史记录");
    const searchLabel = search.closest("label");
    expect(searchLabel).not.toBeNull();
    expect(searchLabel!.contains(select)).toBe(false);
    expect(select.closest("label")).toBeNull();
    expect(screen.getByRole("button", { name: "上一页" }).closest("label")).toBeNull();
    expect(screen.getByRole("button", { name: "下一页" }).closest("label")).toBeNull();
  });
});
