import { QueryClient, queryOptions } from "@tanstack/react-query";
import { decode, encode, type Data } from "./json";

export class ApiError extends Error {
  constructor(
    public detail: Data,
    public status: number,
  ) {
    super(detail.message || `请求失败 (${status})`);
  }
}
export async function api<T = Data>(
  path: string,
  body?: Data,
  signal?: AbortSignal,
): Promise<T> {
  const response = await fetch("/api/v2/" + path, {
    method: body === undefined ? "GET" : "POST",
    signal,
    headers:
      body === undefined ? undefined : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : encode(body),
  });
  const text = await response.text();
  let envelope: Data;
  try {
    envelope = decode(text);
  } catch {
    throw new Error(`服务响应无效 (${response.status})`);
  }
  if (!response.ok || envelope.ok !== true)
    throw new ApiError(envelope.error || {}, response.status);
  return envelope.data as T;
}
export async function action(
  id: string,
  input: Data = {},
  signal?: AbortSignal,
): Promise<Data> {
  const result = await api(
    "actions/" + encodeURIComponent(id),
    { input },
    signal,
  );
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
      api("runs/" + encodeURIComponent(id!), undefined, signal),
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
