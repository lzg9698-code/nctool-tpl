import type { Data, Schema } from "./data";

export type StarterExampleId = "plain-text" | "parameter-report";

export interface StarterExample {
  id: StarterExampleId;
  title: string;
  description: string;
  source: string;
  expected: string;
  schema: Schema;
  defaults: Data;
  outputName: string;
}

export interface StarterBundle {
  format: "nctool-workspace";
  version: 1;
  collections: Record<string, Data>;
  manifest: Data[];
}

const examples: Record<StarterExampleId, Omit<StarterExample, "outputName">> = {
  "plain-text": {
    id: "plain-text",
    title: "普通文本",
    description: "在参数表单填写姓名，渲染一段问候文本。",
    source: "你好，{{ user.name }}！",
    expected: "你好，Ada！",
    schema: {
      type: "object",
      properties: {
        user: {
          type: "object",
          properties: {
            name: { type: "string", title: "姓名" },
          },
          required: ["name"],
        },
      },
      required: ["user"],
    },
    defaults: { user: { name: "Ada" } },
  },
  "parameter-report": {
    id: "parameter-report",
    title: "参数报告",
    description: "在参数表单填写名称和单位，使用模板内的默认值生成报告。",
    source: "报告：{{ report.name }}（{{ report.unit }}）",
    expected: "报告：零件 A（件）",
    schema: {
      type: "object",
      properties: {
        report: {
          type: "object",
          properties: {
            name: { type: "string", title: "名称" },
            unit: { type: "string", title: "单位" },
          },
          required: ["name", "unit"],
        },
      },
      required: ["report"],
    },
    defaults: { report: { name: "零件 A", unit: "件" } },
  },
};

// A short random suffix keeps repeated imports from colliding while staying
// readable in pickers and filenames, unlike a full UUID.
function suffix(): string {
  const bytes = new Uint8Array(5);
  if (globalThis.crypto?.getRandomValues) globalThis.crypto.getRandomValues(bytes);
  else for (let i = 0; i < bytes.length; i++) bytes[i] = Math.floor(Math.random() * 256);
  return Array.from(bytes, (byte) => "abcdefghijklmnopqrstuvwxyz0123456789"[byte % 36]).join("");
}

function canonical(value: Data): Data {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])]));
  }
  return value;
}

function fingerprint(data: Data): string {
  const bytes = new TextEncoder().encode(JSON.stringify(canonical(data)));
  let hash = 0xcbf29ce484222325n;
  for (const byte of bytes) hash = BigInt.asUintN(64, (hash ^ BigInt(byte)) * 0x100000001b3n);
  return `fnv1a64:${hash.toString(16).padStart(16, "0")}`;
}

function collection(records: Data[]): Data {
  return { version: 1, records };
}

export function buildStarterExample(id: StarterExampleId): { example: StarterExample; bundle: StarterBundle } {
  const templateId = `example-${id}-${suffix()}`;
  const example: StarterExample = {
    ...examples[id],
    outputName: `${templateId}.txt`,
  };
  const templateAsset = {
    source: example.source,
    schema: example.schema,
    defaults: example.defaults,
    metadata: { title: example.title, output_name: example.outputName, execution: { action: "template.render" } },
  };
  const records: Data[] = [{ group: "templates", name: templateId, asset: templateAsset }];
  const collections: Record<string, Data> = { template: collection(records) };
  const manifest = Object.entries(collections).map(([name, data]) => ({
    collection: name,
    provider: "template",
    version: 1,
    fingerprint: fingerprint(data),
  }));
  return { example, bundle: { format: "nctool-workspace", version: 1, collections, manifest } };
}
