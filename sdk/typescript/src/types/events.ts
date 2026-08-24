import type { JsonValue, Timestamp, UUID } from "./common.js";

/** Event sequence ordering. */
export type EventOrder = "asc" | "desc";

/** Event resource. */
export type Event = {
  /** Event ID. */
  id: UUID;
  /** Global event sequence. */
  sequence_number: number;
  /** Workspace ID. */
  workspace_id: UUID | null;
  /** Actor user ID. */
  actor_user_id: UUID | null;
  /** Actor username when the event was performed by a user. */
  actor_username: string | null;
  /** API key used by the actor when delegated authentication was used. */
  api_key_id: UUID | null;
  /** User-defined API-key label captured when the event was generated. */
  api_key_label: string | null;
  /** Event kind. */
  event_kind: string;
  /** Object ID. */
  object_id: UUID | null;
  /** Object-version ID. */
  object_version_id: UUID | null;
  /** Object-edge ID. */
  object_edge_id: UUID | null;
  /** Object-grant ID. */
  object_grant_id: UUID | null;
  /** Commentary thread ID. */
  comment_thread_id: UUID | null;
  /** Commentary comment ID. */
  comment_id: UUID | null;
  /** Group ID. */
  group_id: UUID | null;
  /** Target user ID. */
  target_user_id: UUID | null;
  /** Event payload. */
  payload: JsonValue;
  /** Creation timestamp. */
  created_at: Timestamp;
};

/** Event-list query parameters. */
export type EventListParams = {
  /** Maximum events to return. */
  limit?: number | null;
  /** Return events after this sequence number. */
  after_sequence?: number | null;
  /** Return events before this sequence number. */
  before_sequence?: number | null;
  /** Event sequence ordering. Defaults to oldest first. */
  order?: EventOrder;
  /** Filter by event kind. */
  event_kind?: string | null;
  /** Filter by actor user ID. */
  actor_user_id?: UUID | null;
  /** Filter by target user ID. */
  target_user_id?: UUID | null;
  /** Filter by object ID. */
  object_id?: UUID | null;
  /** Filter by group ID. */
  group_id?: UUID | null;
};
