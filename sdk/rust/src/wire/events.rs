//! Event wire protocol types.

pub use kival_types::EventOrder;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{DEFAULT_LIMIT, MAX_LIMIT};

/// Event resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Event {
    /// Event ID.
    pub id: Uuid,

    /// Global event sequence.
    pub sequence_number: i64,

    /// Workspace ID.
    pub workspace_id: Option<Uuid>,

    /// Actor user ID.
    pub actor_user_id: Option<Uuid>,

    /// Actor username, when the event was performed by a user.
    pub actor_username: Option<String>,

    /// API key used by the actor, when delegated authentication was used.
    pub api_key_id: Option<Uuid>,

    /// User-defined API key label captured when the event was generated.
    pub api_key_label: Option<String>,

    /// Event kind.
    pub event_kind: String,

    /// Object ID.
    pub object_id: Option<Uuid>,

    /// Object version ID.
    pub object_version_id: Option<Uuid>,

    /// Object edge ID.
    pub object_edge_id: Option<Uuid>,

    /// Object grant ID.
    pub object_grant_id: Option<Uuid>,

    /// Commentary thread ID.
    pub comment_thread_id: Option<Uuid>,

    /// Comment ID.
    pub comment_id: Option<Uuid>,

    /// Group ID.
    pub group_id: Option<Uuid>,

    /// Target user ID.
    pub target_user_id: Option<Uuid>,

    /// Event payload.
    pub payload: Value,

    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
}

/// Event list query parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EventListParams {
    /// Maximum number of events to return.
    pub limit: Option<i64>,

    /// Return events after this sequence number.
    pub after_sequence: Option<i64>,

    /// Return events before this sequence number.
    pub before_sequence: Option<i64>,

    /// Event sequence ordering.
    #[serde(default)]
    pub order: EventOrder,

    /// Filter by event kind.
    pub event_kind: Option<String>,

    /// Filter by actor user ID.
    pub actor_user_id: Option<Uuid>,

    /// Filter by target user ID.
    pub target_user_id: Option<Uuid>,

    /// Filter by object ID.
    pub object_id: Option<Uuid>,

    /// Filter by group ID.
    pub group_id: Option<Uuid>,
}

impl EventListParams {
    /// Returns the normalized page limit.
    #[must_use]
    pub fn limit(&self) -> i64 {
        self.checked_limit().unwrap_or(1)
    }

    /// Returns the validated page limit.
    ///
    /// Low values are invalid. High values are capped to the maximum page size.
    ///
    /// # Errors
    ///
    /// Returns an error if the requested limit is less than 1.
    pub fn checked_limit(&self) -> Result<i64, &'static str> {
        match self.limit {
            Some(limit) if limit < 1 => Err("limit must be at least 1"),
            Some(limit) => Ok(limit.min(MAX_LIMIT)),
            None => Ok(DEFAULT_LIMIT),
        }
    }
}
