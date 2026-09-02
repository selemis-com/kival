import type { ApiKeyScope } from "./apiKeys.js";
import type { Timestamp, UUID } from "./common.js";
import type { ListResponse } from "./pagination.js";
import type { User } from "./users.js";

/** Current authenticated identity and effective capabilities. */
export type WhoamiResponse = {
  /** Authenticated user. */
  user: User;
  /** Whether the authenticated user is a global administrator. */
  is_global_admin: boolean;
  /** Whether the authenticated user may manage any groups. */
  can_manage_groups: boolean;
  /** Scopes delegated to the authenticating API key. Omitted for browser sessions. */
  scopes?: ApiKeyScope[];
};

/** Authenticated session response envelope. */
export type AuthenticatedSessionResponse = {
  /** Session expiration timestamp. */
  expires_at: Timestamp;
  /** Authenticated user. */
  user: User;
};

/** Browser session resource. */
export type Session = {
  /** Session ID. */
  id: UUID;
  /** Whether this is the browser session making the list request. */
  is_current: boolean;
  /** User ID. */
  user_id: UUID;
  /** Creation timestamp. */
  created_at: Timestamp;
  /** Last update timestamp. */
  updated_at: Timestamp;
  /** Expiration timestamp. */
  expires_at: Timestamp;
  /** Revocation timestamp. */
  revoked_at: Timestamp | null;
  /** User that revoked this session. */
  revoked_by: UUID | null;
  /** Revocation reason. */
  revocation_reason: string | null;
  /** Last-seen timestamp. */
  last_seen_at: Timestamp | null;
  /** User agent recorded at session creation. */
  user_agent: string | null;
  /** IP address recorded at session creation. */
  ip_address: string | null;
};

/** Session response envelope. */
export type SessionOnlyResponse = { session: Session };

/** Session list response envelope. */
export type SessionListResponse = ListResponse<Session>;
