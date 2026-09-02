import type { KivalRequestInit } from "../transports/index.js";

/** Minimal transport contract required by JSON actions. */
export type KivalClientBase = {
  requestJson<T>(path: string, init?: KivalRequestInit): Promise<T>;
};

/** Transport contract required by actions that return bytes or a raw response. */
export type KivalResponseClient = KivalClientBase & {
  requestBytes(path: string, init?: KivalRequestInit): Promise<Uint8Array>;
  requestResponse(path: string, init?: KivalRequestInit): Promise<Response>;
};

/** Adds optional request cancellation to an action parameter object. */
export type WithSignal<Parameters extends object = object> = Parameters & {
  signal?: AbortSignal;
};
