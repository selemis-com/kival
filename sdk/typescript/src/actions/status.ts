import { requestInit } from "../internal/utils.js";
import type { StatusResponse } from "../types.js";
import type { KivalClientBase, WithSignal } from "./types.js";

/** Parameters for {@link health}. */
export type HealthParameters = WithSignal;

/** Return type for {@link health}. */
export type HealthReturnType = StatusResponse;

/** Checks server health. */
export function health(
  client: KivalClientBase,
  parameters: HealthParameters = {},
): Promise<HealthReturnType> {
  return client.requestJson<StatusResponse>(
    "/healthz",
    requestInit({ auth: "none" }, parameters.signal),
  );
}

/** Parameters for {@link ready}. */
export type ReadyParameters = WithSignal;

/** Return type for {@link ready}. */
export type ReadyReturnType = StatusResponse;

/** Checks server readiness. */
export function ready(
  client: KivalClientBase,
  parameters: ReadyParameters = {},
): Promise<ReadyReturnType> {
  return client.requestJson<StatusResponse>(
    "/readyz",
    requestInit({ auth: "none" }, parameters.signal),
  );
}
