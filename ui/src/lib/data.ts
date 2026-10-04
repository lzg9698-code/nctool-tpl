import {
  parse,
  stringify,
  LosslessNumber,
  isLosslessNumber,
} from "lossless-json";
export type Data = any;
export interface Schema {
  type?: string | string[];
  title?: string;
  description?: string;
  properties?: Record<string, Schema>;
  required?: string[];
  items?: Schema;
  enum?: Data[];
  default?: Data;
  minimum?: Data;
  maximum?: Data;
  [key: string]: Data;
}
export interface Asset {
  source: string;
  schema?: Schema;
  defaults?: Data;
  tags?: string[];
  metadata?: Record<string, Data>;
  [key: string]: Data;
}
export interface Document {
  key: string;
  id: string | null;
  title: string;
  asset: Asset;
  context: Data;
  fingerprint: string | null;
  saved: string;
  mode: "use" | "edit";
  tab: "source" | "schema" | "settings";
  outputName: string;
  generationAction?: string;
  jsonInput?: string;
  lastRunId?: string;
  lastRunSignature?: string;
}
export interface Diagnostic {
  level: string;
  code: string;
  message: string;
  path?: string;
}
export interface Run {
  id: string;
  title: string;
  action: string;
  input: Data;
  status: string;
  started_at: Data;
  finished_at?: Data;
  result?: {
    data: Data;
    artifacts: { name: string; media_type: string; text: string }[];
    diagnostics: Diagnostic[];
  };
  error?: { message: string; code: string; diagnostics: Diagnostic[] };
  environment?: Data;
  replay?: { source_run_id: string; check: ReplayCheck };
}
export interface ReplayCheck {
  status: "matching" | "changed" | "unknown";
  differences: string[];
}
export class DraftNumber {
  constructor(public raw: string) {}
}
const NUMBER = /^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?$/;
export function parseData(text: string): Data {
  return parse(text);
}
export function encode(data: Data, pretty = false): string {
  const result = stringify(
    data,
    (_key, value) => {
      if (value instanceof DraftNumber) {
        if (!NUMBER.test(value.raw))
          throw new Error("请输入完整、有效的数字：" + value.raw);
        return new LosslessNumber(value.raw);
      }
      return value;
    },
    pretty ? 2 : undefined,
  );
  if (result === undefined) throw new Error("输入数据为空");
  return result;
}
export function clone<T>(value: T): T {
  return parseData(encode(value)) as T;
}
export function numberText(value: Data): string {
  return value instanceof DraftNumber
    ? value.raw
    : isLosslessNumber(value)
      ? value.value
      : value === undefined || value === null
        ? ""
        : String(value);
}
export function safeNumber(value: Data): number {
  return Number(numberText(value));
}
export function typeOf(schema: Schema): string {
  return Array.isArray(schema.type)
    ? schema.type.find((type) => type !== "null") || "string"
    : schema.type || "unknown";
}
export function defaultValue(schema: Schema): Data {
  if (schema.default !== undefined) return clone(schema.default);
  switch (typeOf(schema)) {
    case "object":
      return Object.fromEntries(
        Object.entries(schema.properties || {})
          .filter(([, s]) => s.default !== undefined || typeOf(s) === "object")
          .map(([k, s]) => [k, defaultValue(s)]),
      );
    case "array":
      return [];
    case "boolean":
      return false;
    default:
      return undefined;
  }
}
export function mergeDefaults(defaults: Data, values: Data): Data {
  return { ...(defaults || {}), ...(values || {}) };
}
export function keyFor(...parts: string[]): string {
  return parts.map(encodeURIComponent).join(":");
}
export function documentValue(doc: Document): string {
  return encode({
    title: doc.title,
    asset: doc.asset,
    outputName: doc.outputName,
    generationAction: doc.generationAction,
  });
}
export function isDirty(doc: Document): boolean {
  try {
    return documentValue(doc) !== doc.saved;
  } catch {
    return true;
  }
}
export function newDocument(): Document {
  return {
    key: crypto.randomUUID(),
    id: null,
    title: "未命名模板",
    asset: {
      source: "",
      schema: { type: "object", properties: {}, required: [] },
      defaults: {},
      tags: [],
      metadata: {},
    },
    context: {},
    fingerprint: null,
    saved: "",
    mode: "edit",
    tab: "source",
    outputName: "output.txt",
  };
}
export function fieldErrors(
  schema: Schema,
  value: Data,
  path = "",
): Record<string, string> {
  const errors: Record<string, string> = {};
  const type = typeOf(schema);
  if (schema["x-untyped"]) {
    errors[path] = "请先在编辑模式定义参数类型";
    return errors;
  }
  if (value === undefined) return errors;
  if (type === "object") {
    for (const name of schema.required || []) {
      if (
        value?.[name] === undefined ||
        (value?.[name] === null &&
          !schema.properties?.[name]?.type?.includes("null")) ||
        value?.[name] === ""
      )
        errors[path + "/" + name] =
          "请填写" + (schema.properties?.[name]?.title || name);
    }
    for (const [name, child] of Object.entries(schema.properties || {}))
      Object.assign(
        errors,
        fieldErrors(child, value?.[name], path + "/" + name),
      );
  } else if (type === "array" && Array.isArray(value)) {
    value.forEach((v, i) =>
      Object.assign(errors, fieldErrors(schema.items || {}, v, path + "/" + i)),
    );
  } else if (type === "number" || type === "integer") {
    const text = numberText(value);
    if (!NUMBER.test(text)) errors[path] = "请输入有效数字";
    else if (type === "integer" && !/^-?\d+$/.test(text))
      errors[path] = "请输入整数";
  }
  return errors;
}
export function download(text: string, name: string, type = "text/plain") {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
export function diagnosticsFrom(error: Data): Diagnostic[] {
  return error?.diagnostics?.length
    ? error.diagnostics
    : [
        {
          level: "error",
          code: error?.code || "error",
          message: error?.message || "操作失败",
        },
      ];
}
