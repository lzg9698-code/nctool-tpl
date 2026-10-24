import { encode, type Data } from "../../lib/json";
function ordered(value: Data): Data {
  if (Array.isArray(value)) return value.map(ordered);
  if (value && typeof value === "object")
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, ordered(value[key])]),
    );
  return value;
}
// The workspace bundle API requires the canonical collection fingerprint.
export function collectionFingerprint(value: Data): string {
  let hash = 14695981039346656037n;
  for (const byte of new TextEncoder().encode(encode(ordered(value))))
    hash = BigInt.asUintN(64, (hash ^ BigInt(byte)) * 1099511628211n);
  return "fnv1a64:" + hash.toString(16).padStart(16, "0");
}
export function sampleBundle(shared: boolean) {
  const suffix = crypto.randomUUID().slice(0, 8),
    name = "sample-" + suffix,
    definition = name + "-name",
    preset = name + "-preset";
  const asset = {
    source: shared ? "报告：{{ name }}（{{ unit }}）" : "你好，{{ name }}！",
    schema: {
      type: "object",
      properties: {
        name: { type: "string", title: "名称" },
        ...(shared ? { unit: { type: "string", title: "单位" } } : {}),
      },
      required: ["name"],
    },
    defaults: shared ? { unit: "件" } : { name: "Ada" },
    metadata: {
      title: shared ? "共享参数示例" : "问候文本示例",
      execution: {
        action: shared ? "template.render.managed" : "template.render",
      },
      ...(shared
        ? {
            parameters: {
              bindings: { "/name": definition },
              preset_id: preset,
            },
          }
        : {}),
    },
  };
  const collections: Data = {
    template: { version: 1, records: [{ group: "templates", name, asset }] },
  };
  if (shared)
    collections.parameters = {
      version: 1,
      records: [
        {
          group: "definitions",
          name: definition,
          asset: {
            format_version: 1,
            id: definition,
            title: "报告名称",
            schema: { type: "string" },
            default: "未命名",
          },
        },
        {
          group: "presets",
          name: preset,
          asset: {
            format_version: 1,
            id: preset,
            title: "报告示例预置",
            values: { [definition]: "零件 A" },
          },
        },
      ],
    };
  const bundle = {
    format: "nctool-workspace",
    version: 1,
    collections,
    manifest: Object.entries(collections).map(([collection, value]) => ({
      collection,
      name: collection,
      provider: collection === "parameters" ? "parameters" : "template",
      version: 1,
      fingerprint: collectionFingerprint(value),
    })),
  };
  return {
    name,
    asset,
    bundle,
    context: shared ? { name: "零件 A", unit: "件" } : { name: "Ada" },
    expected: shared ? "报告：零件 A（件）" : "你好，Ada！",
  };
}
