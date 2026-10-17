// @vitest-environment jsdom

import { StrictMode, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Menu, Modal, PageTitle, JsonEditor } from "./WorkbenchUi";
import { EditorView } from "@codemirror/view";
import { undo } from "@codemirror/commands";

afterEach(cleanup);

// The production entry loads the editor chunk on demand; these tests exercise the
// real CodeMirror synchronously, so they bind the wrapper straight to it.
vi.mock("./LazyEditor", async () => {
  const actual = await vi.importActual<typeof import("./Editor")>("./Editor");
  return { Editor: actual.Editor };
});

function jsonView(container: HTMLElement) {
  return EditorView.findFromDOM(container.querySelector(".cm-editor")!)!;
}

describe("JsonEditor history isolation with real CodeMirror", () => {
  it("never sends A's schema to B when undo follows a document switch", () => {
    const changeA = vi.fn(),
      changeB = vi.fn();
    const a = { properties: { alpha: { type: "number" } } };
    const b = { properties: { beta: { type: "string" } } };
    const { container, rerender } = render(
      <JsonEditor
        editorKey="a"
        value={a}
        onChange={changeA}
        label="高级参数规格"
      />,
    );
    const viewA = jsonView(container);
    act(() => {
      viewA.dispatch({
        changes: {
          from: 0,
          to: viewA.state.doc.length,
          insert: '{"editedA":true}',
        },
      });
    });
    expect(changeA).toHaveBeenCalledWith({ editedA: true });
    rerender(
      <JsonEditor
        editorKey="b"
        value={b}
        onChange={changeB}
        label="高级参数规格"
      />,
    );
    const viewB = jsonView(container);
    expect(viewB.state.doc.toString()).toContain("beta");
    act(() => {
      expect(undo(viewB)).toBe(false);
    });
    expect(viewB.state.doc.toString()).toContain("beta");
    expect(changeB).not.toHaveBeenCalled();
    rerender(
      <JsonEditor
        editorKey="a"
        value={a}
        onChange={changeA}
        label="高级参数规格"
      />,
    );
    act(() => {
      expect(undo(jsonView(container))).toBe(false);
    });
    expect(jsonView(container).state.doc.toString()).toContain("alpha");
    expect(changeA).toHaveBeenCalledTimes(1);
  });

  it("does not record external replacements even for callers without an identity", () => {
    const onChange = vi.fn();
    const { container, rerender } = render(
      <JsonEditor value={{ alpha: true }} onChange={onChange} label="JSON" />,
    );
    rerender(
      <JsonEditor value={{ beta: true }} onChange={onChange} label="JSON" />,
    );
    act(() => {
      expect(undo(jsonView(container))).toBe(false);
    });
    expect(jsonView(container).state.doc.toString()).toContain("beta");
    expect(onChange).not.toHaveBeenCalled();
  });

  it("clears invalid local drafts and errors on identity changes with identical values", () => {
    const onChange = vi.fn();
    const { container, rerender } = render(
      <JsonEditor editorKey="a" value={{}} onChange={onChange} label="JSON" />,
    );
    const view = jsonView(container);
    act(() => {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: "{" },
      });
    });
    expect(screen.getByRole("alert")).toBeTruthy();
    rerender(
      <JsonEditor editorKey="b" value={{}} onChange={onChange} label="JSON" />,
    );
    expect(jsonView(container).state.doc.toString()).toBe("{}");
    expect(screen.queryByRole("alert")).toBeNull();
    act(() => {
      expect(undo(jsonView(container))).toBe(false);
    });
    expect(onChange).not.toHaveBeenCalled();
  });

  it("preserves raw formatting and undo for same-document user edits and parent echoes", () => {
    const onChange = vi.fn();
    function ControlledJson() {
      const [value, setValue] = useState({ enabled: true });
      return (
        <JsonEditor
          editorKey="a"
          value={value}
          label="JSON"
          onChange={(next) => {
            onChange(next);
            setValue(next);
          }}
        />
      );
    }
    const { container } = render(
      <StrictMode>
        <ControlledJson />
      </StrictMode>,
    );
    const view = jsonView(container),
      dom = view.dom;
    const original = view.state.doc.toString();
    act(() => {
      view.dispatch({
        changes: {
          from: 0,
          to: view.state.doc.length,
          insert: '{ "enabled": false }',
        },
      });
    });
    expect(jsonView(container).dom).toBe(dom);
    expect(view.state.doc.toString()).toBe('{ "enabled": false }');
    expect(onChange).toHaveBeenLastCalledWith({ enabled: false });
    act(() => {
      expect(undo(view)).toBe(true);
    });
    expect(view.state.doc.toString()).toBe(original);
    expect(onChange).toHaveBeenLastCalledWith({ enabled: true });
  });
});

