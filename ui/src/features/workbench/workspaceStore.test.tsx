// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import { DraftNumber, encode, newDocument, parseData } from "../../lib/data";
import { loadDrafts, saveDrafts } from "../../lib/drafts";
import {
  createWorkspaceStore,
  useWorkspaceState,
  WorkspaceProvider,
} from "./workspaceStore";

afterEach(() => {
  cleanup();
  localStorage.clear();
});
describe("scoped workspace state", () => {
  it("keeps precise tokens and unfinished numbers through functional updates and draft restoration", () => {
    const store = createWorkspaceStore();
    const document = newDocument();
    document.context = parseData('{"serial":9007199254740993}');
    store.getState().setDocuments([document]);
    store
      .getState()
      .setDocuments((docs) =>
        docs.map((doc) => ({ ...doc, title: "保留精度" })),
      );
    expect(encode(store.getState().documents[0].context)).toBe(
      '{"serial":9007199254740993}',
    );
    store.getState().documents[0].context.pending = new DraftNumber("1e-");
    expect(
      saveDrafts("isolated", store.getState().documents, document.key),
    ).toBe(true);
    const restored = loadDrafts("isolated");
    expect(encode(restored.documents[0].context.serial)).toBe(
      "9007199254740993",
    );
    expect(restored.documents[0].context.pending.raw).toBe("1e-");
    expect(() => encode(restored.documents[0].context)).toThrow();
  });
  it("isolates separately mounted workspaces and starts clean after remount", async () => {
    function Workspace({ label }: { label: string }) {
      const { documents, setDocuments } = useWorkspaceState();
      return (
        <button
          onClick={() => setDocuments((docs) => [...docs, newDocument()])}
        >
          {label}: {documents.length}
        </button>
      );
    }
    const user = userEvent.setup();
    const mounted = render(
      <>
        <WorkspaceProvider>
          <Workspace label="一" />
        </WorkspaceProvider>
        <WorkspaceProvider>
          <Workspace label="二" />
        </WorkspaceProvider>
      </>,
    );
    await user.click(screen.getByRole("button", { name: "一: 0" }));
    expect(screen.getByRole("button", { name: "一: 1" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "二: 0" })).toBeTruthy();
    mounted.unmount();
    render(
      <WorkspaceProvider>
        <Workspace label="重新打开" />
      </WorkspaceProvider>,
    );
    expect(screen.getByRole("button", { name: "重新打开: 0" })).toBeTruthy();
  });
});
