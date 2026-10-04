import { encode, parseData, type Data } from "./data";
export class ApiError extends Error {
  constructor(public detail: Data) {
    super(detail.message || "请求失败");
  }
}
export async function api(path: string, body?: Data): Promise<Data> {
  const response = await fetch("/api/v2/" + path, {
    method: body === undefined ? "GET" : "POST",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : encode(body),
  });
  const text = await response.text();
  let data: Data;
  try {
    data = parseData(text);
  } catch {
    throw new Error("服务返回了无法解析的响应，请检查连接");
  }
  if (!data.ok) throw new ApiError(data.error);
  return data.data;
}
export async function action(id: string, input: Data): Promise<Data> {
  return api("actions/" + encodeURIComponent(id), { input });
}
