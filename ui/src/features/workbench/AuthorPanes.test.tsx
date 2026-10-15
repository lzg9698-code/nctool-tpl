// @vitest-environment jsdom

import { createRef } from "react";
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { EditorView } from "@codemirror/view";
import { undo } from "@codemirror/commands";
import { AuthorPanes } from "./AuthorPanes";
import type { WorkbenchController } from "./useWorkbenchController";
import type { EditorHandle } from "../../components/Editor";
import { newDocument } from "../../lib/data";

afterEach(cleanup);

it("wires advanced-schema identity to the active document and isolates undo callbacks", () => {
  const a = newDocument(),
    b = newDocument();
  a.tab = b.tab = "schema";
  a.asset.schema = {
    type: "object",
    properties: { alpha: { type: "number" } },
  };
  b.asset.schema = { type: "object", properties: { beta: { type: "string" } } };
  const editA = vi.fn(),
    editB = vi.fn();
  const common = {
    items: [],
    code: createRef<EditorHandle>(),
    inspector: "definition",
    setPage: vi.fn(),
    setInspector: vi.fn(),
    discover: vi.fn(),
  };
  const { container, rerender } = render(
    <AuthorPanes
      controller={
        { ...common, doc: a, editDoc: editA } as unknown as WorkbenchController
      }
    />,
  );
  const getView = () =>
    EditorView.findFromDOM(container.querySelector(".cm-editor")!)!;
  const viewA = getView();
  act(() => {
    viewA.dispatch({
      changes: {
        from: 0,
        to: viewA.state.doc.length,
        insert: '{"properties":{"alphaEdited":{"type":"number"}}}',
      },
    });
  });
  expect(editA).toHaveBeenCalledOnce();
  rerender(
    <AuthorPanes
      controller={
        { ...common, doc: b, editDoc: editB } as unknown as WorkbenchController
      }
    />,
  );
  const viewB = getView();
  expect(viewB.state.doc.toString()).toContain("beta");
  act(() => {
    expect(undo(viewB)).toBe(false);
  });
  expect(editB).not.toHaveBeenCalled();
  expect(viewB.state.doc.toString()).toContain("beta");
});
