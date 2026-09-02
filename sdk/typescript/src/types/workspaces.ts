import type {
  ArchiveListStatus,
  ArchiveStatus,
  PatchField,
  Timestamp,
  UserReference,
  UUID,
} from "./common.js";
import type { MembershipRole } from "./groups.js";
import type { ListParams } from "./pagination.js";

/** Workspace resource. */
export type Workspace = {
  /** Whether the authenticated user has pinned this workspace when returned from a directory. */
  pinned?: boolean;
  /** Time at which the authenticated user pinned this workspace. */
  pinned_at?: Timestamp | null;
  /** Workspace ID. */
  id: UUID;
  /** Workspace name. */
  name: string;
  /** Optional workspace description. */
  description: string | null;
  /** Lifecycle status. */
  status: ArchiveStatus;
  /**
   * Effective role derived from the authenticated user's workspace authority.
   *
   * API-key scopes remain an additional restriction for API-key requests.
   */
  effective_role: MembershipRole;
  /** User that created this workspace. */
  created_by: UUID | null;
  /** User that archived this workspace. */
  archived_by: UUID | null;
  /** Creation timestamp. */
  created_at: Timestamp;
  /** Last update timestamp. */
  updated_at: Timestamp;
  /** Archive timestamp. */
  archived_at: Timestamp | null;
};

/** Workspace resource enriched with actor-relative directory information. */
export type WorkspaceListItem = Workspace & { pinned: boolean; pinned_at: Timestamp | null };

/** Workspace collection query parameters. */
export type WorkspaceListParams = ListParams & {
  /** Archive status filter. Defaults to active workspaces. */
  status?: ArchiveListStatus;
  /** Case-insensitive workspace-name search. */
  q?: string | null;
  /** Restricts results by the authenticated user's personal pin state. */
  pinned?: boolean;
};

/** Workspace-group link collection query parameters. */
export type WorkspaceGroupListParams = ListParams & {
  /** Archive status filter. Defaults to active links. */
  status?: ArchiveListStatus;
};

/** Request body for creating a workspace. */
export type CreateWorkspaceRequest = { name: string; description?: string | null };

/** Request body for updating a workspace. */
export type UpdateWorkspaceRequest = {
  /** New workspace name. Omit to leave unchanged. */
  name?: string | null;
  /** New description. Omit to leave unchanged or use `null` to clear it. */
  description?: PatchField<string>;
};

/** Workspace response envelope. */
export type WorkspaceResponse = { workspace: Workspace };

/** Workspace membership resource. */
export type WorkspaceMembership = {
  /** Membership ID. */
  id: UUID;
  /** Workspace ID. */
  workspace_id: UUID;
  /** User ID. */
  user_id: UUID;
  /** Username account identifier. */
  user_username: string;
  /** User display name. */
  user_display_name: string;
  /** Workspace role. */
  workspace_role: MembershipRole;
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

/** Request body for creating a workspace membership. */
export type CreateWorkspaceMembershipRequest = UserReference & {
  /** Workspace role. */
  workspace_role: MembershipRole;
};

/** Request body for updating a workspace membership role. */
export type UpdateWorkspaceMembershipRequest = {
  /** New workspace role. */
  workspace_role: MembershipRole;
};

/** Workspace membership response envelope. */
export type WorkspaceMembershipResponse = { membership: WorkspaceMembership };

/** Request body for linking a group to a workspace. */
export type CreateWorkspaceGroupRequest = {
  /** Group ID. */
  group_id: UUID;
};

/** Workspace-group link resource. */
export type WorkspaceGroup = {
  /** Workspace-group link ID. */
  id: UUID;
  /** Workspace ID. */
  workspace_id: UUID;
  /** Group ID. */
  group_id: UUID;
  /** Human-readable group name. */
  group_name: string;
  /** Optional group description. */
  group_description: string | null;
  /** Lifecycle status. */
  status: ArchiveStatus;
  /** User that created this link. */
  created_by: UUID | null;
  /** User that archived this link. */
  archived_by: UUID | null;
  /** Creation timestamp. */
  created_at: Timestamp;
  /** Last update timestamp. */
  updated_at: Timestamp;
  /** Archive timestamp. */
  archived_at: Timestamp | null;
};

/** Workspace-group link response envelope. */
export type WorkspaceGroupResponse = { workspace_group: WorkspaceGroup };
