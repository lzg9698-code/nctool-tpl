import { it, expect } from "vitest";
import { diffLines } from "diff";
it("identifies inserted and removed lines without shifting all subsequent lines", () => {
  const diff = diffLines("G0 X1\nG1 X2\nM30\n", "G0 X1\nG1 X3\nM8\nM30\n", {
    timeout: 1000,
  });
  expect(
    diff
      ?.filter((part) => part.added)
      .map((part) => part.value)
      .join(""),
  ).toBe("G1 X3\nM8\n");
  expect(
    diff
      ?.filter((part) => part.removed)
      .map((part) => part.value)
      .join(""),
  ).toBe("G1 X2\n");
  expect(diff?.at(-1)?.value).toBe("M30\n");
});
