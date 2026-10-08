import { clone, type Data } from "./data";

export function validatedSelection(selection: Data): Data {
  for (const choice of Object.values(selection?.slots || {}) as Data[]) {
    if (choice.inputs_draft !== undefined || choice.overrides_draft !== undefined)
      throw new Error("请先修复刀具加工输入或人工调整的 JSON 草稿");
  }
  return selection;
}

export function managedRawInput(value: Data, bindings: Data, selection: Data): Data {
  const result = clone(value);
  for (const [path, binding] of Object.entries(bindings || {}) as [string, Data][]) {
    if (!selection?.slots?.[binding.slot]?.tool_id) continue;
    const keys = path.slice(1).split("/").map(k => k.replace(/~1/g, "/").replace(/~0/g, "~"));
    let cursor = result;
    for (const key of keys.slice(0, -1)) cursor = cursor?.[key];
    if (cursor && typeof cursor === "object") delete cursor[keys.at(-1)!];
  }
  return result;
}
export function managedFormSchema(schema: Data, bindings: Data, selection: Data): Data {
  const result = clone(schema);
  for (const [path, binding] of Object.entries(bindings || {}) as [string, Data][]) {
    if (!selection?.slots?.[binding.slot]?.tool_id) continue;
    let cursor = result;
    for (const key of path.slice(1).split("/")) cursor = cursor?.properties?.[key.replace(/~1/g, "/").replace(/~0/g, "~")];
    if (cursor) cursor.readOnly = true;
  }
  return result;
}
