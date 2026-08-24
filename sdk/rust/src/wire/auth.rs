//! Authentication response types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, serde::rfc3339};
use uuid::Uuid;

use crate::{ListResponse, User};

/// Authenticated session response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AuthenticatedSessionResponse {
    /// Session expiration timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub expires_at: OffsetDateTime,
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
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    /// Last update timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
    /// Expiration timestamp.
    #[schemars(with = "String", extend("format" = "date-time"))]
    #[serde(with = "rfc3339")]
    pub expires_at: OffsetDateTime,
    /// Revocation timestamp.
    #[schemars(with = "Option<String>", extend("format" = "date-time"))]
    #[serde(with = "rfc3339::option")]
    pub revoked_at: Option<OffsetDateTime>,
    /// User that revoked this session.
    pub revoked_by: Option<Uuid>,
    /// Revocation reason.
    pub revocation_reason: Option<String>,
    /// Last-seen timestamp.
    #[schemars(with = "Option<String>", extend("format" = "date-time"))]
    #[serde(with = "rfc3339::option")]
    pub last_seen_at: Option<OffsetDateTime>,
    /// User agent recorded at session creation.
    pub user_agent: Option<String>,
    /// IP address recorded at session creation.
    pub ip_address: Option<String>,
}
