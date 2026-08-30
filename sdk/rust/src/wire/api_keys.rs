//! API key resources and requests.

use chrono::{DateTime, Utc};
pub use kival_types::ApiKeyScope;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ListResponse;

/// Request body for creating an API key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateApiKeyRequest {
    /// Stable user-defined label identifying the key and recorded in audit events.
    pub label: String,
    /// Capabilities delegated to the key. These can only reduce the owning user's authority.
    pub scopes: Vec<ApiKeyScope>,
    /// Workspaces in which the key may exercise workspace-scoped capabilities.
    /// An empty list grants no workspace-scoped access.
    pub workspace_ids: Vec<Uuid>,
    /// Optional expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Request body for replacing an active API key's delegated authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateApiKeyRequest {
    /// Expected mutable authorization revision.
    pub authorization_revision: i32,

    /// Capabilities delegated to the key. These can only reduce the owning user's authority.
    pub scopes: Vec<ApiKeyScope>,
    /// Workspaces in which the key may exercise workspace-scoped capabilities.
    /// An empty list grants no workspace-scoped access.
    pub workspace_ids: Vec<Uuid>,
}

/// API key metadata. The secret token is never included after creation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ApiKey {
    /// API key ID.
    pub id: Uuid,
    /// User whose authority the key delegates.
    pub user_id: Uuid,
    /// Stable user-defined label identifying the key and recorded in audit events.
    pub label: String,
    /// Mutable authorization revision.
    pub authorization_revision: i32,
    /// Capabilities delegated to the key.
    pub scopes: Vec<ApiKeyScope>,
    /// Workspaces in which the key may exercise workspace-scoped capabilities.
    pub workspace_ids: Vec<Uuid>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Optional expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
    /// Revocation timestamp.
    pub revoked_at: Option<DateTime<Utc>>,
    /// Last authenticated use timestamp. Updates are intentionally coalesced.
    pub last_used_at: Option<DateTime<Utc>>,
}

/// API key response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ApiKeyResponse {
    /// API key metadata.
    pub api_key: ApiKey,
}

/// API key creation response. The token is returned exactly once.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CreateApiKeyResponse {
    /// Created API key metadata.
    pub api_key: ApiKey,
    /// Plaintext bearer token. Kival does not store this value.
    pub token: String,
}

/// API key list response envelope.
pub type ApiKeyListResponse = ListResponse<ApiKey>;

impl std::fmt::Debug for CreateApiKeyResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CreateApiKeyResponse")
            .field("api_key", &self.api_key)
            .field("token", &"[REDACTED]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use crate::ApiKeyScope;

    #[test]
    fn all_scopes_round_trip_through_stable_wire_names() {
        for scope in ApiKeyScope::ALL {
            assert_eq!(scope.as_str().parse(), Ok(*scope));

            let json = serde_json::to_string(scope).expect("scope should serialize");
            let decoded =
                serde_json::from_str::<ApiKeyScope>(&json).expect("scope should deserialize");
            assert_eq!(decoded, *scope);
        }
    }
}
