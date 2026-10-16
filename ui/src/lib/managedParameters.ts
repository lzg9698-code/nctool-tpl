import { isLosslessNumber } from "lossless-json";
import { action } from "./api";
import { DraftNumber, clone, encode, type Data } from "./data";
import { cloneDraft } from "./drafts";

export interface ParameterPreview {
  values: Data;
  origins: Record<string, string>;
  definitions: Record<string, Data>;
}

// Render an origin record as text a user can act on.
export function originLabel(origin: string): string {
  if (origin === "definition") return "共享定义默认值";
  if (origin === "spec_default") return "规格默认值";
  if (origin === "template_default") return "模板默认值";
  if (origin === "manual") return "本次填写";
  return origin.startsWith("preset:")
    ? "共享预置：" + origin.slice("preset:".length)
    : origin;
}
export function isManual(origin?: string): boolean {
  return origin === "manual";
}
/// JSON Pointer decoding per RFC 6901; bindings reject these tokens already.
export function pointerTokens(pointer: string): string[] {
  return pointer
    .slice(1)
    .split("/")
    .map((token) => token.replace(/~1/g, "/").replace(/~0/g, "~"));
}
export function pointerEscape(token: string): string {
  return token.replace(/~/g, "~0").replace(/\//g, "~1");
}
function atPointer(value: Data, tokens: string[]): Data {
  let cursor = value;
  for (const token of tokens) {
    if (cursor === undefined || cursor === null) return undefined;
    if (Array.isArray(cursor)) {
      const index = Number(token);
      cursor = Number.isInteger(index) ? cursor[index] : undefined;
    } else cursor = cursor[token];
  }
  return cursor;
}
export function pointerValue(values: Data | undefined, pointer: string): Data {
  return values === undefined
    ? undefined
    : atPointer(values, pointerTokens(pointer));
}
function isPlainObject(value: Data): boolean {
  // LosslessNumber is a non-array object too; recursing into it would turn the
  // numeric token into a plain {value:"…"} object and lose exact literals.
  if (value === null || typeof value !== "object" || Array.isArray(value))
    return false;
  return !isLosslessNumber(value) && !(value instanceof DraftNumber);
}
/// Whether a value needs a structured placeholder instead of inline display.
/// Numeric tokens are objects at runtime but display as plain numbers.
export function isStructured(value: Data): boolean {
  return isPlainObject(value) || (Array.isArray(value) && value.length > 0);
}
function same(a: Data, b: Data): boolean {
  if (a === b) return true;
  if (a === undefined || b === undefined) return false;
  try {
    return encode(a) === encode(b);
  } catch {
    return false;
  }
}
function assign(into: Data, path: string[], value: Data) {
  if (!path.length) return;
  let cursor = into;
  for (const key of path.slice(0, -1)) {
    if (!Object.hasOwn(cursor, key) || !isPlainObject(cursor[key]))
      Object.defineProperty(cursor, key, { value: {}, enumerable: true, configurable: true, writable: true });
    cursor = cursor[key];
  }
  const last = path[path.length - 1];
  if (value === undefined) delete cursor[last];
  else Object.defineProperty(cursor, last, { value, enumerable: true, configurable: true, writable: true });
}
function walk(previous: Data, next: Data, path: string[], into: Data) {
  if (isPlainObject(previous) && isPlainObject(next)) {
    for (const key of new Set([
      ...Object.keys(previous),
      ...Object.keys(next),
    ])) {
      const child = [...path, key];
      const before = previous[key];
      const after = next[key];
      if (isPlainObject(before) && isPlainObject(after))
        walk(before, after, child, into);
      else if (!same(before, after)) assign(into, child, after);
    }
    return;
  }
  if (!same(previous, next)) assign(into, path, next);
}
/// Keep only the leaves the user actually changed as this run's overrides.
/// The form reports the whole visible object, which mixes inherited values with
/// edited ones; copying it verbatim would freeze every inherited value into a
/// manual override and defeat the shared defaults and presets.
export function applyOverrides(
  previous: Data,
  next: Data,
  overrides: Data,
): Data {
  const result = clone(overrides || {});
  walk(previous, next, [], result);
  return result;
}
/// Delete a pointer's leaf, pruning parents that would otherwise be left empty.
/// Paths through an array index are rejected: removing or skipping an element
/// shifts the rest, so the pointer would no longer name the same value.
export function removePointer(value: Data, pointer: string): Data {
  const tokens = pointerTokens(pointer);
  const removed = removeAt(value, tokens);
  return removed === undefined ? {} : removed;
}
function removeAt(value: Data, tokens: string[]): Data {
  if (!tokens.length) return undefined;
  const [head, ...rest] = tokens;
  if (Array.isArray(value)) return value;
  if (!isPlainObject(value)) return value;
  const object: Record<string, Data> = { ...value };
  if (rest.length) {
    if (!(head in object)) return object;
    const child = removeAt(object[head], rest);
    if (child === undefined) delete object[head];
    else object[head] = child;
    // Prune the parent once it holds nothing the user still needs.
    return !Object.keys(object[head] ?? {}).length
      ? prune(object, head)
      : object;
  }
  if (head in object) delete object[head];
  return object;
}
function prune(object: Record<string, Data>, key: string): Data {
  delete object[key];
  return Object.keys(object).length ? object : {};
}

/// Resolve the effective values exactly as the backend does, for the preview.
/// Returns the raw response so callers can surface validation errors verbatim.
export async function previewParameters(request: {
  bindings: Data;
  defaults?: Data;
  values?: Data;
  schema?: Data;
  preset_id?: string | null;
}): Promise<ParameterPreview> {
  const payload: Data = { bindings: request.bindings };
  if (request.defaults !== undefined) payload.defaults = request.defaults;
  if (request.values !== undefined) payload.values = request.values;
  if (request.schema !== undefined) payload.schema = request.schema;
  // An explicit decline must reach the backend; an absent field must stay absent.
  if (request.preset_id !== undefined) payload.preset_id = request.preset_id;
  const result = await action("parameter.resolve", payload);
  return {
    values: result.data.values,
    origins: result.data.origins || {},
    definitions: result.data.definitions || {},
  };
}

/// The request the backend derives for this document, so the preview matches generation.
export function managedRequest(doc: {
  asset: { defaults?: Data; schema?: Data; metadata?: Record<string, Data> };
  context: Data;
  parameterSelection?: Data;
}): Data {
  const request: Data = {
    bindings: doc.asset.metadata?.parameters?.bindings || {},
    values: cloneDraft(doc.context || {}),
    defaults: cloneDraft(doc.asset.defaults || {}),
  };
  if (doc.asset.schema !== undefined) request.schema = cloneDraft(doc.asset.schema);
  // The resolve endpoint has no template asset to inherit from, so include the
  // same saved choice that generation reads. Keep an explicit null intact.
  const selection = doc.parameterSelection ?? doc.asset.metadata?.parameters?.selection;
  if (selection && Object.hasOwn(selection, "preset_id"))
    request.preset_id = selection.preset_id;
  else if (Object.hasOwn(doc.asset.metadata?.parameters || {}, "preset_id"))
    request.preset_id = doc.asset.metadata!.parameters.preset_id;
  return request;
}
/// Draft-safe backend merge: objects merge recursively, arrays and numeric tokens
/// are atomic. Incomplete numbers remain editable until request serialization.
export function mergeParameterLayers(base: Data, overrides: Data): Data {
  if (!isPlainObject(overrides)) return cloneDraft(overrides);
  const result = isPlainObject(base) ? cloneDraft(base) : {};
  for (const [key, value] of Object.entries(overrides))
    Object.defineProperty(result, key, {
      value: isPlainObject(value) ? mergeParameterLayers(result[key], value) : cloneDraft(value),
      enumerable: true, configurable: true, writable: true,
    });
  return result;
}

/// A restore/edit below an array always targets the complete array. Deleting an
/// index would create a hole or shift the binding to a different element.
export function overridePointer(values: Data, pointer: string): string {
  const tokens = pointerTokens(pointer);
  let cursor = values;
  const walked: string[] = [];
  for (const token of tokens) {
    if (Array.isArray(cursor)) return "/" + walked.map(pointerEscape).join("/");
    walked.push(token);
    cursor = cursor?.[token];
  }
  return pointer;
}

/// Record the path the user touched, including an explicit equal value. Never
/// diff effective values: equality does not mean the user chose inheritance.
export function setParameterOverride(overrides: Data, effective: Data, pointer: string, value: Data): Data {
  const target = overridePointer(effective, pointer);
  const result = cloneDraft(overrides || {});
  if (target !== pointer) {
    const changed = cloneDraft(effective);
    const tokens = pointerTokens(pointer);
    let cursor = changed;
    for (const token of tokens.slice(0, -1)) cursor = cursor[token];
    const last = tokens.at(-1)!;
    // Clearing a field within an array object is safe; clearing an array item
    // restores the whole array rather than leaving undefined/hole entries.
    if (value === undefined && Array.isArray(cursor)) return removePointer(result, target);
    if (value === undefined) delete cursor[last];
    else Object.defineProperty(cursor, last, { value: cloneDraft(value), enumerable: true, configurable: true, writable: true });
    assign(result, pointerTokens(target), pointerValue(changed, target));
  } else if (value === undefined) return removePointer(result, target);
  else assign(result, pointerTokens(target), cloneDraft(value));
  return result;
}

/// Schema-backed fields render from the schema; the rest come from the context.
export function managedFields(asset?: { schema?: Data }): string[] {
  const schema = asset?.schema;
  if (!schema || typeof schema !== "object") return [];
  return Object.keys(
    (schema as { properties?: Record<string, Data> }).properties || {},
  );
}

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
