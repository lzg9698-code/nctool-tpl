import type { Artifact } from "./contracts";
const encoder = new TextEncoder();
const crcTable = Uint32Array.from({ length: 256 }, (_, n) => {
  for (let i = 0; i < 8; i++) n = n & 1 ? 0xedb88320 ^ (n >>> 1) : n >>> 1;
  return n >>> 0;
});
export function crc32(bytes: Uint8Array): number {
  let n = 0xffffffff;
  for (const b of bytes) n = crcTable[(n ^ b) & 255] ^ (n >>> 8);
  return (n ^ 0xffffffff) >>> 0;
}
/** Stored ZIP entries preserve the exact UTF-8 text, including CRLF; no new RPC format. */
export function artifactArchive(
  artifacts: Artifact[],
): Uint8Array<ArrayBuffer> {
  if (!artifacts.length || artifacts.length > 2048)
    throw Error("产物数量超出限制");
  const names = new Set<string>(),
    files = artifacts.map((a) => {
      if (
        !a.name ||
        a.name.length > 255 ||
        /[\\/\u0000-\u001f]/.test(a.name) ||
        [".", ".."].includes(a.name) ||
        names.has(a.name.toLowerCase())
      )
        throw Error("产物文件名无效或重复");
      names.add(a.name.toLowerCase());
      const name = encoder.encode(a.name),
        data = encoder.encode(a.text);
      return { name, data, crc: crc32(data) };
    });
  const content = files.reduce(
      (n, f) => n + 30 + f.name.length + f.data.length,
      0,
    ),
    directory = files.reduce((n, f) => n + 46 + f.name.length, 0);
  if (content + directory + 22 > 64 * 1024 * 1024)
    throw Error("产物压缩包超过 64 MiB");
  const out = new Uint8Array(content + directory + 22),
    view = new DataView(out.buffer);
  let offset = 0,
    central = content;
  const u16 = (at: number, n: number) => view.setUint16(at, n, true),
    u32 = (at: number, n: number) => view.setUint32(at, n, true);
  for (const f of files) {
    u32(offset, 0x04034b50);
    u16(offset + 4, 20);
    u16(offset + 6, 0x800);
    u16(offset + 12, 33);
    u32(offset + 14, f.crc);
    u32(offset + 18, f.data.length);
    u32(offset + 22, f.data.length);
    u16(offset + 26, f.name.length);
    out.set(f.name, offset + 30);
    out.set(f.data, offset + 30 + f.name.length);
    u32(central, 0x02014b50);
    u16(central + 4, 20);
    u16(central + 6, 20);
    u16(central + 8, 0x800);
    u16(central + 14, 33);
    u32(central + 16, f.crc);
    u32(central + 20, f.data.length);
    u32(central + 24, f.data.length);
    u16(central + 28, f.name.length);
    u32(central + 42, offset);
    out.set(f.name, central + 46);
    offset += 30 + f.name.length + f.data.length;
    central += 46 + f.name.length;
  }
  u32(central, 0x06054b50);
  u16(central + 8, files.length);
  u16(central + 10, files.length);
  u32(central + 12, directory);
  u32(central + 16, content);
  return out;
}
