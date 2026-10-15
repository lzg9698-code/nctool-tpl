// @vitest-environment jsdom
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { newDocument } from "../../lib/data";
import { useWorkspaceSession } from "./useWorkspaceSession";
vi.mock("../../lib/api", () => ({
  api: vi.fn(async (path: string) =>
    path === "workspace"
      ? { id: "test-session" }
      : path === "runs"
        ? { runs: [] }
        : {},
  ),
}));
afterEach(() => {
  cleanup();
  localStorage.clear();
});
describe("session keyboard actions", () => {
  it("uses the newest validation callback even when document identity is unchanged", async () => {
    const generate = vi.fn(),
      invalidGenerate = vi.fn(),
      saveDocument = vi.fn();
    const doc = newDocument();
    const methods = Object.fromEntries(
      [
        "catalog",
        "fail",
        "notify",
        "setWorkspace",
        "setCapabilities",
        "setInventory",
        "setSidebarCollapsed",
        "setPaneRatio",
        "setDocuments",
        "setActive",
        "setRecovered",
        "setRuns",
        "setRunList",
        "setLoading",
      ].map((name) => [name, vi.fn()]),
    );
    const session = {
      workspace: null,
      documents: [],
      active: null,
      runs: {},
      doc,
      saving: false,
      capabilities: {},
      // An open shortcut surface: the template workbench itself.
      scopedShortcuts: true,
      saveDocument,
      generate,
      ...methods,
    } as unknown as Parameters<typeof useWorkspaceSession>[0];
    function Session({ data }: { data: typeof session }) {
      useWorkspaceSession(data);
      return null;
    }
    const mounted = render(<Session data={session} />);
    await waitFor(() => expect(methods.setLoading).toHaveBeenCalled());
    fireEvent.keyDown(window, { key: "Enter", ctrlKey: true });
    expect(generate).toHaveBeenCalledTimes(1);
    mounted.rerender(
      <Session data={{ ...session, generate: invalidGenerate }} />,
    );
    fireEvent.keyDown(window, { key: "Enter", ctrlKey: true });
    expect(invalidGenerate).toHaveBeenCalledTimes(1);
    expect(generate).toHaveBeenCalledTimes(1);
  });
});
describe("workbench shortcut scope", () => {
  const generate = vi.fn(),
    saveDocument = vi.fn();
  const doc = newDocument();
  const methods = Object.fromEntries(
    [
      "catalog",
      "fail",
      "notify",
      "setWorkspace",
      "setCapabilities",
      "setInventory",
      "setSidebarCollapsed",
      "setPaneRatio",
      "setDocuments",
      "setActive",
      "setRecovered",
      "setRuns",
      "setRunList",
      "setLoading",
    ].map((name) => [name, vi.fn()]),
  );
  function mount(scoped: boolean) {
    const session = {
      workspace: null,
      documents: [],
      active: null,
      runs: {},
      doc,
      saving: false,
      capabilities: {},
      scopedShortcuts: scoped,
      saveDocument,
      generate,
      ...methods,
    } as unknown as Parameters<typeof useWorkspaceSession>[0];
    function Session() {
      useWorkspaceSession(session);
      return (
        <div>
          <input aria-label="字段" />
          <div role="dialog" aria-label="阻断对话框">
            <button>确定</button>
          </div>
        </div>
      );
    }
    return render(<Session />);
  }
  afterEach(() => {
    vi.clearAllMocks();
  });
  it("does not fire on pages that are not the template workbench", async () => {
    const mounted = mount(false);
    await waitFor(() => expect(methods.setLoading).toHaveBeenCalled());
    fireEvent.keyDown(window, { key: "Enter", ctrlKey: true });
    fireEvent.keyDown(window, { key: "s", ctrlKey: true });
    expect(generate).not.toHaveBeenCalled();
    expect(saveDocument).not.toHaveBeenCalled();
    mounted.unmount();
  });
  it("does not hijack keys while a field is focused", async () => {
    const mounted = mount(true);
    await waitFor(() => expect(methods.setLoading).toHaveBeenCalled());
    const field = mounted.getByLabelText("字段");
    (field as HTMLElement).focus();
    fireEvent.keyDown(field, { key: "Enter", ctrlKey: true });
    fireEvent.keyDown(field, { key: "s", ctrlKey: true });
    expect(generate).not.toHaveBeenCalled();
    expect(saveDocument).not.toHaveBeenCalled();
    mounted.unmount();
  });
  it("does not hijack keys while a blocking dialog is open", async () => {
    const mounted = mount(true);
    await waitFor(() => expect(methods.setLoading).toHaveBeenCalled());
    const dialog = mounted.getByRole("dialog");
    fireEvent.keyDown(dialog, { key: "Enter", ctrlKey: true });
    fireEvent.keyDown(dialog, { key: "s", ctrlKey: true });
    expect(generate).not.toHaveBeenCalled();
    expect(saveDocument).not.toHaveBeenCalled();
    mounted.unmount();
  });
});
