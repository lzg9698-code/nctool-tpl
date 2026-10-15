// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { newDocument, type Run } from "../../lib/data";
import { ResultPane } from "./ResultPane";
import type { WorkbenchController } from "./useWorkbenchController";
vi.mock("../../components/Editor", () => ({
  Editor: ({ value }: { value: string }) => <pre>{value}</pre>,
}));
afterEach(cleanup);
function controller(status: string, reason: string) {
  const run: Run = {
    id: "r1",
    title: "模板",
    status,
    action: "template.render",
    input: {},
    started_at: 0,
    result: {
      data: { text: "正文" },
      diagnostics: [],
      artifacts: [
        { name: "one.txt", text: "第一份", media_type: "text/plain" },
        { name: "two.txt", text: "第二份", media_type: "text/plain" },
      ],
    },
  };
  return {
    doc: newDocument(),
    latest: run,
    resultTab: "result",
    diagnostics: [],
    stale: !!reason,
    busy: status === "running",
    managedDependencies: {},
    text: "正文",
    exportReason: reason,
    editDoc: vi.fn(),
    setResultTab: vi.fn(),
    notify: vi.fn(),
    fail: vi.fn(),
  } as unknown as WorkbenchController;
}
describe("result delivery controls", () => {
  it.each(["failed", "cancelled", "interrupted", "running", "succeeded"])(
    "blocks all current delivery for %s with an export reason",
    (status) => {
      render(
        <ResultPane controller={controller(status, "当前结果不可交付")} />,
      );
      expect(
        (screen.getByRole("button", { name: "复制结果" }) as HTMLButtonElement)
          .disabled,
      ).toBe(true);
      for (const button of screen.getAllByRole("button", { name: /^导出/ }))
        expect((button as HTMLButtonElement).disabled).toBe(true);
    },
  );
  it("enables every artifact only after complete successful generation", () => {
    render(<ResultPane controller={controller("succeeded", "")} />);
    expect(
      (screen.getByRole("button", { name: "复制结果" }) as HTMLButtonElement)
        .disabled,
    ).toBe(false);
    expect(
      (
        screen.getByRole("button", {
          name: "导出 two.txt",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false);
  });
});
