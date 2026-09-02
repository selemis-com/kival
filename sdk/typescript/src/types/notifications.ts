import type { Timestamp, UUID } from "./common.js";

/** Effective notification preference for one object. */
export type ObjectNotificationPreference = {
  /** Workspace containing the object. */
  workspace_id: UUID;
  /** Object whose ordinary notifications are configured. */
  object_id: UUID;
  /** Whether ordinary object activity may generate notifications. */
  ordinary_notifications_enabled: boolean;
  /** Whether this value comes from an explicit stored preference. */
  explicit: boolean;
  /** Last explicit preference update. */
  updated_at: Timestamp | null;
};

/** Request body for changing an object notification preference. */
export type UpdateObjectNotificationPreferenceRequest = {
  /** Whether ordinary object activity should generate notifications. */
  ordinary_notifications_enabled: boolean;
};

/** One durable personal inbox entry. */
export type InboxEntry = {
  /** Inbox entry ID. */
  id: UUID;
  /** Monotonic sequence used for stable pagination. */
  sequence_number: number;
  /** Recipient user ID. */
  recipient_user_id: UUID;
  /** Source workspace ID. */
  workspace_id: UUID;
  /** Current workspace name resolved under current authorization. */
  workspace_name: string;
  /** Source object ID for object-scoped notifications. */
  object_id: UUID | null;
  /** Current object title for object-scoped notifications. */
  object_title: string | null;
  /** Earliest durable source event represented by this entry. */
  source_event_id: UUID;
  /** Latest durable source event represented by this entry. */
  latest_event_id: UUID;
  /** Latest actor user ID. */
  actor_user_id: UUID | null;
  /** Latest actor username. */
  actor_username: string | null;
  /** Latest actor display name. */
  actor_display_name: string | null;
  /** Stable notification presentation type. */
  notification_type: string;
  /** Reason the entry was generated. */
  reason: string;
  /** Number of source events represented by this entry. */
  event_count: number;
  /** Source commentary thread ID. */
  thread_id: UUID | null;
  /** Source comment ID. */
  comment_id: UUID | null;
  /** Truncated current comment text when the source comment is still available. */
  comment_excerpt: string | null;
  /** Read timestamp. */
  read_at: Timestamp | null;
  /** Creation timestamp. */
  created_at: Timestamp;
  /** Last projection update timestamp. */
  updated_at: Timestamp;
};

/** Inbox-list query parameters. */
export type InboxListParams = {
  /** Maximum entries to return. */
  limit?: number | null;
  /** Opaque pagination cursor from a previous response. */
  cursor?: string | null;
  /** Return unread entries only. */
  unread_only?: boolean;
  /** Restrict entries to one workspace. */
  workspace_id?: UUID | null;
};

/** Request body for changing one inbox entry's read state. */
export type UpdateInboxEntryRequest = {
  /** Whether the entry should be marked read. */
  read: boolean;
};

/** Request body for marking a bounded inbox range read. */
export type MarkInboxReadRequest = {
  /** Optional workspace scope. */
  workspace_id?: UUID | null;
  /** Optional inclusive sequence boundary. */
  through_sequence?: number | null;
};

/** Response returned after a bulk inbox update. */
export type InboxUpdatedResponse = {
  /** Number of inbox entries updated. */
  updated: number;
};

/** Current unread inbox count. */
export type InboxUnreadCountResponse = {
  /** Number of currently visible unread inbox entries. */
  unread_count: number;
};

/** Lightweight realtime invalidation message. */
export type RealtimeMessage = {
  /** Stable invalidation type. */
  type: string;
  /** Related workspace ID. */
  workspace_id: UUID | null;
  /** Related object ID. */
  object_id: UUID | null;
  /** Related durable event ID. */
  event_id: UUID | null;
  /** Related inbox entry ID. */
  inbox_entry_id: UUID | null;
};
