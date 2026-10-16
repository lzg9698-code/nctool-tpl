import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { HistoryPage } from "./HistoryPage";
import type { Run } from "../lib/data";
vi.mock("./Editor", () => ({
  Editor: ({ value }: { value: string }) => <pre>{value}</pre>,
}));
vi.mock("./DiffView", () => ({ DiffView: () => <div>历史比较</div> }));
function markup(action: string, available: string[]) {
  const run: Run = {
    id: "r1",
    title: "旧记录",
    action,
    input: {},
    status: "succeeded",
    started_at: 0,
    result: {
      data: { text: "saved output" },
      artifacts: [
        { name: "saved.txt", media_type: "text/plain", text: "saved output" },
        { name: "second.txt", media_type: "text/plain", text: "second output" },
      ],
      diagnostics: [],
    },
  };
  return renderToStaticMarkup(
    <HistoryPage
      actions={available.map((id) => ({ id }))}
      runList={[run, { ...run, id: "r0" }]}
      selectedHistory={run}
      comparison={null}
      setRunList={() => {}}
      setSelectedHistory={() => {}}
      setComparison={() => {}}
      restoreRun={vi.fn()}
      onCreated={() => {}}
      notify={() => {}}
      fail={() => {}}
    />,
  );
}
describe("history controls follow current providers", () => {
  it("disables rerun and restore while retaining historical text, artifacts and comparison", () => {
    const html = markup("nc.generate", []);
    expect(html).toMatch(
      /<button[^>]*disabled=""[^>]*>按历史输入重新执行<\/button>/,
    );
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>恢复输入<\/button>/);
    expect(html).toContain("saved output");
    expect(html).toContain("当前没有可用提供方");
    expect(html).toContain("比较历史记录");
    expect(html).toMatch(/<button[^>]*>[^<]*导出 second.txt<\/button>/);
    expect(html).not.toMatch(
      /<button[^>]*disabled=""[^>]*>[^<]*导出 second.txt/,
    );
  });
  it("keeps NC history actionable when the NC bundle provides its action", () => {
    const html = markup("nc.generate", ["nc.generate"]);
    expect(html).not.toMatch(/<button[^>]*disabled=""[^>]*>恢复输入<\/button>/);
    expect(html).not.toContain("当前没有可用提供方");
  });
  it("disables missing external actions too, and enables generic managed recovery", () => {
    expect(markup("external.render", [])).toContain("当前没有可用提供方");
    const html = markup("template.render.managed", ["template.render.managed"]);
    expect(html).not.toMatch(/<button[^>]*disabled=""[^>]*>恢复输入<\/button>/);
    expect(html).not.toMatch(
      /<button[^>]*disabled=""[^>]*>按历史输入重新执行<\/button>/,
    );
  });
});
