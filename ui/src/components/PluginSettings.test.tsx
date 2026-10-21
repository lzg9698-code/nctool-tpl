// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { api } from "../lib/api";
import type { Data } from "../lib/data";
import { PluginSettings } from "./PluginSettings";

vi.mock("../lib/api", () => ({ api: vi.fn(), action: vi.fn() }));
vi.mock("./LazyEditor", () => ({
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

const baseConfig = {
  profile: "template",
  enabled: [] as string[],
  disabled: [] as string[],
  plugins: {},
  providers: {},
};
const plugin = (id: string, extra: Data = {}) => ({
  id,
  builtin: true,
  available: true,
  active: false,
  enabled_next_start: ["template", "parameters"].includes(id),
  ...extra,
});

beforeEach(() => {
  vi.mocked(api).mockReset();
});
afterEach(cleanup);

async function setup(
  overrides: Partial<typeof baseConfig> = {},
  extraPlugins: Data[] = [],
) {
  const config = { ...baseConfig, ...overrides };
  vi.mocked(api).mockImplementation(async (path) => {
    if (path === "config") return { config, fingerprint: "saved-fingerprint" };
    if (path === "config/validate") return { plan: { order: [] } };
    throw new Error(`Unexpected API call: ${path}`);
  });
  const reload = vi.fn();
  render(
    <PluginSettings
      inventory={{
        plugins: [
          plugin("template"),
          plugin("parameters"),
          plugin("math"),
          ...extraPlugins,
        ],
      }}
      reload={reload}
      notify={vi.fn()}
      fail={vi.fn()}
    />,
  );
  await screen.findByText("共享参数");
  return { user: userEvent.setup(), reload };
}

function card(name: string) {
  return screen
    .getByText(name, { selector: "strong" })
    .closest(".plugin-config-card") as HTMLElement;
}
function expectEnabled(name: string, enabled: boolean) {
  expect(
    within(card(name)).getByText(`· 配置${enabled ? "启用" : "停用"}`, {
      exact: false,
    }),
  ).toBeTruthy();
  expect(
    within(card(name)).getByRole("button", {
      name: enabled ? "设为停用" : "设为启用",
    }),
  ).toBeTruthy();
}
function editDraft(overrides: Partial<typeof baseConfig>) {
  fireEvent.change(screen.getByRole("textbox", { name: "启动配置 JSON" }), {
    target: { value: JSON.stringify({ ...baseConfig, ...overrides }) },
  });
}

describe("PluginSettings capability selection", () => {
  it("hides every retired inventory item while keeping unknown external plugins", async () => {
    await setup({}, [
      ...["nc", "process", "sync-turning", "undercut", "future-retired"].map(
        (id) => plugin(id, { retired: true, available: false }),
      ),
      plugin("custom-external", { builtin: false }),
    ]);
    for (const id of [
      "nc",
      "process",
      "sync-turning",
      "undercut",
      "future-retired",
    ]) {
      expect(screen.queryByText(id, { selector: "strong" })).toBeNull();
    }
    expectEnabled("custom-external", false);
    expect(
      within(card("custom-external")).getByRole("button", { name: "卸载" }),
    ).toBeTruthy();
  });

  it("shows parameters enabled by default and math disabled", async () => {
    await setup();
    expectEnabled("通用模板", true);
    expectEnabled("共享参数", true);
    expectEnabled("数学扩展", false);
  });

  it("lets parameters toggle off and back on in the unsaved draft", async () => {
    const { user, reload } = await setup();
    await user.click(
      within(card("共享参数")).getByRole("button", { name: "设为停用" }),
    );
    expectEnabled("共享参数", false);
    await waitFor(() =>
      expect(api).toHaveBeenCalledWith("config/validate", {
        config: { ...baseConfig, disabled: ["parameters"] },
      }),
    );
    await user.click(
      within(card("共享参数")).getByRole("button", { name: "设为启用" }),
    );
    expectEnabled("共享参数", true);
    await waitFor(() =>
      expect(api).toHaveBeenLastCalledWith("config/validate", {
        config: { ...baseConfig, enabled: ["parameters"] },
      }),
    );
    expect(api).not.toHaveBeenCalledWith("config", expect.anything());
    expect(reload).not.toHaveBeenCalled();
  });

  it("makes implicit parameters follow template toggle before saving", async () => {
    const { user } = await setup();
    await user.click(
      within(card("通用模板")).getByRole("button", { name: "设为停用" }),
    );
    expectEnabled("通用模板", false);
    expectEnabled("共享参数", false);
    await user.click(
      within(card("通用模板")).getByRole("button", { name: "设为启用" }),
    );
    expectEnabled("共享参数", true);
  });

  it("retains explicitly enabled parameters with template disabled, with disabled taking precedence", async () => {
    await setup({ enabled: ["parameters"], disabled: ["template"] });
    expectEnabled("通用模板", false);
    expectEnabled("共享参数", true);
    editDraft({
      enabled: ["parameters"],
      disabled: ["template", "parameters"],
    });
    expectEnabled("共享参数", false);
  });

  it("recomputes companions and external selection on JSON draft edits despite saved inventory", async () => {
    await setup({}, [
      plugin("custom-external", { builtin: false, enabled_next_start: false }),
    ]);
    editDraft({ disabled: ["template"], enabled: ["custom-external"] });
    expectEnabled("共享参数", false);
    expectEnabled("custom-external", true);
    editDraft({
      disabled: ["template", "custom-external"],
      enabled: ["parameters", "custom-external"],
    });
    expectEnabled("共享参数", true);
    expectEnabled("custom-external", false);
    editDraft({ disabled: ["parameters"] });
    expectEnabled("通用模板", true);
    expectEnabled("共享参数", false);
  });
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("PluginSettings async request gating", () => {
  it("drops an inspect reply whose directory changed, keeps the latest, and installs the inspected directory", async () => {
    const { user, reload } = await setup();
    const first = deferred<Data>();
    const second = deferred<Data>();
    let inspectCalls = 0;
    vi.mocked(api).mockImplementation(async (path) => {
      if (path === "plugins/inspect")
        return inspectCalls++ === 0 ? first.promise : second.promise;
      if (path === "plugins/install") return {};
      throw new Error(`Unexpected API call: ${path}`);
    });
    const directory = screen.getByRole("textbox", { name: "插件目录" });
    fireEvent.change(directory, { target: { value: "/plugins/A" } });
    await user.click(screen.getByRole("button", { name: "读取插件信息" }));
    // Edit the directory before the first reply arrives.
    fireEvent.change(directory, { target: { value: "/plugins/B" } });
    await user.click(screen.getByRole("button", { name: "读取插件信息" }));

    await act(async () => {
      first.resolve({
        descriptor: { id: "plugin-A", version: "1.0.0", requires: [] },
        actions: [],
      });
    });
    expect(api).toHaveBeenCalledWith("plugins/inspect", {
      source: "/plugins/A",
    });
    // The late reply for the abandoned directory must not be shown.
    expect(screen.queryByText(/plugin-A/)).toBeNull();

    await act(async () => {
      second.resolve({
        descriptor: { id: "plugin-B", version: "2.0.0", requires: [] },
        actions: [],
      });
    });
    expect(await screen.findByText("plugin-B · 2.0.0")).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "安装插件" }));
    await waitFor(() =>
      expect(api).toHaveBeenCalledWith("plugins/install", {
        source: "/plugins/B",
      }),
    );
    expect(reload).toHaveBeenCalled();
  });

  it("keeps the save button disabled while the config POST is pending, then re-enables it", async () => {
    const { user, reload } = await setup();
    const post = deferred<Data>();
    vi.mocked(api).mockImplementation(async (path, body) => {
      if (path === "config/validate") return { plan: { order: [] } };
      if (path === "config" && body === undefined)
        return { config: baseConfig, fingerprint: "fingerprint-2" };
      if (path === "config") return post.promise;
      throw new Error(`Unexpected API call: ${path}`);
    });
    const saveButton = screen.getByRole("button", {
      name: "验证并保存配置",
    });
    await user.click(saveButton);
    await waitFor(() =>
      expect(api).toHaveBeenCalledWith("config", {
        config: baseConfig,
        expected: "saved-fingerprint",
      }),
    );
    // Validation already finished (checking=false) but the save is still in
    // flight: the button must stay disabled so it cannot be clicked twice.
    expect((saveButton as HTMLButtonElement).disabled).toBe(true);
    await act(async () => {
      post.resolve({});
    });
    await waitFor(() => expect(reload).toHaveBeenCalled());
    await waitFor(() =>
      expect((saveButton as HTMLButtonElement).disabled).toBe(false),
    );
  });

  it("preserves draft edits made during save and keeps the stored fingerprint for the next save", async () => {
    const { user } = await setup();
    const post = deferred<Data>();
    let configLoads = 0;
    vi.mocked(api).mockImplementation(async (path, body) => {
      if (path === "config/validate") return { plan: { order: [] } };
      if (path === "config" && body === undefined) {
        configLoads += 1;
        return { config: baseConfig, fingerprint: `fingerprint-${configLoads}` };
      }
      if (path === "config") return post.promise;
      throw new Error(`Unexpected API call: ${path}`);
    });
    const draft = { ...baseConfig, enabled: ["math"] };
    const saveButton = screen.getByRole("button", {
      name: "验证并保存配置",
    });
    await user.click(saveButton);
    await waitFor(() =>
      expect(api).toHaveBeenCalledWith("config", {
        config: baseConfig,
        expected: "saved-fingerprint",
      }),
    );
    // Edit the draft while the POST is still in flight.
    const editor = screen.getByRole("textbox", { name: "启动配置 JSON" });
    fireEvent.change(editor, { target: { value: JSON.stringify(draft) } });
    await act(async () => {
      post.resolve({});
    });
    await waitFor(() => expect(configLoads).toBe(1));
    // The reloaded (pre-edit) config must not overwrite the newer draft.
    expect((editor as HTMLTextAreaElement).value).toBe(JSON.stringify(draft));

    // The fingerprint still advances to the value the server just stored.
    const secondPost = deferred<Data>();
    vi.mocked(api).mockImplementation(async (path, body) => {
      if (path === "config/validate") return { plan: { order: [] } };
      if (path === "config" && body === undefined)
        return { config: baseConfig, fingerprint: "fingerprint-2" };
      if (path === "config") return secondPost.promise;
      throw new Error(`Unexpected API call: ${path}`);
    });
    await user.click(saveButton);
    await waitFor(() =>
      expect(api).toHaveBeenCalledWith("config", {
        config: draft,
        expected: "fingerprint-1",
      }),
    );
    await act(async () => {
      secondPost.resolve({});
    });
  });
});
