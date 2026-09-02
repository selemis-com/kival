import { DEFAULT_TIMEOUT } from "../constants.js";
import {
  KivalApiError,
  KivalResponseError,
  KivalTransportError,
  type KivalTransportErrorKind,
} from "../errors/index.js";
import { getResponseError } from "../internal/responseError.js";
import { normalizePrefix } from "../internal/utils.js";

/** Default API prefix used by the Kival server. */
export const API_PREFIX = "/api/v1";

/** Fetch-compatible request function. */
export type FetchLike = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

/** Authentication policy for a Kival request. */
export type KivalRequestAuth = "apiKey" | "none";

/** Request options understood by Kival transports. */
export type KivalRequestInit = RequestInit & {
  /** Authentication policy. Defaults to API-key authentication. */
  auth?: KivalRequestAuth;
};

/** Options for the default Fetch API transport. */
export type HttpTransportOptions = {
  /** HTTP or HTTPS server origin root, for example `https://kival.example`. */
  baseUrl: string;
  /** API path prefix. Defaults to `/api/v1`. */
  apiPrefix?: string;
  /** Custom Fetch API implementation. Defaults to `globalThis.fetch`. */
  fetch?: FetchLike;
  /** Request timeout in milliseconds. Defaults to 30 seconds. */
  timeout?: number;
  /** Bearer API key. Empty keys are rejected when the transport is created. */
  apiKey: string;
};

/** Transport contract consumed by the Kival action layer. */
export type KivalTransport = {
  /** Normalized server origin root. */
  readonly baseUrl: string;
  /** Normalized API path prefix. */
  readonly apiPrefix: string;
  /** Executes a request that must return a JSON body. */
  requestJson<T>(path: string, init?: KivalRequestInit): Promise<T>;
  /** Executes a request and buffers its complete response body as bytes. */
  requestBytes(path: string, init?: KivalRequestInit): Promise<Uint8Array>;
  /** Executes a request whose successful response body is intentionally ignored. */
  requestVoid(path: string, init?: KivalRequestInit): Promise<void>;
  /** Executes a request and returns its response after the headers arrive. */
  requestResponse(path: string, init?: KivalRequestInit): Promise<Response>;
  /** Constructs an absolute API URL without making a request. */
  url(path: string): string;
};

/**
 * Creates the default Fetch API transport.
 *
 * Authenticated requests use bearer API-key authentication; public requests explicitly omit it.
 * Browser credentials are omitted and unsuccessful responses are converted to `KivalApiError`.
 */
export function http(options: HttpTransportOptions): KivalTransport {
  const baseUrl = normalizeBaseUrl(options.baseUrl);
  const apiPrefix = normalizePrefix(options.apiPrefix ?? API_PREFIX);
  const fetchImpl = options.fetch ?? globalThis.fetch.bind(globalThis);
  const apiKey = options.apiKey;
  const timeout = options.timeout ?? DEFAULT_TIMEOUT;

  if (typeof apiKey !== "string" || apiKey.trim() === "") {
    throw new TypeError("apiKey must not be empty");
  }

  if (!Number.isFinite(timeout) || timeout <= 0) {
    throw new TypeError("timeout must be a positive finite number");
  }

  function url(path: string) {
    const normalizedPath = path.startsWith("/") ? path : `/${path}`;
    return `${baseUrl}${apiPrefix}${normalizedPath}`;
  }

  function executeResponse(path: string, init: KivalRequestInit) {
    const { auth = "apiKey", ...requestInit } = init;
    const headers = new Headers(requestInit.headers);

    if (auth === "apiKey") {
      headers.set("authorization", `Bearer ${apiKey}`);
    } else {
      headers.delete("authorization");
    }

    return fetchResponse(fetchImpl, url(path), {
      ...requestInit,
      credentials: "omit",
      headers,
    });
  }

  async function requestJson<T>(path: string, init: KivalRequestInit = {}) {
    const request = withTimeout(init, timeout);

    try {
      return await decodeJsonResponse<T>(await executeResponse(path, request.init));
    } catch (cause) {
      if (request.init.signal?.aborted) {
        const transportCause =
          cause instanceof KivalResponseError || cause instanceof KivalTransportError
            ? cause.cause
            : cause;
        throw transportError(transportCause, request.init.signal);
      }
      throw cause;
    } finally {
      request.cleanup();
    }
  }

  async function requestVoid(path: string, init: KivalRequestInit = {}) {
    const request = withTimeout(init, timeout);

    try {
      const response = await executeResponse(path, request.init);
      await response.body?.cancel();
    } catch (cause) {
      if (cause instanceof KivalApiError) throw cause;
      if (cause instanceof KivalTransportError && !request.init.signal?.aborted) throw cause;

      const transportCause = cause instanceof KivalTransportError ? cause.cause : cause;
      throw transportError(transportCause, request.init.signal);
    } finally {
      request.cleanup();
    }
  }

  async function requestBytes(path: string, init: KivalRequestInit = {}) {
    const request = withTimeout(init, timeout);

    try {
      return await decodeBytesResponse(await executeResponse(path, request.init));
    } catch (cause) {
      if (request.init.signal?.aborted) {
        const transportCause =
          cause instanceof KivalResponseError || cause instanceof KivalTransportError
            ? cause.cause
            : cause;
        throw transportError(transportCause, request.init.signal);
      }
      throw cause;
    } finally {
      request.cleanup();
    }
  }

  async function requestResponse(path: string, init: KivalRequestInit = {}) {
    const request = withTimeout(init, timeout);

    try {
      return await executeResponse(path, request.init);
    } finally {
      request.cleanup();
    }
  }

  return { baseUrl, apiPrefix, requestJson, requestBytes, requestVoid, requestResponse, url };
}

