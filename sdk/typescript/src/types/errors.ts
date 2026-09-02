/** Inner JSON API error object. */
export type ApiErrorBody = {
  /** Stable machine-readable error code. */
  code: string;
  /** Human-readable error message. */
  message: string;
};

/** Top-level JSON API error response body. */
export type ApiErrorResponse = {
  error: ApiErrorBody;
};
