// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { TemplateWelcome, type WelcomeTemplate } from "./TemplateWelcome";

vi.mock("./StarterExamples", () => ({
  StarterExamples: ({ onSaved }: { onSaved: () => void }) => (
    <button onClick={onSaved}>保存入门示例</button>
  ),
}));

const templates: WelcomeTemplate[] = Array.from({ length: 5 }, (_, index) => ({
  id: `template-${index}`,
  title: `模板 ${index}`,
  description: index === 0 ? "" : `说明 ${index}`,
  tags: [],
  fingerprint: `fp-${index}`,
}));

describe("TemplateWelcome", () => {
  it("wires create/import/open actions and limits recent templates to four", () => {
    const onCreate = vi.fn();
    const onImport = vi.fn();
    const onOpen = vi.fn();
    const onSaved = vi.fn();
    render(
      <TemplateWelcome
        items={templates}
        onCreate={onCreate}
        onImport={onImport}
        onOpen={onOpen}
        onSaved={onSaved}
        emptyReady={false}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "新建模板" }));
    fireEvent.click(screen.getByRole("button", { name: "导入模板" }));
    fireEvent.click(screen.getByRole("button", { name: /模板 0/ }));

    expect(onCreate).toHaveBeenCalledTimes(1);
    expect(onImport).toHaveBeenCalledTimes(1);
    expect(onOpen).toHaveBeenCalledWith(templates[0]);
    expect(screen.getAllByRole("button", { name: /模板 [0-9]/ })).toHaveLength(4);
    expect(screen.getByText("填写参数并生成文件")).toBeTruthy();
  });

  it("shows lazy starter examples only when the catalog is ready and empty", async () => {
    const onSaved = vi.fn();
    const view = render(
      <TemplateWelcome
        items={[]}
        onCreate={vi.fn()}
        onImport={vi.fn()}
        onOpen={vi.fn()}
        onSaved={onSaved}
        emptyReady={false}
      />,
    );
    expect(screen.queryByRole("button", { name: "保存入门示例" })).toBeNull();
    view.rerender(
      <TemplateWelcome
        items={[]}
        onCreate={vi.fn()}
        onImport={vi.fn()}
        onOpen={vi.fn()}
        onSaved={onSaved}
        emptyReady
      />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "保存入门示例" }));
    expect(onSaved).toHaveBeenCalledTimes(1);
  });
});
