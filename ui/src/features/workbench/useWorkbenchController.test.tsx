// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useWorkbenchController } from "./useWorkbenchController";
import { WorkspaceProvider } from "./workspaceStore";
import { toast } from "sonner";
import { action, api } from "../../lib/api";
import type { Asset, Schema } from "../../lib/data";

// Only the controller and its store are real; every boundary it calls is a stub
// so one test can control the ordering of inspect requests and user edits.
vi.mock("../../lib/api", () => ({
  api: vi.fn(async (path: string) => {
    if (path === "workspace") return { id: "review" };
    if (path === "capabilities") return { actions: [], panels: [] };
    if (path === "plugins") return { plugins: [] };
    return { runs: [] };
  }),
  action: vi.fn(),
  ApiError: class extends Error {},
}));
vi.mock("../../lib/templateCatalog", () => ({
  loadTemplateCatalog: vi.fn(async () => ({ items: [] })),
}));
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

afterEach(() => {
  cleanup();
  localStorage.clear();
  vi.clearAllMocks();
});

/** Text handed to the toast, so assertions do not depend on toast internals. */
function notified(): string[] {
  const texts: string[] = [];
  for (const mock of [toast.success, toast.error]) {
    for (const call of vi.mocked(mock).mock.calls) {
      const message = call[0];
      const text =
        typeof message === "string"
          ? message
          : (message as { props?: { children?: string } })?.props?.children;
      if (typeof text === "string") texts.push(text);
    }
  }
  return texts;
}
function failed(): boolean {
  return vi.mocked(toast.error).mock.calls.length > 0;
}
async function ready() {
  const hook = renderHook(() => useWorkbenchController(), {
    wrapper: WorkspaceProvider,
  });
  await waitFor(() => expect(hook.result.current.loading).toBe(false));
  act(() => hook.result.current.create());
  return hook;
}
function asset(properties: Record<string, Schema>, required?: string[]): Asset {
  return {
    source: "unused",
    schema: { type: "object", properties, required: required || [] },
  };
}

describe("discover applies inspection results only to a matching document version", () => {
  it("keeps a newer schema and source when the response arrives after the source changed", async () => {
    const hook = await ready();
    act(() =>
      hook.result.current.editDoc({
        asset: asset({}, []),
      }),
    );
    let release: (value: unknown) => void = () => {};
    vi.mocked(action).mockImplementation(
      async () =>
        new Promise((resolve) => {
          release = resolve;
        }) as never,
    );
    let pending: Promise<void>;
    act(() => {
      pending = hook.result.current.discover();
    });
    // The user rewrites the source and declares a variable while in flight.
    act(() =>
      hook.result.current.editDoc({
        asset: {
          source: "{{ newer }}",
          schema: {
            type: "object",
            properties: { newer: { type: "number" } },
            required: [],
          },
        },
      }),
    );
    await act(async () => {
      release({ data: { variables: [{ name: "old", optional: false }] } });
      await pending;
    });

    expect(hook.result.current.doc?.asset.source).toBe("{{ newer }}");
    const properties = hook.result.current.doc?.asset.schema?.properties || {};
    expect(properties).toHaveProperty("newer");
    // The stale result must not be merged in at all.
    expect(properties).not.toHaveProperty("old");
    expect(hook.result.current.doc?.asset.schema?.required).not.toContain("old");
    expect(failed()).toBe(true);
    expect(notified()).toContain("源码或规格已修改，本次识别结果已忽略，请重新识别");
  });

  it("discards a late response when only the schema version changed", async () => {
    const hook = await ready();
    act(() =>
      hook.result.current.editDoc({
        asset: {
          source: "{{ shared }}",
          schema: { type: "object", properties: {}, required: [] },
        },
      }),
    );
    let release: (value: unknown) => void = () => {};
    vi.mocked(action).mockImplementation(
      async () =>
        new Promise((resolve) => {
          release = resolve;
        }) as never,
    );
    let pending: Promise<void>;
    act(() => {
      pending = hook.result.current.discover();
    });
    act(() =>
      hook.result.current.editDoc({
        asset: {
          source: "{{ shared }}",
          schema: {
            type: "object",
            properties: { fresh: { type: "string" } },
            required: [],
          },
        },
      }),
    );
    await act(async () => {
      release({ data: { variables: [{ name: "old", optional: false }] } });
      await pending;
    });

    const properties = hook.result.current.doc?.asset.schema?.properties || {};
    expect(properties).toHaveProperty("fresh");
    expect(properties).not.toHaveProperty("old");
    expect(hook.result.current.doc?.asset.schema?.required).not.toContain("old");
  });

  it("merges additively when the source still matches, touching only new variables", async () => {
    const hook = await ready();
    act(() =>
      hook.result.current.editDoc({
        asset: {
          source: "{{ a }}{{ b }}{{ c }}",
          schema: {
            type: "object",
            properties: {
              a: { type: "number", title: "自定义" },
              b: { type: "string" },
            },
            required: ["a"],
          },
        },
      }),
    );
    await act(async () => {
      vi.mocked(action).mockResolvedValue({
        data: {
          variables: [
            { name: "b", optional: true },
            { name: "c", optional: false },
          ],
        },
      } as never);
      await hook.result.current.discover();
    });

    const current = hook.result.current.doc?.asset.schema;
    // Hand-set definitions survive untouched, including the custom title.
    expect(current?.properties?.a).toEqual({ type: "number", title: "自定义" });
    expect(current?.properties?.b).toEqual({ type: "string" });
    // Only the previously unreferenced variable is added as untyped.
    expect(current?.properties?.c).toEqual({ title: "c", "x-untyped": true });
    // required keeps a and gains non-optional c, without duplicating optional b.
    expect(current?.required).toEqual(["a", "c"]);
    expect(notified()).toContain("发现 1 个新参数，请选择类型");
    expect(hook.result.current.inspector).toBe("definition");
  });

  it("normalizes a schema that is missing containers before adding variables", async () => {
    const hook = await ready();
    act(() =>
      hook.result.current.editDoc({ asset: { source: "{{ only }}" } }),
    );
    await act(async () => {
      vi.mocked(action).mockResolvedValue({
        data: { variables: [{ name: "only", optional: false }] },
      } as never);
      await hook.result.current.discover();
    });

    const current = hook.result.current.doc?.asset.schema;
    expect(current?.properties?.only).toEqual({
      title: "only",
      "x-untyped": true,
    });
    expect(current?.required).toEqual(["only"]);
    expect(notified()).toContain("发现 1 个新参数，请选择类型");
  });

  it("does not write a new asset when nothing was discovered", async () => {
    const hook = await ready();
    const before = {
      source: "{{ known }}",
      schema: {
        type: "object",
        properties: { known: { type: "number" } },
        required: ["known"],
      },
    };
    act(() => hook.result.current.editDoc({ asset: before }));
    await act(async () => {
      vi.mocked(action).mockResolvedValue({
        data: { variables: [{ name: "known", optional: false }] },
      } as never);
      await hook.result.current.discover();
    });

    expect(hook.result.current.doc?.asset).toEqual(before);
    expect(failed()).toBe(false);
    expect(notified()).toContain("参数已与源码引用同步");
  });

  it("reports the failure without jumping the editor for a rejected inspection", async () => {
    const hook = await ready();
    act(() =>
      hook.result.current.editDoc({
        asset: {
          source: "{{ boom }}",
          schema: { type: "object", properties: {}, required: [] },
        },
      }),
    );
    await act(async () => {
      vi.mocked(action).mockRejectedValue(new Error("编译失败"));
      await hook.result.current.discover();
    });

    expect(failed()).toBe(true);
    expect(hook.result.current.doc?.asset.schema?.properties).toEqual({});
    expect(hook.result.current.jump).toBeUndefined();
  });
});

