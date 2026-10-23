import {
  parse,
  stringify,
  isLosslessNumber,
  LosslessNumber,
} from "lossless-json";

// Dynamic plugin payloads are intentionally open; UI state and transport are typed separately.
// eslint is not part of this repository's toolchain.
export type Data = any;
export type Schema = Record<string, Data> | boolean;
export function decode(text: string): Data {
  return parse(text);
}
export function encode(value: Data, pretty = false): string {
  const result = stringify(value, undefined, pretty ? 2 : undefined);
  if (result === undefined) throw new Error("输入不能为空");
  return result;
}
export function numericText(value: Data): string {
  return isLosslessNumber(value)
    ? value.value
    : value == null
      ? ""
      : String(value);
}
export function numeric(raw: string): LosslessNumber {
  if (!/^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?$/.test(raw))
    throw new Error("请输入完整数字");
  return new LosslessNumber(raw);
}
export function object(value: Data): value is Record<string, Data> {
  return (
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    !isLosslessNumber(value)
  );
}
export function merge(base: Data, overrides: Data): Data {
  if (!object(overrides)) return overrides;
  return Object.fromEntries(
    [
      ...new Set([
        ...Object.keys(object(base) ? base : {}),
        ...Object.keys(overrides),
      ]),
    ].map((key) => [
      key,
      Object.hasOwn(overrides, key)
        ? merge(base?.[key], overrides[key])
        : base[key],
    ]),
  );
}
export function defaultFor(schema: Schema): Data {
  if (typeof schema !== "object") return undefined;
  if (Object.hasOwn(schema, "default")) return schema.default;
  if (schema.type === "object")
    return Object.fromEntries(
      Object.entries(schema.properties || {})
        .map(([key, field]) => [key, defaultFor(field as Schema)])
        .filter(([, value]) => value !== undefined),
    );
  return undefined;
}
export function pointer(path: (string | number)[]): string {
  return path.length
    ? "/" +
        path
          .map((key) => String(key).replace(/~/g, "~0").replace(/\//g, "~1"))
          .join("/")
    : "";
}
export function getAt(value: Data, path: (string | number)[]): Data {
  return path.reduce((node, key) => node?.[key], value);
}
export function setAt(
  value: Data,
  path: (string | number)[],
  next: Data,
): Data {
  if (!path.length) return next;
  const [key, ...rest] = path;
  if (Array.isArray(value)) {
    const copy = [...value];
    copy[Number(key)] = setAt(copy[Number(key)], rest, next);
    return copy;
  }
  const copy = { ...(object(value) ? value : {}) };
  if (next === undefined && !rest.length) delete copy[key];
  else
    Object.defineProperty(copy, key, {
      value: setAt(copy[key], rest, next),
      enumerable: true,
      writable: true,
      configurable: true,
    });
  return copy;
}
export function removeAt(value: Data, path: (string | number)[]): Data {
  if (!path.length) return {};
  const [key, ...rest] = path;
  if (!object(value)) return value;
  const copy = { ...value };
  if (rest.length) {
    const child = removeAt(copy[key], rest);
    if (object(child) && !Object.keys(child).length) delete copy[key];
    else copy[key] = child;
  } else delete copy[key];
  return copy;
}
export function overrideAt(
  overrides: Data,
  effective: Data,
  path: (string | number)[],
  value: Data,
): Data {
  const firstArray = path.findIndex((_, index) =>
    Array.isArray(getAt(effective, path.slice(0, index))),
  );
  if (firstArray >= 0) {
    const root = path.slice(0, firstArray);
    return value === undefined
      ? removeAt(overrides, root)
      : setAt(overrides, root, getAt(setAt(effective, path, value), root));
  }
  return value === undefined
    ? removeAt(overrides, path)
    : setAt(overrides, path, value);
}
export function saveFile(
  name: string,
  text: string,
  mediaType = "text/plain;charset=utf-8",
) {
  const url = URL.createObjectURL(new Blob([text], { type: mediaType }));
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
export const errorText = (error: unknown) =>
  error instanceof Error ? error.message : String(error);
