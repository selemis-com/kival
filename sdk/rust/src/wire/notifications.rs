//! Notification, inbox, and realtime wire protocol types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{DEFAULT_LIMIT, MAX_LIMIT};

/// Effective notification preference for one object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectNotificationPreference {
    /// Workspace containing the object.
    pub workspace_id: Uuid,

    /// Object whose ordinary notifications are configured.
    pub object_id: Uuid,

    /// Whether ordinary object activity may generate notifications.
    pub ordinary_notifications_enabled: bool,

    /// Whether the value comes from an explicit stored preference.
    pub explicit: bool,

    /// Last explicit preference update, when one exists.
    pub updated_at: Option<DateTime<Utc>>,
}

/// Request body for changing an object notification preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateObjectNotificationPreferenceRequest {
    /// Whether ordinary activity on the object should generate notifications.
    pub ordinary_notifications_enabled: bool,
}

/// One durable personal inbox entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InboxEntry {
    /// Inbox entry ID.
    pub id: Uuid,

    /// Monotonic sequence used for stable pagination.
    pub sequence_number: i64,

    /// Recipient user ID.
    pub recipient_user_id: Uuid,

    /// Source workspace ID.
    pub workspace_id: Uuid,

    /// Current workspace name resolved under the recipient's authorization context.
    pub workspace_name: String,

    /// Source object ID for object-scoped notifications.
    pub object_id: Option<Uuid>,

    /// Current object title for object-scoped notifications.
    pub object_title: Option<String>,

    /// Earliest durable source event represented by this entry.
    pub source_event_id: Uuid,

    /// Latest durable source event represented by this entry.
    pub latest_event_id: Uuid,

    /// Latest actor user ID, when available.
    pub actor_user_id: Option<Uuid>,

    /// Latest actor username, when available and currently visible.
    pub actor_username: Option<String>,

    /// Stable notification presentation type.
    pub notification_type: String,

    /// Reason the entry was generated.
    pub reason: String,

    /// Number of source events represented by this entry.
    pub event_count: i32,

    /// Source commentary thread ID, when applicable.
    pub thread_id: Option<Uuid>,

    /// Source comment ID, when applicable.
    pub comment_id: Option<Uuid>,

    /// Truncated current comment text, when the source comment is still available.
    pub comment_excerpt: Option<String>,

    /// Read timestamp.
    pub read_at: Option<DateTime<Utc>>,

    /// Creation timestamp.
    pub created_at: DateTime<Utc>,

    /// Last projection update timestamp.
    pub updated_at: DateTime<Utc>,
}

/// Inbox list query parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InboxListParams {
    /// Maximum number of entries to return.
    pub limit: Option<i64>,

    /// Opaque pagination cursor from a previous response.
    pub cursor: Option<String>,

    /// Return unread entries only.
    #[serde(default)]
    pub unread_only: bool,

    /// Restrict entries to one workspace.
    pub workspace_id: Option<Uuid>,
}

impl InboxListParams {
    /// Returns the normalized page limit.
    #[must_use]
    pub fn limit(&self) -> i64 {
        self.checked_limit().unwrap_or(1)
    }

    /// Returns the validated page limit.
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

/// Request body for changing one inbox entry's read state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateInboxEntryRequest {
    /// Whether the inbox entry should be marked read.
    pub read: bool,
}

/// Request body for marking a bounded inbox range read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkInboxReadRequest {
    /// Optional workspace scope.
    pub workspace_id: Option<Uuid>,

    /// Optional inclusive sequence boundary.
    pub through_sequence: Option<i64>,
}

/// Response returned after a bulk inbox update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InboxUpdatedResponse {
    /// Number of inbox entries updated.
    pub updated: u64,
}

/// Current unread inbox count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InboxUnreadCountResponse {
    /// Number of currently visible unread inbox entries.
    pub unread_count: i64,
}

/// Lightweight realtime invalidation message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RealtimeMessage {
    /// Stable invalidation type.
    #[serde(rename = "type")]
    pub kind: String,

    /// Related workspace ID.
    pub workspace_id: Option<Uuid>,

    /// Related object ID.
    pub object_id: Option<Uuid>,

    /// Related durable event ID.
    pub event_id: Option<Uuid>,

    /// Related inbox entry ID.
    pub inbox_entry_id: Option<Uuid>,
}
