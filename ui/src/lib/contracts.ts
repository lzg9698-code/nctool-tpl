// Host-owned transport contracts. Plugin data, asset metadata and Schema keywords
// remain open; validating the host envelope must never coerce their JSON numbers.
import { isLosslessNumber } from "lossless-json";
import { type Data, type Schema, object } from "./json";

export interface Diagnostic {
  level: string;
  code: string;
  message: string;
  path?: string | null;
  template?: string;
  line?: number;
  column?: number;
}
export interface PluginError {
  code: string;
  message: string;
  diagnostics?: Diagnostic[];
}
export interface Artifact {
  name: string;
  media_type: string;
  text: string;
}
export interface ActionResult<T = Data> {
  data: T;
  artifacts: Artifact[];
  diagnostics: Diagnostic[];
}
export interface ActionDescriptor {
  id: string;
  title: string;
  input_schema: Schema;
  output_schema: Schema;
  example?: Data;
  plugin?: string;
  ui_schema?: Data;
}
export interface Panel {
  id: string;
  title: string;
  actions: string[];
  view: string;
}
export interface PluginDescriptor {
  id: string;
  version: string;
  protocol_version: number;
  requires: { id: string; version: number }[];
  provides: { id: string; version: number; action: string }[];
  config_schema: Schema;
  panels: Panel[];
  asset_collections?: Data[];
}
export interface ExecutionEnvironment {
  format_version: number;
  host_version: string;
  plugins: Record<string, { version: string; protocol_version: number }>;
  services: Record<string, [number, string, string]>;
  provenance?: {
    scope: string;
    host: string | null;
    plugins: Record<string, { package: string | null; public_config: string }>;
  };
}
export interface CallRecord {
  trace_id: string;
  call_id: number;
  action: string;
  owner: string;
  parent_action: string | null;
  depth: number;
  started_at: number;
  elapsed_us: number;
  error_code: string | null;
}
export interface Diagnostics {
  calls: CallRecord[];
  capacity: number;
  scope: string;
}
export interface Capabilities {
  actions: ActionDescriptor[];
  document_actions: ActionDescriptor[];
  services: Record<string, [number, string, string]>;
  panels: Panel[];
  workbenches: EngineeringView[];
}
export interface EngineeringView {
  plugin: string;
  id: string;
  title: string;
  kind: string;
  analyze_action: string;
  generate_action: string;
  prepare_action?: string;
  template_action?: string;
  schema: Record<string, Data>;
  example: Data;
  tables: { field: string; title: string; row: Data }[];
  formats: string[];
}
export type RunStatus =
  "queued" | "running" | "succeeded" | "failed" | "cancelled" | "interrupted";
export interface RunSummary {
  id: string;
  action: string;
  title: string;
  status: RunStatus;
  started_at: number;
  finished_at: number | null;
  diagnostic_count: number;
}
export interface Run extends Omit<RunSummary, "diagnostic_count"> {
  input: Data;
  result: ActionResult | null;
  error: PluginError | null;
  plugins: PluginDescriptor[];
  environment?: ExecutionEnvironment;
  replay?: { source_run_id: string; check: ReplayCheck };
}
export interface RunPage {
  runs: RunSummary[];
  total: number;
  offset: number;
  limit: number;
}
export interface TemplateAsset {
  source: string;
  schema?: Schema;
  defaults?: Data;
  metadata?: Data;
  tags?: string[];
  [key: string]: Data;
}
export interface AssetRead {
  asset: TemplateAsset;
  fingerprint: string;
}
export interface AssetSaved {
  fingerprint: string;
}
export interface CatalogItem {
  id: string;
  title: string;
  description: string;
  tags: string[];
  fingerprint: string;
}
export interface Catalog {
  items: CatalogItem[];
}
export interface ReplayCheck {
  status: "matching" | "changed" | "unknown";
  differences: string[];
}
export type Reader<T> = (value: unknown) => T;

