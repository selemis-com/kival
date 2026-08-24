//! Object commentary wire protocol types.

pub use kival_types::CommentStatus;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, serde::rfc3339};
use uuid::Uuid;

use crate::ListResponse;

/// Maximum number of Unicode scalar values accepted in one comment body.
pub const COMMENT_BODY_MAX_CHARS: usize = 20_000;

/// Maximum number of distinct users mentioned by one comment.
pub const COMMENT_MENTION_MAX_USERS: usize = 50;

/// Public identity embedded in a commentary response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommentAuthor {
    /// Stable user identifier.
    pub id: Uuid,
    /// Stable username.
    pub username: String,
    /// Current display name.
    pub display_name: String,
}

/// A stable user mention attached to a comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommentMention {
    /// Stable mentioned user identifier.
    pub user_id: Uuid,
    /// Current stable username.
    pub username: String,
    /// Current display name.
    pub display_name: String,
}

/// Mutable commentary attached to an object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Comment {
    /// Stable comment identifier.
    pub id: Uuid,
    /// Workspace security boundary.
    pub workspace_id: Uuid,
    /// Object that owns this commentary.
    pub object_id: Uuid,
    /// Stable thread identifier.
    pub thread_id: Uuid,
    /// Parent comment for a reply, or `None` for the thread root.
    pub parent_comment_id: Option<Uuid>,
    /// Current public identity of the comment author.
    pub author: CommentAuthor,
    /// Current lifecycle state.
    pub status: CommentStatus,
    /// Comment body while active.
    pub body: Option<String>,
    /// Explicit stable user mentions associated with the current body.
    pub mentions: Vec<CommentMention>,
    /// Creation timestamp.
    #[schemars(with = "String")]
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    /// Last modification timestamp.
    #[schemars(with = "String")]
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
    /// Most recent body-edit timestamp.
    #[schemars(with = "Option<String>")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub edited_at: Option<OffsetDateTime>,
    /// Soft-deletion timestamp.
    #[schemars(with = "Option<String>")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub deleted_at: Option<OffsetDateTime>,
    /// User who soft-deleted the comment.
    pub deleted_by: Option<Uuid>,
    /// Timestamp at which retention removed the body.
    #[schemars(with = "Option<String>")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub expired_at: Option<OffsetDateTime>,
    /// Explicit boundary consumed by retention workers.
    #[schemars(with = "Option<String>")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub retention_expires_at: Option<OffsetDateTime>,
}

/// A top-level discussion with a bounded page of comments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommentThread {
    /// Stable thread identifier.
    pub id: Uuid,
    /// Workspace security boundary.
    pub workspace_id: Uuid,
    /// Object that owns this thread.
    pub object_id: Uuid,
    /// User who created the root comment.
    pub created_by: Uuid,
    /// Creation timestamp.
    #[schemars(with = "String")]
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    /// Last thread activity timestamp.
    #[schemars(with = "String")]
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
    /// Resolution timestamp, when resolved.
    #[schemars(with = "Option<String>")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub resolved_at: Option<OffsetDateTime>,
    /// User who most recently resolved the thread.
    pub resolved_by: Option<Uuid>,
    /// Explicit boundary consumed by retention workers.
    #[schemars(with = "Option<String>")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub retention_expires_at: Option<OffsetDateTime>,
    /// Comments ordered by creation time, with the root first.
    pub comments: Vec<Comment>,
    /// Opaque cursor for loading more comments in this thread.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comments_next_cursor: Option<String>,
}

/// User who may currently be mentioned from commentary on an object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommentMentionCandidate {
    /// Stable user identifier.
    pub user_id: Uuid,
    /// Stable username inserted into commentary.
    pub username: String,
    /// Current display name.
    pub display_name: String,
}

/// Parameters for object-scoped mention autocomplete.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommentMentionCandidateParams {
    /// Username prefix or display-name fragment. Empty values list the first candidates.
    #[serde(default)]
    pub q: String,
    /// Maximum candidates to return. Defaults to 8 and is capped at 20.
    pub limit: Option<i64>,
}

impl CommentMentionCandidateParams {
    /// Returns the validated candidate limit.
    ///
    /// # Errors
    ///
    /// Returns an error when `limit` is less than one.
    pub fn checked_limit(&self) -> Result<i64, &'static str> {
        match self.limit {
            Some(limit) if limit < 1 => Err("limit must be at least 1"),
            Some(limit) => Ok(limit.min(20)),
            None => Ok(8),
        }
    }
}

/// Request body for creating a top-level comment or reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCommentRequest {
    /// Plain-text comment body.
    pub body: String,
    /// Additional stable IDs to mention. The server also resolves `@username` tokens in the body.
    #[serde(default)]
    pub mentioned_user_ids: Vec<Uuid>,
}

/// Request body for editing an existing comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateCommentRequest {
    /// Complete replacement body.
    pub body: String,
    /// Additional replacement mentions. The server also resolves `@username` tokens in the body.
    #[serde(default)]
    pub mentioned_user_ids: Vec<Uuid>,
}

/// Single-comment response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommentResponse {
    /// Created, edited, or deleted comment.
    pub comment: Comment,
}

/// Single-thread response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommentThreadResponse {
    /// Created or transitioned thread.
    pub thread: CommentThread,
}

/// Object commentary list response.
pub type CommentThreadListResponse = ListResponse<CommentThread>;

/// Paginated comments in a single commentary thread.
pub type CommentListResponse = ListResponse<Comment>;

/// Object-scoped mention autocomplete response.
pub type CommentMentionCandidateListResponse = ListResponse<CommentMentionCandidate>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mention_candidate_query_limits_are_validated() {
        assert_eq!(CommentMentionCandidateParams::default().checked_limit(), Ok(8));
        assert!(
            CommentMentionCandidateParams { q: String::new(), limit: Some(0) }
                .checked_limit()
                .is_err()
        );
        assert_eq!(
            CommentMentionCandidateParams { q: String::new(), limit: Some(25) }.checked_limit(),
            Ok(20),
        );
    }

    #[test]
    fn create_request_rejects_unknown_fields() {
        let result = serde_json::from_value::<CreateCommentRequest>(serde_json::json!({
            "body": "hello",
            "unexpected": true,
        }));

        assert!(result.is_err());
    }
}