describe("generation entry gate", () => {
  // R8: the button disables while the previous task is queued or running, but
  // the shortcut slipped a second task past it — and even generated a hidden
  // document after the user navigated to the shared parameter page.
  const descriptor = {
    id: "template.render",
    ui_schema: {
      document_input: {
        default: true,
        bindings: { source: "source", context: "context", schema: "schema" },
      },
    },
  };
  it("refuses a second task while the previous one is still queued or running", async () => {
    let created = 0;
    vi.mocked(api).mockImplementation(async (path: string, body?: unknown) => {
      if (path === "capabilities")
        return { actions: [descriptor], panels: [] };
      if (path === "runs" && body) return { id: "run-" + ++created };
      return { runs: [] };
    });
    const hook = await ready();
    act(() =>
      hook.result.current.editDoc({
        asset: {
          source: "hello",
          schema: { type: "object", properties: {}, required: [] },
        },
      }),
    );
    await act(async () => hook.result.current.generate());
    expect(created).toBe(1);
    expect(hook.result.current.busy).toBe(true);
    await act(async () => hook.result.current.generate());
    await act(async () => hook.result.current.generate());
    expect(created).toBe(1);
  });

  it("reports why the entry was refused instead of failing silently", async () => {
    let created = 0;
    vi.mocked(api).mockImplementation(async (path: string, body?: unknown) => {
      if (path === "capabilities")
        return { actions: [descriptor], panels: [] };
      if (path === "runs" && body) return { id: "run-" + ++created };
      return { runs: [] };
    });
    const hook = await ready();
    act(() =>
      hook.result.current.editDoc({
        asset: {
          source: "hello",
          schema: { type: "object", properties: {}, required: [] },
        },
      }),
    );
    await act(async () => hook.result.current.generate());
    await act(async () => hook.result.current.generate());
    expect(notified()).toContain("任务正在执行，可在结果区取消");
  });

  it("keeps generating once the previous task has finished", async () => {
    let created = 0;
    vi.mocked(api).mockImplementation(async (path: string, body?: unknown) => {
      if (path === "capabilities")
        return { actions: [descriptor], panels: [] };
      if (path === "runs" && body) return { id: "run-" + ++created };
      return { runs: [] };
    });
    const hook = await ready();
    act(() =>
      hook.result.current.editDoc({
        asset: {
          source: "hello",
          schema: { type: "object", properties: {}, required: [] },
        },
      }),
    );
    await act(async () => hook.result.current.generate());
    expect(created).toBe(1);
    // The task finished outside the UI (the service advanced it); the entry must
    // open again instead of staying locked out forever.
    act(() =>
      hook.result.current.setRuns((current) => ({
        ...current,
        "run-1": { ...current["run-1"], status: "succeeded" },
      })),
    );
    await act(async () => hook.result.current.generate());
    expect(created).toBe(2);
  });
});
