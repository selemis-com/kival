import type {
  ArchiveListStatus,
  ArchiveStatus,
  PatchField,
  Timestamp,
  UserReference,
  UUID,
} from "./common.js";
import type { ListParams } from "./pagination.js";

/** Membership role for groups and workspaces. */
export type MembershipRole = "member" | "admin";

/** Group resource. */
export type Group = {
  /** Group ID. */
  id: UUID;
  /** Group name. */
  name: string;
  /** Optional group description. */
  description: string | null;
  /** Lifecycle status. */
  status: ArchiveStatus;
  /** User that created this group. */
  created_by: UUID | null;
  /** User that archived this group. */
  archived_by: UUID | null;
  /** Creation timestamp. */
  created_at: Timestamp;
  /** Last update timestamp. */
  updated_at: Timestamp;
  /** Archive timestamp. */
  archived_at: Timestamp | null;
};

/** Group collection query parameters. */
export type GroupListParams = ListParams & {
  /** Archive status filter. Defaults to active groups. */
  status?: ArchiveListStatus;
  /** Case-insensitive group-name search. */
  q?: string | null;
};

/** Request body for creating a group. */
export type CreateGroupRequest = { name: string; description?: string | null };

/** Request body for updating a group. */
export type UpdateGroupRequest = {
  /** New group name. Omit to leave unchanged. */
  name?: string | null;
  /** New description. Omit to leave unchanged or use `null` to clear it. */
  description?: PatchField<string>;
};

/** Group response envelope. */
export type GroupResponse = { group: Group };

/** Group membership resource. */
export type GroupMembership = {
  /** Membership ID. */
  id: UUID;
  /** Group ID. */
  group_id: UUID;
  /** User ID. */
  user_id: UUID;
  /** Username account identifier. */
  user_username: string;
  /** User display name. */
  user_display_name: string;
  /** Group role. */
  group_role: MembershipRole;
  /** User that created this membership. */
  created_by: UUID | null;
  /** User that revoked this membership. */
  revoked_by: UUID | null;
  /** Creation timestamp. */
  created_at: Timestamp;
  /** Last update timestamp. */
  updated_at: Timestamp;
  /** Revocation timestamp. */
  revoked_at: Timestamp | null;
};

/** Request body for creating a group membership. */
export type CreateGroupMembershipRequest = UserReference & {
  /** Group role. */
  group_role: MembershipRole;
};

/** Request body for updating a group membership role. */
export type UpdateGroupMembershipRequest = { group_role: MembershipRole };

/** Group membership response envelope. */
export type GroupMembershipResponse = { membership: GroupMembership };
