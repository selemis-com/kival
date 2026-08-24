/** Standard collection response envelope. */
export type ListResponse<T> = {
  /** Collection items. */
  items: T[];
  /**
   * Opaque cursor for the next page.
   *
   * Omitted on the final page. List responses do not include a total count.
   */
  next_cursor?: string;
};

/** Collection query parameters. */
export type ListParams = {
  /** Maximum items per page. Defaults to 50 and is capped at 200. */
  limit?: number | null;
  /** Opaque pagination cursor from a previous `response.next_cursor`. */
  cursor?: string | null;
};
