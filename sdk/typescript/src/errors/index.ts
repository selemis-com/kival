/** Broad semantic category for an error returned by the Kival API. */
export type KivalApiErrorKind =
  | "unauthorized"
  | "forbidden"
  | "notFound"
  | "conflict"
  | "rateLimited"
  | "invalidRequest"
  | "serverError"
  | "other";

/** Broad category for a request transport failure. */
export type KivalTransportErrorKind = "connect" | "timeout" | "abort" | "other";

/** Structured error returned for an unsuccessful Kival API response. */
export class KivalApiError extends Error {
  /** HTTP status returned by Kival. */
  readonly status: number;
  /** Broad semantic category derived from the HTTP status. */
  readonly kind: KivalApiErrorKind;
  /** HTTP status text returned by the transport. */
  readonly statusText: string;
  /** Response headers, including request and rate-limit metadata when provided. */
  readonly headers: Headers;
  /** Raw `Retry-After` header value, when present. */
  readonly retryAfter: string | null;
  /** Delta-seconds `Retry-After` value converted to milliseconds, when representable. */
  readonly retryAfterMilliseconds: number | null;
  /** Stable Kival API error code, when present. */
  readonly code: string | null;
  /** Parsed JSON or original plain-text response body, bounded to 64 KiB. */
  readonly body: unknown;

  constructor(
    status: number,
    message: string,
    code: string | null = null,
    body: unknown = null,
    statusText = "",
    headers: HeadersInit = {},
  ) {
    super(message);
    this.name = "KivalApiError";
    this.status = status;
    this.kind = apiErrorKind(status);
    this.statusText = statusText;
    this.headers = new Headers(headers);
    this.retryAfter = this.headers.get("retry-after");
    this.retryAfterMilliseconds = retryAfterMilliseconds(this.retryAfter);
    this.code = code;
    this.body = body;
  }
}

/** Error raised when a successful response violates its declared decoding contract. */
export class KivalResponseError extends Error {
  readonly kind = "decode";
  /** Response that could not be decoded as declared. Its body may already have been consumed. */
  readonly response: Response;
  override readonly cause: unknown;

  constructor(message: string, response: Response, cause?: unknown) {
    super(message, { cause });
    this.name = "KivalResponseError";
    this.response = response;
    this.cause = cause;
  }
}

/** Error raised when request execution or response-body transport fails. */
export class KivalTransportError extends Error {
  /** Broad transport failure category. */
  readonly kind: KivalTransportErrorKind;
  override readonly cause: unknown;

  constructor(kind: KivalTransportErrorKind, message: string, cause?: unknown) {
    super(message, { cause });
    this.name = "KivalTransportError";
    this.kind = kind;
    this.cause = cause;
  }
}

function apiErrorKind(status: number): KivalApiErrorKind {
  switch (status) {
    case 401:
      return "unauthorized";
    case 403:
      return "forbidden";
    case 404:
      return "notFound";
    case 409:
      return "conflict";
    case 429:
      return "rateLimited";
    default:
      if (status >= 400 && status < 500) return "invalidRequest";
      if (status >= 500 && status < 600) return "serverError";
      return "other";
  }
}

function retryAfterMilliseconds(retryAfter: string | null): number | null {
  if (retryAfter === null || !/^\d+$/.test(retryAfter)) return null;

  const seconds = Number(retryAfter);
  const milliseconds = seconds * 1_000;
  return Number.isSafeInteger(milliseconds) ? milliseconds : null;
}