function DialogScenario({
  onClose = () => {},
  allowClose = true,
}: {
  onClose?: () => void;
  allowClose?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState("原草稿");
  return (
    <>
      <button onClick={() => setOpen(true)}>编辑模板</button>
      <button>外部操作</button>
      {open && (
        <Modal
          title="编辑模板草稿"
          onClose={() => {
            onClose();
            if (allowClose) setOpen(false);
          }}
          buttons={<button>保存草稿</button>}
        >
          <label>
            模板名称
            <input
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
            />
          </label>
        </Modal>
      )}
    </>
  );
}

function renderDialog(props: Parameters<typeof DialogScenario>[0] = {}) {
  return render(
    <StrictMode>
      <DialogScenario {...props} />
    </StrictMode>,
  );
}

function renderMenu(
  copy: () => void,
  remove: () => void,
  disabled: () => void,
) {
  return render(
    <StrictMode>
      <Menu text="操作">
        <button onClick={copy}>
          <span>复制模板</span>
        </button>
        <button disabled onClick={disabled}>
          禁用操作
        </button>
        <button onClick={remove}>删除模板</button>
      </Menu>
    </StrictMode>,
  );
}

describe("Modal interactions", () => {
  it("has a visible accessible title and keeps Tab/Shift+Tab inside the dialog", async () => {
    const user = userEvent.setup();
    const { container } = renderDialog();
    await user.click(screen.getByRole("button", { name: "编辑模板" }));

    const dialog = screen.getByRole("dialog", { name: "编辑模板草稿" });
    expect(
      screen
        .getByRole("heading", { name: "编辑模板草稿" })
        .closest('[role="dialog"]'),
    ).toBe(dialog);
    expect(container.contains(dialog)).toBe(false);
    const field = screen.getByRole("textbox", { name: "模板名称" });
    const save = screen.getByRole("button", { name: "保存草稿" });
    const close = screen.getByRole("button", { name: "关闭对话框" });
    expect(document.activeElement).toBe(field);

    await user.tab();
    expect(document.activeElement).toBe(save);
    await user.tab();
    expect(document.activeElement).toBe(close);
    await user.tab({ shift: true });
    expect(document.activeElement).toBe(save);
    await user.tab();
    await user.tab();
    expect(document.activeElement).toBe(field);
  });

  it("closes on Escape and restores focus to the external opener on every open", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    renderDialog({ onClose });
    const opener = screen.getByRole("button", { name: "编辑模板" });
    for (let i = 0; i < 2; i++) {
      await user.click(opener);
      await user.keyboard("{Escape}");
      expect(screen.queryByRole("dialog")).toBeNull();
      await waitFor(() => expect(document.activeElement).toBe(opener));
    }
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("closes through the labeled close button and restores focus", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    renderDialog({ onClose });
    const opener = screen.getByRole("button", { name: "编辑模板" });
    await user.click(opener);
    await user.click(screen.getByRole("button", { name: "关闭对话框" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(onClose).toHaveBeenCalledOnce();
    await waitFor(() => expect(document.activeElement).toBe(opener));
  });

  it("requests close on an overlay click without closing when the caller retains a draft", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    renderDialog({ onClose, allowClose: false });
    await user.click(screen.getByRole("button", { name: "编辑模板" }));
    const field = screen.getByRole("textbox", { name: "模板名称" });
    await user.clear(field);
    await user.type(field, "未保存草稿");
    const overlay = document.querySelector<HTMLElement>(".modal-overlay")!;
    await user.click(overlay);
    expect(onClose).toHaveBeenCalledOnce();
    expect(screen.getByRole("dialog", { name: "编辑模板草稿" })).toBeTruthy();
    expect((field as HTMLInputElement).value).toBe("未保存草稿");
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("dialog", { name: "编辑模板草稿" })).toBeTruthy();
    expect((field as HTMLInputElement).value).toBe("未保存草稿");
  });

  it("closes after an overlay click when the caller accepts the request", async () => {
    const user = userEvent.setup();
    renderDialog();
    const opener = screen.getByRole("button", { name: "编辑模板" });
    await user.click(opener);
    await user.click(document.querySelector<HTMLElement>(".modal-overlay")!);
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(opener));
  });
});

function MenuDialogScenario() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Menu>
        <button onClick={() => setOpen(true)}>删除模板</button>
      </Menu>
      {open && (
        <Modal title="确认删除模板" onClose={() => setOpen(false)}>
          <p>确认后将删除当前模板。</p>
        </Modal>
      )}
    </>
  );
}

