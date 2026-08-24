/** Server health or readiness status. */
export type Status = "ok" | "error";

/** JSON response indicating server health or readiness. */
export type StatusResponse = {
  status: Status;
  /** Additional status information, when present. */
  message?: string;
};
