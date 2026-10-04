import {
  DraftNumber,
  isDirty,
  newDocument,
  parseData,
  type Data,
  type Document,
} from "./data";
import { isLosslessNumber } from "lossless-json";
// Tagged every node: user objects never collide with numeric-token metadata.
function pack(value: Data): Data {
  if (value instanceof DraftNumber) return ["n", value.raw];
  if (isLosslessNumber(value)) return ["n", value.value];
  if (value === undefined) return ["u"];
  if (Array.isArray(value)) return ["a", value.map(pack)];
  if (value !== null && typeof value === "object")
    return [
      "o",
      Object.fromEntries(
        Object.entries(value).map(([key, v]) => [key, pack(v)]),
      ),
    ];
  return ["v", value];
}
function unpack(value: Data): Data {
  switch (value[0]) {
    case "n":
      try {
        return parseData(value[1]);
      } catch {
        return new DraftNumber(value[1]);
      }
    case "u":
      return undefined;
    case "a":
      return value[1].map(unpack);
    case "o":
      return Object.fromEntries(
        Object.entries(value[1]).map(([key, v]) => [key, unpack(v)]),
      );
    default:
      return value[1];
  }
}
export function saveDrafts(
  workspace: string,
  documents: Document[],
  active: string | null,
): boolean {
  try {
    localStorage.setItem(
      "nctool:workbench:" + workspace,
      JSON.stringify({ version: 1, active, documents: pack(documents) }),
    );
    return true;
  } catch {
    return false;
  }
}
export function loadDrafts(workspace: string): {
  documents: Document[];
  active: string | null;
} {
  try {
    const text = localStorage.getItem("nctool:workbench:" + workspace);
    if (text) {
      const data = JSON.parse(text);
      if (data.version === 1) {
        const documents = unpack(data.documents) as Document[];
        if (
          Array.isArray(documents) &&
          documents.every((doc) => doc.key && doc.asset?.source !== undefined)
        )
          return { documents, active: data.active };
      }
    }
  } catch {}
  return { documents: [], active: null };
}
export { isDirty, newDocument };

export function cloneDraft<T>(value: T): T {
  return unpack(pack(value)) as T;
}
export function encodeDraft(value: Data): string {
  return JSON.stringify(pack(value));
}
export function decodeDraft<T>(text: string): T {
  return unpack(JSON.parse(text)) as T;
}
