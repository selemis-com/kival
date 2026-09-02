import type { ListParams } from "kival-sdk";

export function withParams(path: string, params: URLSearchParams) {
  const encoded = params.toString();
  return encoded ? `${path}?${encoded}` : path;
}

export function listParams(options: ListParams) {
  const params = new URLSearchParams();

  if (options.cursor != null) {
    params.set("cursor", options.cursor);
  }

  if (options.limit != null) {
    params.set("limit", options.limit.toString());
  }

  return params;
}

export function pathId(id: string) {
  return encodeURIComponent(id);
}

export function jsonRequest(
  method: "POST" | "PATCH",
  input: unknown,
  signal?: AbortSignal,
): RequestInit {
  return {
    method,
    headers: { "content-type": "application/json" },
    body: JSON.stringify(input),
    signal,
  };
}
