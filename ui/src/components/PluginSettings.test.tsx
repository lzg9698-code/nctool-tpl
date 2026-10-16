// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
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
vi.mock("./Editor", () => ({
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
