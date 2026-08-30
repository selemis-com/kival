//! Authentication response types.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ListResponse, User};

/// Authenticated session response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AuthenticatedSessionResponse {
    /// Session expiration timestamp.
    pub expires_at: DateTime<Utc>,
    /// Authenticated user.
    pub user: User,
}

/// Session-only response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionOnlyResponse {
    /// Session resource.
    pub session: Session,
}

/// Sessions list response envelope.
pub type SessionListResponse = ListResponse<Session>;

/// Session resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Session {
    /// Session ID.
    pub id: Uuid,
    /// Whether this is the browser session making the list request.
    pub is_current: bool,
    /// User ID.
    pub user_id: Uuid,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Expiration timestamp.
    pub expires_at: DateTime<Utc>,
    /// Revocation timestamp.
    pub revoked_at: Option<DateTime<Utc>>,
    /// User that revoked this session.
    pub revoked_by: Option<Uuid>,
    /// Revocation reason.
    pub revocation_reason: Option<String>,
    /// Last-seen timestamp.
    pub last_seen_at: Option<DateTime<Utc>>,
    /// User agent recorded at session creation.
    pub user_agent: Option<String>,
    /// IP address recorded at session creation.
    pub ip_address: Option<String>,
}
