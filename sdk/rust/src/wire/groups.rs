//! Group wire protocol types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, serde::rfc3339};
use uuid::Uuid;

use crate::{ArchiveListStatus, ArchiveStatus, ListParams, PatchField};

/// Group collection query parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GroupListParams {
    /// Maximum number of groups to return.
    pub limit: Option<i64>,

    /// Opaque pagination cursor.
    pub cursor: Option<String>,

    /// Archive status filter.
    #[serde(default)]
    pub status: ArchiveListStatus,

    /// Case-insensitive group name search.
    pub q: Option<String>,
}

impl Default for GroupListParams {
    fn default() -> Self {
        Self { limit: None, cursor: None, status: ArchiveListStatus::Active, q: None }
    }
}

impl GroupListParams {
    /// Returns the normalized page limit.
    #[must_use]
    pub fn limit(&self) -> i64 {
        ListParams { limit: self.limit, cursor: self.cursor.clone() }.limit()
    }
}

pub use kival_types::MembershipRole;

/// Request body for creating a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CreateGroupRequest {
    /// Group name.
    pub name: String,
    /// Optional group description.
    pub description: Option<String>,
}

/// Request body for updating a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateGroupRequest {
    /// New group name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// New group description. `null` clears the description.
    #[serde(default, skip_serializing_if = "PatchField::is_missing")]
    pub description: PatchField<String>,
}

/// Group resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Group {
    /// Group ID.
    pub id: Uuid,
    /// Group name.
    pub name: String,
    /// Optional group description.
    pub description: Option<String>,
    /// Lifecycle status.
    pub status: ArchiveStatus,
    /// User that created this group.
    pub created_by: Option<Uuid>,
    /// User that archived this group.
    pub archived_by: Option<Uuid>,
    /// Creation timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    /// Last update timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
    /// Archive timestamp.
    #[schemars(with = "Option<String>", extend("format" = "date-time"))]
    #[serde(with = "rfc3339::option")]
    pub archived_at: Option<OffsetDateTime>,
}

/// Group response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GroupResponse {
    /// Group resource.
    pub group: Group,
}

/// Request body for creating a group membership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateGroupMembershipRequest {
    /// Member user ID. Exactly one of `user_id` and `username` must be supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<Uuid>,
    /// Account username. Exactly one of `user_id` and `username` must be supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Group role.
    pub group_role: MembershipRole,
}

/// Request body for updating a group membership role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateGroupMembershipRequest {
    /// New group role.
    pub group_role: MembershipRole,
}

/// Group membership resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GroupMembership {
    /// Membership ID.
    pub id: Uuid,
    /// Group ID.
    pub group_id: Uuid,
    /// User ID.
    pub user_id: Uuid,
    /// User username account identifier.
    pub user_username: String,
    /// User display name.
    pub user_display_name: String,
    /// Group role.
    pub group_role: MembershipRole,
    /// User that created this membership.
    pub created_by: Option<Uuid>,
    /// User that revoked this membership.
    pub revoked_by: Option<Uuid>,
    /// Creation timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    /// Last update timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
    /// Revocation timestamp.
    #[schemars(with = "Option<String>", extend("format" = "date-time"))]
    #[serde(with = "rfc3339::option")]
    pub revoked_at: Option<OffsetDateTime>,
}

/// Group membership response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GroupMembershipResponse {
    /// Group membership resource.
    pub membership: GroupMembership,
}
