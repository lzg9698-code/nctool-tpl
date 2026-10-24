import { expect, it } from "vitest";
import { artifactArchive, crc32 } from "./artifactArchive";
it("使用标准 CRC32 并保留文件边界和 CRLF", () => {
  expect(crc32(new TextEncoder().encode("123456789"))).toBe(0xcbf43926);
  const bytes = artifactArchive([
      { name: "path.SPF", media_type: "text/plain", text: "G1 X1\r\nM17\r\n" },
    ]),
    v = new DataView(bytes.buffer);
  expect(v.getUint32(0, true)).toBe(0x04034b50);
  expect(v.getUint16(6, true)).toBe(0x800);
  expect(v.getUint32(bytes.length - 22, true)).toBe(0x06054b50);
  expect(new TextDecoder().decode(bytes)).toContain("G1 X1\r\nM17\r\n");
});
it("拒绝不完整或文件名冲突的整体导出", () => {
  expect(() => artifactArchive([])).toThrow();
  expect(() =>
    artifactArchive([{ name: "../bad", text: "", media_type: "" }]),
  ).toThrow();
  expect(() =>
    artifactArchive([
      { name: "a", text: "", media_type: "" },
      { name: "A", text: "", media_type: "" },
    ]),
  ).toThrow();
});
