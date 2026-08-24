import { requestInit } from "../internal/utils.js";
import type { User, UserResponse } from "../types.js";
import type { KivalClientBase, WithSignal } from "./types.js";

/** Parameters for {@link whoami}. */
export type WhoamiParameters = WithSignal;

/** Return type for {@link whoami}. */
export type WhoamiReturnType = User;

/** Returns the user that owns the configured API key. */
export function whoami(
  client: KivalClientBase,
  parameters: WhoamiParameters = {},
): Promise<WhoamiReturnType> {
  return client
    .requestJson<UserResponse>("/auth/whoami", requestInit({}, parameters.signal))
    .then((response) => response.user);
}