function withTimeout(init: KivalRequestInit, timeout: number) {
  if (init.signal?.aborted) {
    return { init, cleanup() {} };
  }

  const timeoutController = new AbortController();
  const callerSignal = init.signal;
  let timeoutId: number | undefined;

  const cleanup = () => {
    if (timeoutId !== undefined) {
      globalThis.clearTimeout(timeoutId);
      timeoutId = undefined;
    }
  };

  timeoutId = globalThis.setTimeout(() => {
    timeoutId = undefined;
    timeoutController.abort(
      new DOMException(`Request timed out after ${timeout} ms`, "TimeoutError"),
    );
  }, timeout);

  const signal = callerSignal
    ? AbortSignal.any([callerSignal, timeoutController.signal])
    : timeoutController.signal;

  return { init: { ...init, signal }, cleanup };
}

/**
 * Executes a Fetch API request and applies Kival's API and transport error boundaries.
 *
 * Custom transports can use this helper while retaining control over URL construction, headers,
 * credentials, and other request options.
 */
export async function fetchResponse(
  fetchImpl: FetchLike,
  input: RequestInfo | URL,
  init: RequestInit = {},
) {
  let response: Response;

  try {
    response = await fetchImpl(input, init);
  } catch (cause) {
    throw transportError(cause, init.signal, "connect");
  }

  if (response.ok) return response;

  try {
    throw await getResponseError(response);
  } catch (cause) {
    if (cause instanceof KivalApiError) throw cause;
    throw transportError(cause, init.signal);
  }
}

/** Decodes a successful response that is contractually required to contain JSON. */
export async function decodeJsonResponse<T>(response: Response): Promise<T> {
  let text: string;

  try {
    text = await response.text();
  } catch (cause) {
    throw transportError(cause, undefined);
  }

  if (!text) {
    throw new KivalResponseError(
      "Kival returned an empty body where a JSON response was required.",
      response,
    );
  }

  try {
    return JSON.parse(text) as T;
  } catch (cause) {
    throw new KivalResponseError("Kival returned malformed JSON.", response, cause);
  }
}

/** Buffers a successful response body as bytes. */
export async function decodeBytesResponse(response: Response): Promise<Uint8Array> {
  try {
    return new Uint8Array(await response.arrayBuffer());
  } catch (cause) {
    throw transportError(cause, undefined);
  }
}

function transportError(
  cause: unknown,
  signal: AbortSignal | null | undefined,
  typeErrorKind: Extract<KivalTransportErrorKind, "connect" | "other"> = "other",
) {
  if (cause instanceof KivalTransportError) return cause;

  const kind = transportErrorKind(cause, signal, typeErrorKind);
  return new KivalTransportError(kind, transportErrorMessage(kind), cause);
}

function transportErrorKind(
  cause: unknown,
  signal: AbortSignal | null | undefined,
  typeErrorKind: Extract<KivalTransportErrorKind, "connect" | "other">,
): KivalTransportErrorKind {
  if (hasErrorName(cause, "TimeoutError") || hasErrorName(signal?.reason, "TimeoutError")) {
    return "timeout";
  }

  if (hasErrorName(cause, "AbortError") || signal?.aborted) {
    return "abort";
  }

  return cause instanceof TypeError ? typeErrorKind : "other";
}

function transportErrorMessage(kind: KivalTransportErrorKind) {
  switch (kind) {
    case "connect":
      return "Failed to connect to Kival.";
    case "timeout":
      return "Kival request timed out.";
    case "abort":
      return "Kival request was aborted.";
    default:
      return "Kival request transport failed.";
  }
}

function hasErrorName(value: unknown, name: string) {
  return typeof value === "object" && value !== null && "name" in value && value.name === name;
}

function normalizeBaseUrl(value: string) {
  let url: URL;

  try {
    url = new URL(value);
  } catch {
    throw new TypeError("baseUrl must be a valid HTTP or HTTPS origin root");
  }

  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new TypeError("baseUrl must use HTTP or HTTPS");
  }

  if (url.username || url.password) {
    throw new TypeError("baseUrl must not contain credentials");
  }

  if (url.pathname !== "/") {
    throw new TypeError("baseUrl must not contain a path prefix");
  }

  if (url.search) {
    throw new TypeError("baseUrl must not contain a query string");
  }

  if (url.hash) {
    throw new TypeError("baseUrl must not contain a fragment");
  }

  return url.origin;
}
