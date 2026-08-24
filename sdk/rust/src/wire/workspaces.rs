//! Workspace wire protocol types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, serde::rfc3339};
use uuid::Uuid;

use crate::{ArchiveListStatus, ArchiveStatus, ListParams, MembershipRole, PatchField};

/// Workspace collection query parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceListParams {
    /// Maximum number of workspaces to return.
    pub limit: Option<i64>,

    /// Opaque pagination cursor.
    pub cursor: Option<String>,

    /// Archive status filter.
    #[serde(default)]
    pub status: ArchiveListStatus,

    /// Case-insensitive workspace name search.
    pub q: Option<String>,

    /// Restricts the list by the authenticated user's personal pin state.
    pub pinned: Option<bool>,
}

impl Default for WorkspaceListParams {
    fn default() -> Self {
        Self { limit: None, cursor: None, status: ArchiveListStatus::Active, q: None, pinned: None }
    }
}

impl WorkspaceListParams {
    /// Returns the normalized page limit.
    #[must_use]
    pub fn limit(&self) -> i64 {
        ListParams { limit: self.limit, cursor: self.cursor.clone() }.limit()
    }
}

/// Workspace-group link collection query parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceGroupListParams {
    /// Maximum number of links to return.
    pub limit: Option<i64>,

    /// Opaque pagination cursor.
    pub cursor: Option<String>,

    /// Archive status filter.
    #[serde(default)]
    pub status: ArchiveListStatus,
}

impl Default for WorkspaceGroupListParams {
    fn default() -> Self {
        Self { limit: None, cursor: None, status: ArchiveListStatus::Active }
    }
}

impl WorkspaceGroupListParams {
    /// Returns the normalized page limit.
    #[must_use]
    pub fn limit(&self) -> i64 {
        ListParams { limit: self.limit, cursor: self.cursor.clone() }.limit()
    }
}

/// Request body for creating a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CreateWorkspaceRequest {
    /// Workspace name.
    pub name: String,

    /// Optional workspace description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Request body for updating a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateWorkspaceRequest {
    /// New workspace name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// New workspace description. `null` clears the description.
    #[serde(default, skip_serializing_if = "PatchField::is_missing")]
    pub description: PatchField<String>,
}

/// Workspace resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Workspace {
    /// Workspace ID.
    pub id: Uuid,

    /// Workspace name.
    pub name: String,

    /// Optional workspace description.
    pub description: Option<String>,

    /// Lifecycle status.
    pub status: ArchiveStatus,

    /// Effective role derived from the authenticated user's workspace authority.
    /// API-key scopes remain an additional restriction for API-key requests.
    pub effective_role: MembershipRole,

    /// User that created this workspace.
    pub created_by: Option<Uuid>,

    /// User that archived this workspace.
    pub archived_by: Option<Uuid>,

    /// Creation timestamp.
    #[schemars(with = "String")]
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,

    /// Last update timestamp.
    #[schemars(with = "String")]
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,

    /// Archive timestamp.
    #[schemars(with = "Option<String>")]
    #[serde(with = "rfc3339::option")]
    pub archived_at: Option<OffsetDateTime>,
}

/// Workspace response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceResponse {
    /// Workspace resource.
    pub workspace: Workspace,
}

/// Workspace resource enriched with actor-relative directory information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceListItem {
    /// Core workspace resource.
    #[serde(flatten)]
    pub workspace: Workspace,

    /// Whether the authenticated user has pinned this workspace.
    pub pinned: bool,

    /// Time at which the authenticated user pinned this workspace.
    #[schemars(with = "Option<String>")]
    #[serde(with = "rfc3339::option")]
    pub pinned_at: Option<OffsetDateTime>,
}

impl std::ops::Deref for WorkspaceListItem {
    type Target = Workspace;

    fn deref(&self) -> &Self::Target {
        &self.workspace
    }
}

/// Request body for creating a workspace membership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateWorkspaceMembershipRequest {
    /// Member user ID. Exactly one of `user_id` and `username` must be supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<Uuid>,

    /// Account username. Exactly one of `user_id` and `username` must be supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// Workspace role.
    pub workspace_role: MembershipRole,
}

/// Request body for updating a workspace membership role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateWorkspaceMembershipRequest {
    /// New workspace role.
    pub workspace_role: MembershipRole,
}

/// Workspace membership resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceMembership {
    /// Membership ID.
    pub id: Uuid,

    /// Workspace ID.
    pub workspace_id: Uuid,

    /// User ID.
    pub user_id: Uuid,

    /// Username.
    pub user_username: String,

    /// User display name.
    pub user_display_name: String,

    /// Workspace role.
    pub workspace_role: MembershipRole,

    /// User that created this membership.
    pub created_by: Option<Uuid>,

    /// User that revoked this membership.
    pub revoked_by: Option<Uuid>,

    /// Creation timestamp.
    #[schemars(with = "String")]
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,

    /// Last update timestamp.
    #[schemars(with = "String")]
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,

    /// Revocation timestamp.
    #[schemars(with = "Option<String>")]
    #[serde(with = "rfc3339::option")]
    pub revoked_at: Option<OffsetDateTime>,
}

/// Workspace membership response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceMembershipResponse {
    /// Workspace membership resource.
    pub membership: WorkspaceMembership,
}

/// Request body for linking a group to a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CreateWorkspaceGroupRequest {
    /// Group ID.
    pub group_id: Uuid,
}

/// Workspace group link resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceGroup {
    /// Workspace group link ID.
    pub id: Uuid,

    /// Workspace ID.
    pub workspace_id: Uuid,

    /// Group ID.
    pub group_id: Uuid,

    /// Human-readable group name.
    pub group_name: String,

    /// Optional group description.
    pub group_description: Option<String>,

    /// Lifecycle status.
    pub status: ArchiveStatus,

    /// User that created this link.
    pub created_by: Option<Uuid>,

    /// User that archived this link.
    pub archived_by: Option<Uuid>,

    /// Creation timestamp.
    #[schemars(with = "String")]
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,

    /// Last update timestamp.
    #[schemars(with = "String")]
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,

    /// Archive timestamp.
    #[schemars(with = "Option<String>")]
    #[serde(with = "rfc3339::option")]
    pub archived_at: Option<OffsetDateTime>,
}

/// Workspace group response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceGroupResponse {
    /// Workspace group link resource.
    pub workspace_group: WorkspaceGroup,
}
