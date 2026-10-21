// @vitest-environment jsdom
import { useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { newDocument, encode } from "../lib/data";
import { SharedParameterInputs } from "./SharedParameterInputs";
vi.mock("../lib/api", () => ({ action: vi.fn(async () => ({ data: { items: [] } })) }));
vi.mock("../lib/managedParameters", async original => ({
  ...await original<typeof import("../lib/managedParameters")>(),
  previewParameters: vi.fn(async () => ({ values: { width: 25, title: "继承" }, origins: { "/width": "definition" }, definitions: {} })),
}));
vi.mock("./WorkbenchUi", () => ({ JsonEditor: () => null }));
afterEach(cleanup);
it("uses the real Form path event for sparse exact-value overrides and incomplete drafts", async () => {
  const change = vi.fn();
  function Harness() {
    const [doc, setDoc] = useState(() => {
      const initial = newDocument();
      initial.asset.schema = { type: "object", properties: { width: { type: "number", title: "宽度" }, title: { type: "string", title: "标题" } } };
      return initial;
    });
    return <SharedParameterInputs doc={doc} mode="form" onChange={patch => { change(patch); setDoc(previous => ({ ...previous, ...patch })); }} />;
  }
  render(<Harness />);
  await waitFor(() => expect((screen.getByLabelText("宽度") as HTMLInputElement).value).toBe("25"));
  fireEvent.change(screen.getByLabelText("宽度"), { target: { value: "9007199254740993" } });
  expect(encode(change.mock.lastCall![0].context)).toBe('{"width":9007199254740993}');
  fireEvent.change(screen.getByLabelText("宽度"), { target: { value: "25" } });
  expect(encode(change.mock.lastCall![0].context)).toBe('{"width":25}');
  fireEvent.change(screen.getByLabelText("宽度"), { target: { value: "1e-" } });
  expect((screen.getByLabelText("宽度") as HTMLInputElement).value).toBe("1e-");
  expect((screen.getByLabelText("标题") as HTMLInputElement).value).toBe("继承");
  expect(screen.getByText(/当前参数校验失败/)).toBeTruthy();
});
