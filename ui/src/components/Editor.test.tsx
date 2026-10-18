// @vitest-environment jsdom

import { createRef, StrictMode, useState } from "react";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { EditorView } from "@codemirror/view";
import { undo, undoDepth } from "@codemirror/commands";
import { Editor, type EditorHandle } from "./Editor";

afterEach(() => {
  cleanup();
  document.documentElement.removeAttribute("data-theme");
});

function viewIn(container: HTMLElement) {
  return EditorView.findFromDOM(container.querySelector(".cm-editor")!)!;
}

describe("Editor document history", () => {
  it("can insert on readiness without losing the insertion to controlled synchronization", () => {
    const ref = createRef<EditorHandle>();
    const change = vi.fn();
    function ReadyEditor() {
      const [value, setValue] = useState("base");
      return <Editor ref={ref} value={value} onChange={(text) => { change(text); setValue(text); }} onReady={() => ref.current?.insert("include")} />;
    }
    const { container } = render(<ReadyEditor />);
    expect(viewIn(container).state.doc.toString()).toBe("includebase");
    expect(change).toHaveBeenCalledExactlyOnceWith("includebase");
    act(() => { expect(undo(viewIn(container))).toBe(true); });
    expect(viewIn(container).state.doc.toString()).toBe("base");
  });

  it("does not undo an external value replacement or emit it as a user change", () => {
    const onChange = vi.fn();
    const { container, rerender } = render(
      <Editor value="document A" onChange={onChange} />,
    );
    const view = viewIn(container);
    rerender(<Editor value="document B" onChange={onChange} />);
    expect(viewIn(container)).toBe(view);
    expect(view.state.doc.toString()).toBe("document B");
    expect(undoDepth(view.state)).toBe(0);
    act(() => {
      expect(undo(view)).toBe(false);
    });
    expect(onChange).not.toHaveBeenCalled();
  });

  it("resets history on identity changes even when the next document has identical text", () => {
    const changeA = vi.fn(),
      changeB = vi.fn();
    const { container, rerender } = render(
      <Editor editorKey="a" value="same" onChange={changeA} />,
    );
    act(() => {
      viewIn(container).dispatch({ changes: { from: 4, insert: " edited" } });
    });
    rerender(<Editor editorKey="b" value="same edited" onChange={changeB} />);
    const view = viewIn(container);
    act(() => {
      expect(undo(view)).toBe(false);
    });
    expect(view.state.doc.toString()).toBe("same edited");
    expect(changeB).not.toHaveBeenCalled();
  });

  it("preserves DOM, selection and user undo across theme changes and controlled echoes", () => {
    const onChange = vi.fn();
    function ControlledEditor() {
      const [value, setValue] = useState("abc");
      const [theme, setTheme] = useState("light");
      return (
        <div data-theme={theme}>
          <button
            onClick={() => {
              document.documentElement.dataset.theme = "dark";
              setTheme("dark");
            }}
          >
            theme
          </button>
          <Editor
            editorKey="a"
            value={value}
            onChange={(next) => {
              onChange(next);
              setValue(next);
            }}
          />
        </div>
      );
    }
    const { container, getByText } = render(
      <StrictMode>
        <ControlledEditor />
      </StrictMode>,
    );
    const view = viewIn(container);
    const dom = view.dom,
      content = view.contentDOM;
    act(() => {
      view.dispatch({
        changes: { from: 3, insert: "d" },
        selection: { anchor: 1, head: 3 },
      });
    });
    expect(onChange).toHaveBeenLastCalledWith("abcd");
    const state = view.state;
    fireEvent.click(getByText("theme"));
    expect(viewIn(container)).toBe(view);
    expect(view.dom).toBe(dom);
    expect(view.contentDOM).toBe(content);
    expect(view.state).toBe(state);
    expect(view.state.selection.main.anchor).toBe(1);
    expect(view.state.selection.main.head).toBe(3);
    act(() => {
      expect(undo(view)).toBe(true);
    });
    expect(view.state.doc.toString()).toBe("abc");
    expect(onChange).toHaveBeenLastCalledWith("abc");
  });
});
