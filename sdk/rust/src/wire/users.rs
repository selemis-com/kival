//! User wire protocol types.

use chrono::{DateTime, Utc};
pub use kival_types::{UserListStatus, UserStatus};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ListParams;

/// User collection query parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UserListParams {
    /// Maximum number of users to return.
    pub limit: Option<i64>,

    /// Opaque pagination cursor.
    pub cursor: Option<String>,

    /// User status filter.
    #[serde(default)]
    pub status: UserListStatus,

    /// Case-insensitive username or display-name search.
    pub q: Option<String>,
}

impl Default for UserListParams {
    fn default() -> Self {
        Self { limit: None, cursor: None, status: UserListStatus::Active, q: None }
    }
}

impl UserListParams {
    /// Returns the normalized page limit.
    #[must_use]
    pub fn limit(&self) -> i64 {
        ListParams { limit: self.limit, cursor: self.cursor.clone() }.limit()
    }
}

/// Request body for updating a user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateUserRequest {
    /// New display name.
    pub display_name: Option<String>,
}

/// User resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct User {
    /// User ID.
    pub id: Uuid,
    /// Username.
    pub username: String,
    /// Display name.
    pub display_name: String,
    /// Lifecycle status.
    pub status: UserStatus,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Disable timestamp.
    pub disabled_at: Option<DateTime<Utc>>,
    /// User that disabled this user.
    pub disabled_by: Option<Uuid>,
}

/// User response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UserResponse {
    /// User resource.
    pub user: User,
    /// Whether the current authenticated user is a global administrator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_global_admin: Option<bool>,
    /// Whether the current authenticated user may manage any groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_manage_groups: Option<bool>,
}
