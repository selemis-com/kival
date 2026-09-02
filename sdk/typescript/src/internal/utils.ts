import type { KivalRequestInit } from "../transports/index.js";
import type { ListParams } from "../types.js";

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

export function normalizePrefix(prefix: string) {
  if (prefix.trim() !== prefix) {
    throw new TypeError("apiPrefix must not contain leading or trailing whitespace");
  }

  if (/^[A-Za-z][A-Za-z\d+.-]*:/.test(prefix)) {
    throw new TypeError("apiPrefix must be a path, not an absolute URL");
  }

  if (prefix.includes("?") || prefix.includes("#")) {
    throw new TypeError("apiPrefix must not contain a query string or fragment");
  }

  if (prefix.includes("\\")) {
    throw new TypeError("apiPrefix must use forward slashes");
  }

  const withLeadingSlash = prefix.startsWith("/") ? prefix : `/${prefix}`;
  return trimTrailingSlash(withLeadingSlash);
}

function trimTrailingSlash(value: string) {
  return value.replace(/\/+$/, "");
}

export function pathId(id: string) {
  return encodeURIComponent(id);
}

export function jsonRequest(
  method: "POST" | "PATCH",
  input: unknown,
  signal?: AbortSignal,
): KivalRequestInit {
  return requestInit(
    {
      method,
      headers: { "content-type": "application/json" },
      body: JSON.stringify(input),
    },
    signal,
  );
}

export function requestInit(init: KivalRequestInit, signal?: AbortSignal): KivalRequestInit {
  return signal ? { ...init, signal } : init;
}
