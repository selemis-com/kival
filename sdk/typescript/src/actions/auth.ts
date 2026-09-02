import { requestInit } from "../internal/utils.js";
import type { WhoamiResponse } from "../types.js";
import type { KivalClientBase, WithSignal } from "./types.js";

/** Parameters for {@link whoami}. */
export type WhoamiParameters = WithSignal;

/** Return type for {@link whoami}. */
export type WhoamiReturnType = WhoamiResponse;

/** Returns the authenticated identity and effective API-key scopes. */
export function whoami(
  client: KivalClientBase,
  parameters: WhoamiParameters = {},
): Promise<WhoamiReturnType> {
  return client.requestJson<WhoamiResponse>("/auth/whoami", requestInit({}, parameters.signal));
}