function invalid(): never {
  throw new Error("服务响应契约无效");
}
function record(value: unknown): Record<string, Data> {
  if (!object(value)) invalid();
  return value;
}
function text(value: unknown): string {
  if (typeof value !== "string") invalid();
  return value;
}
function integer(value: unknown, minimum = 0): number {
  if (isLosslessNumber(value) && !/^(0|[1-9]\d*)$/.test(value.value)) invalid();
  const n = isLosslessNumber(value) ? Number(value.value) : value;
  if (typeof n !== "number" || !Number.isSafeInteger(n) || n < minimum)
    invalid();
  return n;
}
function list<T>(value: unknown, read: Reader<T>): T[] {
  if (!Array.isArray(value)) invalid();
  return value.map(read);
}
function schema(value: unknown): Schema {
  if (typeof value === "boolean") return value;
  return record(value);
}
export const readDiagnostic: Reader<Diagnostic> = (value) => {
  const v = record(value);
  return {
    level: text(v.level),
    code: text(v.code),
    message: text(v.message),
    ...(v.path == null ? {} : { path: text(v.path) }),
    ...(v.template === undefined ? {} : { template: text(v.template) }),
    ...(v.line === undefined ? {} : { line: integer(v.line, 1) }),
    ...(v.column === undefined ? {} : { column: integer(v.column, 1) }),
  };
};
export const readPluginError: Reader<PluginError> = (value) => {
  const v = record(value);
  return {
    code: text(v.code),
    message: text(v.message),
    diagnostics:
      v.diagnostics === undefined ? [] : list(v.diagnostics, readDiagnostic),
  };
};
export const readActionResult: Reader<ActionResult> = (value) => {
  const v = record(value);
  return {
    data: v.data ?? null,
    artifacts:
      v.artifacts === undefined
        ? []
        : list(v.artifacts, (item) => {
            const a = record(item);
            return {
              name: text(a.name),
              media_type: text(a.media_type),
              text: text(a.text),
            };
          }),
    diagnostics:
      v.diagnostics === undefined ? [] : list(v.diagnostics, readDiagnostic),
  };
};
export const readAction: Reader<ActionDescriptor> = (value) => {
  const v = record(value);
  return {
    ...v,
    id: text(v.id),
    title: text(v.title),
    input_schema: schema(v.input_schema),
    output_schema: schema(v.output_schema),
  };
};
export const readCapabilities: Reader<Capabilities> = (value) => {
  const v = record(value);
  return {
    actions: list(v.actions, readAction),
    document_actions: list(v.document_actions, readAction),
    services: readServices(v.services),
    panels: list(v.panels, readPanel),
    workbenches: list(v.workbenches ?? [], (item) => {
      const w = record(item);
      return {
        plugin: text(w.plugin),
        id: text(w.id),
        title: text(w.title),
        kind: text(w.kind),
        analyze_action: text(w.analyze_action),
        generate_action: text(w.generate_action),
        ...(w.prepare_action == null
          ? {}
          : { prepare_action: text(w.prepare_action) }),
        ...(w.template_action == null
          ? {}
          : { template_action: text(w.template_action) }),
        schema: record(w.schema),
        example: w.example,
        tables: list(w.tables ?? [], (table) => {
          const t = record(table);
          return { field: text(t.field), title: text(t.title), row: t.row };
        }),
        formats: list(w.formats, text),
      };
    }),
  };
};
function readServices(
  value: unknown,
): Record<string, [number, string, string]> {
  return Object.fromEntries(
    Object.entries(record(value)).map(([key, binding]) => {
      if (!Array.isArray(binding) || binding.length !== 3) invalid();
      return [
        key,
        [integer(binding[0], 1), text(binding[1]), text(binding[2])],
      ];
    }),
  );
}
function readPanel(item: unknown): Panel {
  const p = record(item);
  return {
    id: text(p.id),
    title: text(p.title),
    actions: list(p.actions, text),
    view: p.view === undefined ? "form" : text(p.view),
  };
}
export const readPlugin: Reader<PluginDescriptor> = (value) => {
  const v = record(value);
  return {
    id: text(v.id),
    version: text(v.version),
    protocol_version: integer(v.protocol_version, 1),
    requires: list(v.requires ?? [], (item) => {
      const r = record(item);
      return { id: text(r.id), version: integer(r.version, 1) };
    }),
    provides: list(v.provides ?? [], (item) => {
      const r = record(item);
      return {
        id: text(r.id),
        version: integer(r.version, 1),
        action: text(r.action),
      };
    }),
    config_schema:
      v.config_schema === undefined
        ? { type: "object" }
        : schema(v.config_schema),
    panels: list(v.panels ?? [], readPanel),
    ...(v.asset_collections === undefined
      ? {}
      : { asset_collections: list(v.asset_collections, record) }),
  };
};
const readEnvironment: Reader<ExecutionEnvironment> = (value) => {
  const v = record(value);
  return {
    format_version: integer(v.format_version, 1),
    host_version: text(v.host_version),
    ...(v.provenance === undefined
      ? {}
      : { provenance: readProvenance(v.provenance) }),
    services: readServices(v.services),
    plugins: Object.fromEntries(
      Object.entries(record(v.plugins)).map(([id, entry]) => {
        const p = record(entry);
        return [
          id,
          {
            version: text(p.version),
            protocol_version: integer(p.protocol_version, 1),
          },
        ];
      }),
    ),
  };
};
function readProvenance(
  value: unknown,
): NonNullable<ExecutionEnvironment["provenance"]> {
  const v = record(value);
  return {
    scope: text(v.scope),
    host: v.host === null ? null : text(v.host),
    plugins: Object.fromEntries(
      Object.entries(record(v.plugins)).map(([id, entry]) => {
        const p = record(entry);
        return [
          id,
          {
            package: p.package === null ? null : text(p.package),
            public_config: text(p.public_config),
          },
        ];
      }),
    ),
  };
}
export const readDiagnostics: Reader<Diagnostics> = (value) => {
  const v = record(value);
  return {
    capacity: integer(v.capacity, 1),
    scope: text(v.scope),
    calls: list(v.calls, (item) => {
      const c = record(item);
      return {
        trace_id: text(c.trace_id),
        call_id: integer(c.call_id),
        action: text(c.action),
        owner: text(c.owner),
        parent_action: c.parent_action === null ? null : text(c.parent_action),
        depth: integer(c.depth),
        started_at: integer(c.started_at),
        elapsed_us: integer(c.elapsed_us),
        error_code: c.error_code === null ? null : text(c.error_code),
      };
    }),
  };
};
const statuses: RunStatus[] = [
  "queued",
  "running",
  "succeeded",
  "failed",
  "cancelled",
  "interrupted",
];
function runBase(value: unknown): Omit<RunSummary, "diagnostic_count"> {
  const v = record(value),
    status = text(v.status) as RunStatus;
  if (!statuses.includes(status)) invalid();
  return {
    id: text(v.id),
    action: text(v.action),
    title: text(v.title),
    status,
    started_at: integer(v.started_at),
    finished_at: v.finished_at === null ? null : integer(v.finished_at),
  };
}
export const readRun: Reader<Run> = (value) => {
  const v = record(value);
  if (!("input" in v)) invalid();
  return {
    ...runBase(v),
    input: v.input,
    result: v.result === null ? null : readActionResult(v.result),
    error: v.error === null ? null : readPluginError(v.error),
    plugins: list(v.plugins, readPlugin),
    ...(v.environment == null
      ? {}
      : { environment: readEnvironment(v.environment) }),
    ...(v.replay == null
      ? {}
      : {
          replay: {
            source_run_id: text(record(v.replay).source_run_id),
            check: readReplayCheck(record(v.replay).check),
          },
        }),
  };
};
export const readRunPage: Reader<RunPage> = (value) => {
  const v = record(value);
  return {
    runs: list(v.runs, (item) => ({
      ...runBase(item),
      diagnostic_count: integer(record(item).diagnostic_count),
    })),
    total: integer(v.total),
    offset: integer(v.offset),
    limit: integer(v.limit, 1),
  };
};
export const readAsset: Reader<AssetRead> = (value) => {
  const v = record(value),
    asset = record(v.asset);
  text(asset.source);
  if (asset.schema !== undefined) schema(asset.schema);
  if (asset.tags !== undefined) list(asset.tags, text);
  if (asset.metadata !== undefined) record(asset.metadata);
  return { asset: asset as TemplateAsset, fingerprint: text(v.fingerprint) };
};
export const readSaved: Reader<AssetSaved> = (value) => ({
  fingerprint: text(record(value).fingerprint),
});
export const readCatalog: Reader<Catalog> = (value) => ({
  items: list(record(value).items, (item) => {
    const v = record(item);
    return {
      id: text(v.id),
      title: text(v.title),
      description: text(v.description),
      tags: list(v.tags, text),
      fingerprint: text(v.fingerprint),
    };
  }),
});
export const readCreated: Reader<{ id: string }> = (value) => ({
  id: text(record(value).id),
});
export const readWorkspace: Reader<{ id: string; profile: string }> = (
  value,
) => {
  const v = record(value);
  return { id: text(v.id), profile: text(v.profile) };
};
export const readReplayCheck: Reader<ReplayCheck> = (value) => {
  const v = record(value);
  if (!["matching", "changed", "unknown"].includes(v.status)) invalid();
  return { status: v.status, differences: list(v.differences, text) };
};
export function readEnvelope(
  value: unknown,
): { ok: true; data: unknown } | { ok: false; error: PluginError } {
  const v = record(value);
  if (v.ok === true && Object.hasOwn(v, "data"))
    return { ok: true, data: v.data };
  if (v.ok === false) return { ok: false, error: readPluginError(v.error) };
  return invalid();
}
