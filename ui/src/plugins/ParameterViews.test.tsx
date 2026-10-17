// @vitest-environment jsdom

import { StrictMode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { action } from "../lib/api";
import { decodeDraft, encodeDraft } from "../lib/drafts";
import type { Data } from "../lib/data";
import { ParameterLibrary } from "./ParameterViews";
import type { ViewProps } from "./ViewProps";

vi.mock("../lib/api", () => ({ action: vi.fn() }));
// Exercise the real JsonEditor parsing and ParameterLibrary persistence; only replace
// CodeMirror's DOM surface so the request ordering tests do not depend on geometry.
vi.mock("../components/LazyEditor", () => ({
  Editor: ({
    value,
    onChange,
    label,
  }: {
    value: string;
    onChange: (value: string) => void;
    label: string;
  }) => (
    <textarea
      aria-label={label}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const storageKey = "nctool:parameter-draft:parameter-test:parameter";
const asset = (id = "a", title = "submitted") => ({
  format_version: 1,
  id,
  title,
  schema: { type: "number" },
  unit: "",
  group: "",
  description: "",
});
const preset = (id = "p", title = "Preset") => ({
  format_version: 1,
  id,
  title,
  values: {},
});
const fail = vi.fn();
const props: ViewProps = {
  workspaceId: "parameter-test",
  panel: {},
  capabilities: {},
  runs: {},
  start: vi.fn(),
  fail,
};
function deferred() {
  let resolve!: (result: Data) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<Data>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
let pending: ReturnType<typeof deferred>;
let catalog: Data[];
function stored(): Data {
  return decodeDraft(localStorage.getItem(storageKey)!);
}
function editor() {
  return screen.getByRole("textbox", {
    name: "资产内容",
  }) as HTMLTextAreaElement;
}
function edit(value: Data) {
  fireEvent.change(editor(), { target: { value: JSON.stringify(value) } });
}
function save() {
  fireEvent.click(screen.getByRole("button", { name: "保存资产" }));
}
function saveCalls() {
  return vi.mocked(action).mock.calls.filter(([id]) => id === "parameter.save");
}
function openCatalog(option: string) {
  // The catalog is now a list of buttons; an empty id means "no selection".
  if (!option) return;
  fireEvent.click(screen.getByLabelText(`读取资产 ${option}`));
}
function listedAssets() {
  return screen
    .queryAllByRole("listitem")
    .map((item) => item.textContent || "")
    .join("\n");
}
async function mount() {
  const view = render(
    <StrictMode>
      <ParameterLibrary {...props} />
    </StrictMode>,
  );
  await act(async () => {});
  return view;
}
async function succeed() {
  await act(async () => pending.resolve({ data: { fingerprint: "saved-v1" } }));
}

beforeEach(() => {
  pending = deferred();
  catalog = [
    { id: "a", title: "A", asset: asset(), fingerprint: "a0" },
    {
      id: "b",
      title: "B",
      asset: asset("b", "other asset"),
      fingerprint: "b0",
    },
  ];
  localStorage.setItem(
    storageKey,
    encodeDraft({
      group: "definitions",
      asset: asset(),
      expected: "a0",
    }),
  );
  vi.mocked(action).mockImplementation(async (id) => {
    if (id === "parameter.save") return pending.promise;
    return { data: { items: catalog } } as Awaited<ReturnType<typeof action>>;
  });
});
afterEach(() => {
  cleanup();
  localStorage.clear();
  vi.resetAllMocks();
});

describe("ParameterLibrary structured form", () => {
  // The structured editor writes through the same asset as the advanced JSON
  // editor. A regression here silently reverts every keystroke, because React
  // re-renders the input from an unchanged draft.
  it("keeps a structured field edit in the asset and the stored draft", async () => {
    const view = await mount();
    fireEvent.change(screen.getByLabelText("显示名称"), { target: { value: "加工深度" } });
    fireEvent.change(screen.getByLabelText("单位"), { target: { value: "mm" } });
    expect((screen.getByLabelText("显示名称") as HTMLInputElement).value).toBe("加工深度");
    expect(stored().asset).toMatchObject({ title: "加工深度", unit: "mm" });
    view.unmount();
  });

  it("switches a definition type and drops containers that no longer apply", async () => {
    const view = render(
      <StrictMode>
        <ParameterLibrary {...props} />
      </StrictMode>,
    );
    await act(async () => {});
    fireEvent.change(screen.getByLabelText("参数类型"), { target: { value: "number" } });
    await act(async () => {});
    expect(stored().asset.schema).toEqual({ type: "number" });
    view.unmount();
  });

  it("clears the advanced JSON draft when a structured field is edited", async () => {
    const view = await mount();
    // An unresolved raw draft would otherwise win over the structured edit.
    fireEvent.change(editor(), { target: { value: '{"id":"a", broken' } });
    expect(stored()).toHaveProperty("raw");
    fireEvent.change(screen.getByLabelText("显示名称"), { target: { value: "重新编辑" } });
    expect(stored()).not.toHaveProperty("raw");
    expect(stored().asset.title).toBe("重新编辑");
    view.unmount();
  });
});

describe("ParameterLibrary save responses", () => {
  it("saves the submitted asset normally and uses its fingerprint for the next save", async () => {
    await mount();
    save();
    expect(saveCalls()[0][1]).toEqual({
      group: "definitions",
      name: "a",
      asset: asset(),
      expected: "a0",
    });
    await succeed();
    expect(screen.getByRole("status").textContent).toBe("已保存");
    expect(stored()).toEqual({
      group: "definitions",
      asset: asset(),
      expected: "saved-v1",
    });
    pending = deferred();
    edit(asset("a", "next revision"));
    save();
    expect(saveCalls()[1][1]).toMatchObject({
      expected: "saved-v1",
      asset: { title: "next revision" },
    });
    await succeed();
  });

  it("keeps later edits in the editor and storage while advancing the same asset's expected fingerprint", async () => {
    await mount();
    save();
    edit(asset("a", "new unsaved edit"));
    await succeed();
    expect(editor().value).toContain("new unsaved edit");
    expect(stored()).toMatchObject({
      asset: { title: "new unsaved edit" },
      expected: "saved-v1",
    });
    expect(screen.queryByText("已保存")).toBeNull();
    pending = deferred();
    save();
    expect(saveCalls()[1][1]).toMatchObject({
      asset: { title: "new unsaved edit" },
      expected: "saved-v1",
    });
    await succeed();
  });

  it("preserves invalid JSON and its error when the earlier save succeeds, then saves repaired input against the new fingerprint", async () => {
    await mount();
    save();
    fireEvent.change(editor(), { target: { value: '{"id":"a", invalid' } });
    const message = screen.getByRole("status").textContent;
    await succeed();
    expect(editor().value).toBe('{"id":"a", invalid');
    expect(stored()).toMatchObject({
      raw: '{"id":"a", invalid',
      expected: "saved-v1",
    });
    expect(screen.getByRole("status").textContent).toBe(message);
    expect(
      (screen.getByRole("button", { name: "保存资产" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    edit(asset("a", "repaired"));
    pending = deferred();
    save();
    expect(saveCalls()[1][1]).toMatchObject({
      expected: "saved-v1",
      asset: { title: "repaired" },
    });
    await succeed();
  });

  it.each([
    "read",
    "new",
    "group",
    "reload same asset",
    "rename away and back",
  ])("does not apply an old response after %s", async (change) => {
    await mount();
    save();
    if (change === "read" || change === "reload same asset") {
      openCatalog(change === "read" ? "b" : "a");
    } else if (change === "new") {
      fireEvent.click(screen.getByRole("button", { name: "新建资产" }));
      edit(asset("a", "new draft with same ID"));
    } else if (change === "group") {
      fireEvent.change(screen.getByLabelText("资产类型"), {
        target: { value: "presets" },
      });
    } else {
      edit(asset("different"));
      edit(asset("a", "renamed back"));
    }
    const before = localStorage.getItem(storageKey);
    const text = editor().value;
    await succeed();
    expect(localStorage.getItem(storageKey)).toBe(before);
    expect(editor().value).toBe(text);
    expect(screen.queryByText("已保存")).toBeNull();
    expect(fail).not.toHaveBeenCalled();
  });

  it("ignores duplicate saves while the first request is pending", async () => {
    await mount();
    save();
    save();
    expect(saveCalls()).toHaveLength(1);
    expect(
      (screen.getByRole("button", { name: "保存资产" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    edit(asset("a", "still editable"));
    await succeed();
    expect(
      (screen.getByRole("button", { name: "保存资产" }) as HTMLButtonElement)
        .disabled,
    ).toBe(false);
    expect(stored().asset.title).toBe("still editable");
  });

  it("keeps a failed submission and expected fingerprint intact and permits a retry", async () => {
    await mount();
    save();
    const error = new Error("fingerprint conflict");
    await act(async () => pending.reject(error));
    expect(editor().value).toContain("submitted");
    expect(stored().expected).toBe("a0");
    expect(screen.getByRole("status").textContent).toBe("fingerprint conflict");
    expect(fail).toHaveBeenCalledWith(error);
    pending = deferred();
    save();
    expect(saveCalls()[1][1]).toMatchObject({ expected: "a0" });
    await succeed();
  });

  it("keeps new invalid input and its validation error when a save fails", async () => {
    await mount();
    save();
    fireEvent.change(editor(), { target: { value: "{ broken" } });
    const before = localStorage.getItem(storageKey);
    const message = screen.getByRole("status").textContent;
    await act(async () => pending.reject(new Error("save failed")));
    expect(localStorage.getItem(storageKey)).toBe(before);
    expect(editor().value).toBe("{ broken");
    expect(screen.getByRole("status").textContent).toBe(message);
    expect(fail).toHaveBeenCalledOnce();
  });

  it("does not report a late failure against another asset", async () => {
    await mount();
    save();
    openCatalog("b");
    const before = localStorage.getItem(storageKey);
    await act(async () => pending.reject(new Error("old asset failure")));
    expect(localStorage.getItem(storageKey)).toBe(before);
    expect(screen.queryByText("old asset failure")).toBeNull();
    expect(fail).not.toHaveBeenCalled();
  });

  it.each([true, false])(
    "does not mutate storage or notify after unmount (success=%s)",
    async (success) => {
      const view = await mount();
      save();
      view.unmount();
      const catalogs = () =>
        vi.mocked(action).mock.calls.filter(([id]) => id === "parameter.catalog").length;
      const before = catalogs();
      const replacement = encodeDraft({
        group: "definitions",
        asset: asset("b"),
        expected: "b1",
      });
      localStorage.setItem(storageKey, replacement);
      await act(async () => {
        if (success) pending.resolve({ data: { fingerprint: "saved-v1" } });
        else pending.reject(new Error("late failure"));
      });
      expect(localStorage.getItem(storageKey)).toBe(replacement);
      expect(fail).not.toHaveBeenCalled();
      // Settling a save after unmount must not trigger further catalog work.
      expect(catalogs()).toBe(before);
    },
  );
});

describe("ParameterLibrary catalog request ordering", () => {
  // Each group gets its own controllable response, so the definitions request can
  // stay in flight while the presets one already completed.
  let responses: Record<string, ReturnType<typeof deferred>>;
  function respond(group: string, items: Data[]) {
    act(() => responses[group].resolve({ data: { items } } as Data));
  }
  beforeEach(() => {
    responses = { definitions: deferred(), presets: deferred() };
    vi.mocked(action).mockImplementation(async (id, input) => {
      if (id === "parameter.catalog")
        return responses[input.group]
          .promise as Awaited<ReturnType<typeof action>>;
      return { data: { items: [] } } as Awaited<ReturnType<typeof action>>;
    });
  });
  afterEach(() => cleanup());

  it("drops a late catalog response from the group the user switched away from", async () => {
    localStorage.setItem(
      storageKey,
      encodeDraft({
        group: "definitions",
        asset: asset(),
        expected: "a0",
      }),
    );
    render(<ParameterLibrary {...props} />);
    await act(async () => {});
    fireEvent.change(screen.getByLabelText("资产类型"), {
      target: { value: "presets" },
    });
    // The presets request completes first, the earlier definitions one later.
    respond("presets", [
      { id: "p", title: "Preset", asset: preset(), fingerprint: "p0" },
    ]);
    await act(async () => {});
    expect(listedAssets()).toContain("p");
    expect(listedAssets()).not.toContain("Definition");
    respond("definitions", [
      { id: "d", title: "Definition", asset: asset("d"), fingerprint: "d0" },
    ]);
    await act(async () => {});
    // The stale response neither renders under presets nor becomes readable.
    expect(screen.getByLabelText<HTMLSelectElement>("资产类型").value).toBe(
      "presets",
    );
    expect(listedAssets()).not.toContain("Definition");
    // A stale definitions entry must not be listed, so it can never be read back.
    expect(screen.queryByLabelText("读取资产 d")).toBeNull();
    expect(stored().group).toBe("presets");
    // The draft stayed the preset the user was editing; no definition replaced it.
    expect(stored().asset.id).toBe("");
    expect(stored().asset).toHaveProperty("values");
    expect(stored().asset).not.toHaveProperty("schema");
  });

  it("clears the catalog of the previous group when switching types", async () => {
    localStorage.setItem(
      storageKey,
      encodeDraft({
        group: "definitions",
        asset: asset(),
        expected: "a0",
      }),
    );
    render(<ParameterLibrary {...props} />);
    respond("definitions", [
      { id: "a", title: "A", asset: asset(), fingerprint: "a0" },
    ]);
    await act(async () => {});
    expect(listedAssets()).toContain("a");
    fireEvent.change(screen.getByLabelText("资产类型"), {
      target: { value: "presets" },
    });
    // The presets catalog is still in flight, so nothing from definitions remains.
    expect(listedAssets()).not.toContain("A");
    expect(stored().group).toBe("presets");
  });

  it("shows only the catalog of the group currently being edited", async () => {
    localStorage.setItem(
      storageKey,
      encodeDraft({
        group: "presets",
        asset: preset(),
        expected: null,
      }),
    );
    render(<ParameterLibrary {...props} />);
    // Both responses arrive after the mount, presets last.
    respond("definitions", [
      { id: "d", title: "Definition", asset: asset("d"), fingerprint: "d0" },
    ]);
    respond("presets", [
      { id: "p", title: "Preset", asset: preset(), fingerprint: "p0" },
    ]);
    await act(async () => {});
    expect(screen.getByLabelText<HTMLSelectElement>("资产类型").value).toBe(
      "presets",
    );
    expect(listedAssets()).not.toContain("Definition");
    openCatalog("p");
    // Reading keeps the preset the user asked for, not a definition of another group.
    expect(stored().asset.id).toBe("p");
    expect(stored().asset).toHaveProperty("values");
    expect(stored().asset).not.toHaveProperty("schema");
    expect(stored().expected).toBe("p0");
  });
});
