//! Object and object-version wire protocol types.

use chrono::{DateTime, Utc};
pub use kival_types::ObjectListOrder;
use kival_types::{MembershipRole, ObjectRole};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;
use uuid::Uuid;

use crate::{ArchiveListStatus, ArchiveStatus, ListParams};

/// Object collection query parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectListParams {
    /// Maximum number of objects to return.
    pub limit: Option<i64>,

    /// Opaque pagination cursor.
    pub cursor: Option<String>,

    /// Archive status filter.
    #[serde(default)]
    pub status: ArchiveListStatus,

    /// Sort order.
    #[serde(default)]
    pub order: ObjectListOrder,

    /// Restricts the list to the authenticated user's favorites when set.
    pub favorited: Option<bool>,

    /// Restricts the list by the authenticated user's personal pin state.
    pub pinned: Option<bool>,
}

impl Default for ObjectListParams {
    fn default() -> Self {
        Self {
            limit: None,
            cursor: None,
            status: ArchiveListStatus::Active,
            order: ObjectListOrder::Created,
            favorited: None,
            pinned: None,
        }
    }
}

impl ObjectListParams {
    /// Returns the normalized page limit.
    #[must_use]
    pub fn limit(&self) -> i64 {
        ListParams { limit: self.limit, cursor: self.cursor.clone() }.limit()
    }
}

/// Request body for creating an object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateObjectRequest {
    /// Object title.
    pub title: String,

    /// Initial object body.
    #[serde(default)]
    pub body: String,

    /// Initial flat object metadata.
    #[serde(default = "empty_object")]
    #[schemars(with = "std::collections::BTreeMap<String, Value>")]
    pub metadata: Value,
}

/// Request body for updating an object with optimistic concurrency control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateObjectRequest {
    /// Expected current version ID for optimistic concurrency control.
    ///
    /// The update fails with `409 Conflict` if the object has changed since this version was read.
    pub expected_current_version_id: Uuid,

    /// New object title. Omitted values inherit from the current version.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_non_null_option"
    )]
    pub title: Option<String>,

    /// New object body. Omitted values inherit from the current version.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_non_null_option"
    )]
    pub body: Option<String>,

    /// New flat object metadata. Omitted values inherit from the current version.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_non_null_option"
    )]
    pub metadata: Option<Value>,
}

/// Deserializes an optional field while rejecting an explicit JSON `null`.
///
/// Missing fields are supplied by `#[serde(default)]` as `None`; present fields
/// must contain a concrete value.
fn deserialize_non_null_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)?
        .map(Some)
        .ok_or_else(|| de::Error::custom("null is not allowed; omit the field instead"))
}

/// Object resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectResource {
    /// Object ID.
    pub id: Uuid,

    /// Parent workspace ID.
    pub workspace_id: Uuid,

    /// Current version ID.
    pub current_version_id: Option<Uuid>,

    /// Title projected from the current immutable version.
    pub title: String,

    /// Lifecycle status.
    pub status: ArchiveStatus,

    /// User that created this object.
    pub created_by: Option<Uuid>,

    /// User that archived this object.
    pub archived_by: Option<Uuid>,

    /// Creation timestamp.
    pub created_at: DateTime<Utc>,

    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,

    /// Archive timestamp.
    pub archived_at: Option<DateTime<Utc>>,
}

/// Object resource enriched with scan-oriented workspace-list information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectListItem {
    /// Core object resource.
    #[serde(flatten)]
    pub object: ObjectResource,

    /// Username that created the current object version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by_username: Option<String>,

    /// Display name of the user that created the current object version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by_display_name: Option<String>,

    /// Updater's active workspace role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by_workspace_role: Option<MembershipRole>,

    /// Updater's effective access role for this object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by_object_role: Option<ObjectRole>,

    /// Number of visible active object connections.
    pub connection_count: i64,

    /// Number of unresolved, unexpired commentary threads.
    pub unresolved_thread_count: i64,

    /// Whether the authenticated user has favorited this object.
    pub favorited: bool,

    /// Whether the authenticated user has pinned this object.
    pub pinned: bool,

    /// Time at which the authenticated user pinned this object.
    pub pinned_at: Option<DateTime<Utc>>,
}

impl std::ops::Deref for ObjectListItem {
    type Target = ObjectResource;

    fn deref(&self) -> &Self::Target {
        &self.object
    }
}

/// Object version summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectVersion {
    /// Version ID.
    pub id: Uuid,

    /// Object ID.
    pub object_id: Uuid,

    /// Monotonic version number within the object.
    pub version_number: i64,

    /// Version title.
    pub title: String,

    /// Version body.
    pub body: String,

    /// Version metadata.
    #[schemars(with = "std::collections::BTreeMap<String, Value>")]
    pub metadata: Value,

    /// User that created this version.
    pub created_by: Option<Uuid>,

    /// Username of the user that created this version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by_username: Option<String>,

    /// Display name of the user that created this version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by_display_name: Option<String>,

    /// Creator's current effective workspace role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by_workspace_role: Option<MembershipRole>,

    /// Creator's current effective access role for this object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by_object_role: Option<ObjectRole>,

    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
}

/// Request to reuse an existing object attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReuseObjectAttachmentRequest {
    /// Authorized source attachment ID.
    pub source_attachment_id: Uuid,

    /// Optional target object version ID.
    pub version_id: Option<Uuid>,
}

/// Query parameters for uploading an object attachment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadObjectAttachmentParams {
    /// Optional object version ID this attachment belongs to.
    pub version_id: Option<Uuid>,

    /// Optional attachment display name.
    pub name: Option<String>,

    /// Optional media type.
    pub media_type: Option<String>,

    /// Optional attachment metadata as a flat JSON object string.
    pub metadata: Option<String>,
}

/// Object attachment resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectAttachment {
    /// Attachment ID.
    pub id: Uuid,

    /// Parent workspace ID.
    pub workspace_id: Uuid,

    /// Parent object ID.
    pub object_id: Uuid,

    /// Optional object version ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<Uuid>,

    /// Stable SHA-256 content reference for the attachment bytes.
    pub content_ref: String,

    /// Stored attachment length in bytes.
    pub size_bytes: u64,

    /// Best-effort provenance for a reused attachment.
    ///
    /// This is cleared if the source attachment is deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_attachment_id: Option<Uuid>,

    /// Optional attachment display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional media type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    /// Attachment metadata.
    #[schemars(with = "std::collections::BTreeMap<String, Value>")]
    pub metadata: Value,

    /// User that created this attachment.
    pub created_by: Option<Uuid>,

    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
}

/// Object attachment response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectAttachmentResponse {
    /// Object attachment resource.
    pub attachment: ObjectAttachment,
}

/// Object response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectResponse {
    /// Object resource.
    pub object: ObjectResource,

    /// Current version, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_version: Option<ObjectVersion>,

    /// Effective role derived from the authenticated user's object authority.
    /// API-key scopes remain an additional restriction for API-key requests.
    pub effective_role: ObjectRole,
}

/// Object version response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectVersionResponse {
    /// Object version resource.
    pub version: ObjectVersion,
}

/// Wikilink derived from an immutable object version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectVersionWikilink {
    /// Normalized title target authored inside the double brackets.
    pub raw_target: String,

    /// Optional display text authored after the `|` separator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_text: Option<String>,

    /// Resolved target object when the requesting user can read it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_object_id: Option<Uuid>,
}

/// Object-version wikilink response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectVersionWikilinksResponse {
    /// Wikilinks in authored source order.
    pub items: Vec<ObjectVersionWikilink>,
}

/// Returns an empty JSON object value.
fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}
