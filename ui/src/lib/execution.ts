import { encode, parseData, fieldErrors, type Data, type Schema } from "./data";
import { encodeDraft } from "./drafts";

export function signature(action: string, input: Data, raw?: string): string {
  return encodeDraft({ action, input, raw });
}
// Read the raw editor at dispatch time: an invalid draft never substitutes an old value.
export function validPluginInput(
  schema: Schema,
  value: Data,
  raw?: string,
): Data {
  const input = raw === undefined ? value : parseData(raw);
  if (
    schema.type === "object" &&
    (input === null || Array.isArray(input) || typeof input !== "object")
  )
    throw new Error("插件输入必须是 JSON 对象");
  encode(input);
  if (Object.keys(fieldErrors(schema, input)).length)
    throw new Error("请补齐或修正标记的参数");
  return input;
}