it("returns to the menu trigger after closing a dialog opened by a menu action", async () => {
  const user = userEvent.setup();
  render(
    <StrictMode>
      <MenuDialogScenario />
    </StrictMode>,
  );
  const trigger = screen.getByRole("button", { name: "更多模板操作" });
  await user.click(trigger);
  await user.click(screen.getByRole("menuitem", { name: "删除模板" }));
  expect(screen.getByRole("dialog", { name: "确认删除模板" })).toBeTruthy();
  expect(screen.queryByRole("menu")).toBeNull();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).toBeNull();
  await waitFor(() => expect(document.activeElement).toBe(trigger));
});

describe("Menu interactions", () => {
  it("selects with arrow keys and Enter, skips disabled items, and restores trigger focus", async () => {
    const user = userEvent.setup();
    const copy = vi.fn(),
      remove = vi.fn(),
      disabled = vi.fn();
    const { container } = renderMenu(copy, remove, disabled);
    const trigger = screen.getByRole("button", { name: "更多模板操作" });
    expect(trigger.classList.contains("menu-trigger")).toBe(true);
    expect(trigger.closest(".menu-labeled")).toBeTruthy();
    await user.tab();
    await user.keyboard("{ArrowDown}");
    const menu = screen.getByRole("menu");
    expect(container.contains(menu)).toBe(false);
    const first = screen.getByRole("menuitem", { name: "复制模板" });
    await waitFor(() => expect(document.activeElement).toBe(first));
    expect(first.textContent).toBe("复制模板");
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(
      screen.getByRole("menuitem", { name: "删除模板" }),
    );
    await user.keyboard("{Enter}");
    expect(remove).toHaveBeenCalledOnce();
    expect(copy).not.toHaveBeenCalled();
    expect(disabled).not.toHaveBeenCalled();
    expect(screen.queryByRole("menu")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it("dismisses with Escape and keeps the original click handler for an enabled item", async () => {
    const user = userEvent.setup();
    const copy = vi.fn(),
      remove = vi.fn(),
      disabled = vi.fn();
    renderMenu(copy, remove, disabled);
    const trigger = screen.getByRole("button", { name: "更多模板操作" });
    await user.click(trigger);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(trigger));
    expect(copy).not.toHaveBeenCalled();
    expect(remove).not.toHaveBeenCalled();

    await user.click(trigger);
    await user.click(screen.getByRole("menuitem", { name: "复制模板" }));
    expect(copy).toHaveBeenCalledOnce();
    expect(screen.queryByRole("menu")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it("never executes a disabled item when clicked and leaves the menu open", async () => {
    const user = userEvent.setup();
    const disabled = vi.fn();
    renderMenu(vi.fn(), vi.fn(), disabled);
    await user.click(screen.getByRole("button", { name: "更多模板操作" }));
    const item = screen.getByRole("menuitem", { name: "禁用操作" });
    expect(item.getAttribute("aria-disabled")).toBe("true");
    expect((item as HTMLButtonElement).disabled).toBe(true);
    await user.click(item);
    expect(disabled).not.toHaveBeenCalled();
    expect(screen.getByRole("menu")).toBeTruthy();
  });
});

it("PageTitle displays its title and description without the WORKSPACE decoration", () => {
  render(<PageTitle title="模板" description="管理模板资产" />);
  expect(screen.getByRole("heading", { level: 1, name: "模板" })).toBeTruthy();
  expect(screen.getByText("管理模板资产")).toBeTruthy();
  expect(screen.queryByText("WORKSPACE")).toBeNull();
});
