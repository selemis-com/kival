import type { Timestamp, UUID } from "./common.js";
import type { ListResponse } from "./pagination.js";

/**
 * Capability delegated to an API key.
 *
 * - `workspaces:read`: discover and inspect permitted workspaces.
 * - `workspaces:write`: modify permitted existing workspaces; also satisfies `workspaces:read`.
 * - `objects:read`: read, list, and search objects.
 * - `objects:write`: mutate objects; also satisfies `objects:read`.
 * - `attachments:read`: read attachment metadata and content.
 * - `attachments:write`: upload and reuse attachments; also satisfies `attachments:read`.
 * - `graph:read`: read graphs, backlinks, and edges.
 * - `graph:write`: create and revoke edges; also satisfies `graph:read`.
 * - `events:read`: read workspace and object activity.
 * - `realtime:read`: receive ephemeral realtime invalidations for resources the key may read.
 * - `access:manage`: manage workspace memberships, workspace-group links, and object grants.
 * - `admin`: access API-key-enabled global operations, bounded by the owner's current authority.
 */
export type ApiKeyScope =
  | "workspaces:read"
  | "workspaces:write"
  | "objects:read"
  | "objects:write"
  | "attachments:read"
  | "attachments:write"
  | "graph:read"
  | "graph:write"
  | "events:read"
  | "realtime:read"
  | "access:manage"
  | "admin";

/** API key metadata. The secret token is never included after creation. */
export type ApiKey = {
  /** API key ID. */
  id: UUID;
  /** User whose authority the key delegates. */
  user_id: UUID;
  /** Stable user-defined label identifying the key and recorded in audit events. */
  label: string;
  /** Mutable authorization revision. */
  authorization_revision: number;
  /** Capabilities delegated to the key. */
  scopes: ApiKeyScope[];
  /** Workspaces in which the key may exercise workspace-scoped capabilities. */
  workspace_ids: UUID[];
  /** Creation timestamp. */
  created_at: Timestamp;
  /** Last update timestamp. */
  updated_at: Timestamp;
  /** Optional expiration timestamp. */
  expires_at: Timestamp | null;
  /** Revocation timestamp. */
  revoked_at: Timestamp | null;
  /** Last authenticated use timestamp. Updates are intentionally coalesced. */
  last_used_at: Timestamp | null;
};

/** Request body for creating an API key. */
export type CreateApiKeyRequest = {
  /** Stable user-defined label identifying the key and recorded in audit events. */
  label: string;
  /** Capabilities delegated to the key; these can only reduce the owner's authority. */
  scopes: ApiKeyScope[];
  /** Permitted workspaces. An empty list grants no workspace-scoped access. */
  workspace_ids: UUID[];
  /** Optional expiration timestamp. */
  expires_at?: Timestamp | null;
};

/** Request body for replacing an active API key's delegated authorization. */
export type UpdateApiKeyRequest = {
  /** Expected mutable authorization revision. */
  authorization_revision: number;
  /** Replacement delegated capabilities. */
  scopes: ApiKeyScope[];
  /** Replacement permitted workspaces. */
  workspace_ids: UUID[];
};

/** API key response envelope. */
export type ApiKeyResponse = { api_key: ApiKey };

/** API key creation response. The plaintext token is returned exactly once. */
export type CreateApiKeyResponse = ApiKeyResponse & { token: string };

/** API key list response envelope. */
export type ApiKeyListResponse = ListResponse<ApiKey>;
