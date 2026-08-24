import type { ApiKeyScope } from "../../shared/types";

export const apiKeyScopeOptions = [
  ["workspaces:read", "Read workspaces", "Discover and inspect selected workspaces."],
  ["workspaces:write", "Write workspaces", "Modify selected workspace metadata and lifecycle."],
  ["objects:read", "Read objects", "List, search, and read objects."],
  ["objects:write", "Write objects", "Create, update, archive, and restore objects."],
  ["attachments:read", "Read attachments", "Read attachment metadata and content."],
  ["attachments:write", "Write attachments", "Upload and reuse attachments."],
  ["graph:read", "Read graph", "Read graphs, backlinks, and edges."],
  ["graph:write", "Write graph", "Create and revoke graph edges."],
  ["events:read", "Read activity", "Read workspace and object activity."],
  [
    "realtime:read",
    "Read realtime",
    "Receive ephemeral change invalidations for resources this key can read.",
  ],
  ["access:manage", "Manage access", "Manage memberships, group links, and grants."],
  ["admin", "Administrative API", "Use API-key-enabled global administration operations."],
] as const satisfies ReadonlyArray<readonly [ApiKeyScope, string, string]>;

export type ExpirationOption = "7" | "30" | "60" | "90" | "365" | "none" | "custom";

export const expirationOptions: ReadonlyArray<readonly [ExpirationOption, string]> = [
  ["7", "7 days"],
  ["30", "30 days"],
  ["60", "60 days"],
  ["90", "90 days"],
  ["365", "365 days"],
  ["none", "No expiration"],
  ["custom", "Custom (days)"],
];
