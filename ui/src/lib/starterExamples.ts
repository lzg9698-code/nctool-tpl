import type { Data } from "./data";

export type StarterExampleId = "plain-text" | "shared-parameters";

export interface StarterExample {
  id: StarterExampleId;
  title: string;
  description: string;
  source: string;
  expected: string;
  bindings: Record<string, string>;
  defaults: Data;
  outputName: string;
  executionAction: "template.render" | "template.render.managed";
  schema?: Data;
  definition?: Data;
  preset?: Data;
}

export interface StarterBundle {
  format: "nctool-workspace";
  version: 1;
  collections: Record<string, Data>;
  manifest: Data[];
}

const examples: Record<StarterExampleId, Omit<StarterExample, "bindings" | "defaults" | "outputName" | "executionAction">> = {
  "plain-text": {
    id: "plain-text",
    title: "普通文本",
    description: "从上下文读取姓名，渲染一段问候文本。",
    source: "你好，{{ user.name }}！",
    expected: "你好，Ada！",
  },
  "shared-parameters": {
    id: "shared-parameters",
    title: "共享参数",
    description: "模板把 /report/name 绑定到共享定义，并可由共享预置提供值。",
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
    definition: {
      schema: { type: "string" },
      default: "未命名",
    },
    preset: { values: {} },
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
  const definitionId = `example-name-${suffix()}`;
  const presetId = `example-report-${suffix()}`;
  const templateId = `example-report-${suffix()}`;
  const base = examples[id];
  const shared = id === "shared-parameters";
  const definition = shared ? { ...base.definition, title: "名称", description: "报告中显示的名称", id: definitionId, format_version: 1 } : undefined;
  const preset = shared ? { title: "示例报告", values: { [definitionId]: "零件 A" }, id: presetId, format_version: 1 } : undefined;
  const example: StarterExample = {
    ...base,
    defaults: shared ? { report: { name: "未命名", unit: "件" } } : { user: { name: "Ada" } },
    outputName: `${templateId}.txt`,
    executionAction: shared ? "template.render.managed" : "template.render",
    bindings: shared ? { "/report/name": definitionId } : {},
    ...(shared ? { definition, preset } : {}),
  };
  const templateAsset = {
    source: example.source,
    defaults: example.defaults,
    ...(shared ? { schema: example.schema } : {}),
    metadata: shared
      ? { title: base.title, output_name: example.outputName, execution: { action: example.executionAction }, parameters: { bindings: example.bindings, preset_id: presetId } }
      : { title: base.title, output_name: example.outputName, execution: { action: example.executionAction } },
  };
  const records: Data[] = [{ group: "templates", name: templateId, asset: templateAsset }];
  const collections: Record<string, Data> = { template: collection(records) };
  if (shared) {
    collections.parameters = collection([
      { group: "definitions", name: definitionId, asset: example.definition },
      { group: "presets", name: presetId, asset: example.preset },
    ]);
  }
  const manifest = Object.entries(collections).map(([name, data]) => ({
    collection: name,
    provider: name === "parameters" ? "parameters" : "template",
    version: 1,
    fingerprint: fingerprint(data),
  }));
  return { example, bundle: { format: "nctool-workspace", version: 1, collections, manifest } };
}
