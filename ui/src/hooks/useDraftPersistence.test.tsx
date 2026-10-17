// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, render } from "@testing-library/react";
import { newDocument } from "../lib/data";
import { saveDrafts } from "../lib/drafts";
import { useDraftPersistence } from "./useDraftPersistence";

vi.mock("../lib/drafts", () => ({ saveDrafts: vi.fn(() => true) }));

function Harness({ source }: { source: string }) {
  const document = newDocument();
  document.asset.source = source;
  useDraftPersistence("workspace-test", [document], document.key, vi.fn());
  return null;
}

afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe("useDraftPersistence", () => {
  it("debounces writes and flushes the latest draft on pagehide", () => {
    vi.useFakeTimers();
    const view = render(<Harness source="first" />);
    view.rerender(<Harness source="latest" />);

    act(() => vi.advanceTimersByTime(349));
    expect(saveDrafts).not.toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(1));
    expect(saveDrafts).toHaveBeenCalledTimes(1);
    expect(vi.mocked(saveDrafts).mock.calls[0][1][0].asset.source).toBe("latest");

    vi.mocked(saveDrafts).mockClear();
    act(() => window.dispatchEvent(new Event("pagehide")));
    expect(saveDrafts).toHaveBeenCalledTimes(1);
    expect(vi.mocked(saveDrafts).mock.calls[0][1][0].asset.source).toBe("latest");
    view.unmount();
  });

  it("flushes and blocks unload when there are unsaved edits", () => {
    const preventDefault = vi.fn();
    render(<Harness source="unsaved" />);
    const event = new Event("beforeunload", { cancelable: true });
    event.preventDefault = preventDefault;
    act(() => window.dispatchEvent(event));
    expect(saveDrafts).toHaveBeenCalled();
    expect(preventDefault).toHaveBeenCalled();
  });
});
