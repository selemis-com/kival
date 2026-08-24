import { listParams } from "../internal/utils.js";
import type { ListParams } from "../types.js";

export function searchableListParams(options: ListParams & { q?: string | null }) {
  const params = listParams(options);
  setString(params, "q", options.q);
  return params;
}

export function setString(params: URLSearchParams, key: string, value: string | null | undefined) {
  if (value != null) params.set(key, value);
}

export function setNumber(params: URLSearchParams, key: string, value: number | null | undefined) {
  if (value != null) params.set(key, value.toString());
}

export function setBoolean(
  params: URLSearchParams,
  key: string,
  value: boolean | null | undefined,
) {
  if (value != null) params.set(key, value.toString());
}
