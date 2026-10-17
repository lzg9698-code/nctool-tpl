// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { ConflictDialog } from "./ConflictDialog";

describe("ConflictDialog", () => {
  it("shows both versions and delegates each explicit resolution choice", () => {
    const onContinue = vi.fn();
    const onLoadDisk = vi.fn();
    const onSaveCopy = vi.fn();
    render(
      <ConflictDialog
        draftSource="draft version"
        diskAsset={{ source: "disk version" }}
        onContinue={onContinue}
        onLoadDisk={onLoadDisk}
        onSaveCopy={onSaveCopy}
      />,
    );
    expect(screen.getByText("draft version")).toBeTruthy();
    expect(screen.getByText("disk version")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "继续编辑" }));
    fireEvent.click(screen.getByRole("button", { name: "加载磁盘版本" }));
    fireEvent.click(screen.getByRole("button", { name: "另存为副本" }));
    expect(onContinue).toHaveBeenCalledTimes(1);
    expect(onLoadDisk).toHaveBeenCalledTimes(1);
    expect(onSaveCopy).toHaveBeenCalledTimes(1);
  });
});
