import { pathId, requestInit, withParams } from "../internal/utils.js";
import type { SearchParams, SearchResponse, UUID } from "../types.js";
import { setBoolean, setNumber, setString } from "./params.js";
import type { KivalClientBase, WithSignal } from "./types.js";

/** Parameters for {@link searchWorkspace}. */
export type SearchWorkspaceParameters = WithSignal<
  SearchParams & {
    workspaceId: UUID;
  }
>;

/** Return type for {@link searchWorkspace}. */
export type SearchWorkspaceReturnType = SearchResponse;

/** Searches visible workspace content. */
export function searchWorkspace(
  client: KivalClientBase,
  parameters: SearchWorkspaceParameters,
): Promise<SearchWorkspaceReturnType> {
  const { workspaceId, ...options } = parameters;
  const params = new URLSearchParams({ q: options.q });
  setString(params, "categories", options.categories);
  setString(params, "status", options.status);
  setNumber(params, "limit", options.limit);
  setString(params, "cursor", options.cursor);
  setString(params, "mode", options.mode);
  setBoolean(params, "case_sensitive", options.case_sensitive);
  setNumber(params, "context", options.context);
  setBoolean(params, "include_history", options.include_history);
  return client.requestJson<SearchWorkspaceReturnType>(
    withParams(`/workspaces/${pathId(workspaceId)}/search`, params),
    requestInit({}, options.signal),
  );
}
