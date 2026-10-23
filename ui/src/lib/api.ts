import { QueryClient, queryOptions } from "@tanstack/react-query";
import { decode, encode, type Data } from "./json";
import { readEnvelope, readActionResult, readRun, readCatalog, readAsset, readSaved,
  type Reader, type PluginError, type ActionResult, type Catalog, type AssetRead, type AssetSaved } from "./contracts";

export class ApiError extends Error {
  constructor(
    public detail: PluginError,
    public status: number,
  ) {
    super(detail.message || `请求失败 (${status})`);
  }
}
// Typed callers must supply a decoder; a generic assertion cannot validate a wire response.
export function api<T>(path: string, body: Data, signal: AbortSignal | undefined, read: Reader<T>): Promise<T>;
export function api(path: string, body?: Data, signal?: AbortSignal): Promise<Data>;
export async function api(
  path: string,
  body?: Data,
  signal?: AbortSignal,
  read?: Reader<unknown>,
): Promise<unknown> {
  const response = await fetch("/api/v2/" + path, {
    method: body === undefined ? "GET" : "POST",
    signal,
    headers:
      body === undefined ? undefined : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : encode(body),
  });
  const text = await response.text();
  let envelope: ReturnType<typeof readEnvelope>;
  try {
    envelope = readEnvelope(decode(text));
  } catch {
    throw new Error(`服务响应无效 (${response.status})`);
  }
  if (!envelope.ok) throw new ApiError(envelope.error, response.status);
  if (!response.ok) throw new Error(`服务响应状态无效 (${response.status})`);
  return read ? read(envelope.data) : envelope.data;
}
export function action(id: "template.catalog", input?: Data, signal?: AbortSignal): Promise<ActionResult<Catalog>>;
export function action(id: "template.read", input?: Data, signal?: AbortSignal): Promise<ActionResult<AssetRead>>;
export function action(id: "template.save", input?: Data, signal?: AbortSignal): Promise<ActionResult<AssetSaved>>;
export function action(id: string, input?: Data, signal?: AbortSignal): Promise<ActionResult>;
export async function action(
  id: string,
  input: Data = {},
  signal?: AbortSignal,
): Promise<ActionResult> {
  const result = await api(
    "actions/" + encodeURIComponent(id),
    { input },
    signal,
    readActionResult,
  );
  if (id === "template.catalog") result.data = readCatalog(result.data);
  if (id === "template.read") result.data = readAsset(result.data);
  if (id === "template.save") result.data = readSaved(result.data);
  return result;
}
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      structuralSharing: false,
      staleTime: 3000,
      retry: 1,
      refetchOnWindowFocus: false,
    },
    mutations: { retry: false },
  },
});
export const running = (status?: string) =>
  status === "queued" || status === "running";
export const runQuery = (workspace: string, id?: string) =>
  queryOptions({
    queryKey: [workspace, "run", id],
    enabled: !!id,
    queryFn: ({ signal }) =>
      api("runs/" + encodeURIComponent(id!), undefined, signal, readRun),
    refetchInterval: (query) =>
      running(query.state.data?.status) ? 500 : false,
    refetchIntervalInBackground: true,
  });
export const catalogQuery = (workspace: string) =>
  queryOptions({
    queryKey: [workspace, "templates"],
    queryFn: ({ signal }) => action("template.catalog", {}, signal),
  });
export const parameterQuery = (workspace: string, group: string) =>
  queryOptions({
    queryKey: [workspace, "parameters", group],
    queryFn: ({ signal }) => action("parameter.catalog", { group }, signal),
  });
