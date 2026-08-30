//! Audit event state bindings.

use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{EventOrder, Result};

/// Audit event kinds emitted by Kival state transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    /// Initial administrator bootstrap completed.
    AdminBootstrapCompleted,
    /// Deployment operator issued passkey recovery.
    AdminPasskeyRecoveryIssued,
    /// API key created.
    AuthApiKeyCreated,
    /// API key revoked.
    AuthApiKeyRevoked,
    /// API key updated.
    AuthApiKeyUpdated,
    /// Interactive session logged out.
    AuthLogout,
    /// Passkey enrolled.
    AuthPasskeyEnrolled,
    /// Passkey enrollment code consumed.
    AuthPasskeyEnrollmentCodeConsumed,
    /// Fresh passkey authentication completed.
    AuthPasskeyFreshAuthenticated,
    /// Passkey login completed.
    AuthPasskeyLogin,
    /// Passkey revoked.
    AuthPasskeyRevoked,
    /// Session revoked.
    AuthSessionRevoked,
    /// Comment created.
    CommentCreated,
    /// Comment deleted.
    CommentDeleted,
    /// Comment edited.
    CommentEdited,
    /// User mentioned in a comment.
    CommentMentioned,
    /// Reply created.
    CommentReplied,
    /// Comment thread reopened.
    CommentThreadReopened,
    /// Comment thread resolved.
    CommentThreadResolved,
    /// Group archived.
    GroupArchived,
    /// Group created.
    GroupCreated,
    /// Group membership created.
    GroupMembershipCreated,
    /// Group membership revoked.
    GroupMembershipRevoked,
    /// Group membership updated.
    GroupMembershipUpdated,
    /// Group unarchived.
    GroupUnarchived,
    /// Group updated.
    GroupUpdated,
    /// Object archived.
    ObjectArchived,
    /// Object attachment created.
    ObjectAttachmentCreated,
    /// Object created.
    ObjectCreated,
    /// Object notification preference changed.
    ObjectNotificationPreferenceChanged,
    /// Object references updated.
    ObjectReferencesUpdated,
    /// Object unarchived.
    ObjectUnarchived,
    /// Object updated.
    ObjectUpdated,
    /// Object version appended.
    ObjectVersionAppended,
    /// Object wikilinks re-resolved.
    ObjectWikilinksReresolved,
    /// Object edge created.
    ObjectEdgeCreated,
    /// Object edge revoked.
    ObjectEdgeRevoked,
    /// Object grant created.
    ObjectGrantCreated,
    /// Object grant revoked.
    ObjectGrantRevoked,
    /// Object grant updated.
    ObjectGrantUpdated,
    /// User created.
    UserCreated,
    /// User disabled.
    UserDisabled,
    /// User enabled.
    UserEnabled,
    /// User updated.
    UserUpdated,
    /// Workspace archived.
    WorkspaceArchived,
    /// Workspace created.
    WorkspaceCreated,
    /// Workspace group link archived.
    WorkspaceGroupArchived,
    /// Workspace group linked.
    WorkspaceGroupLinked,
    /// Workspace group link unarchived.
    WorkspaceGroupUnarchived,
    /// Workspace membership created.
    WorkspaceMembershipCreated,
    /// Workspace membership revoked.
    WorkspaceMembershipRevoked,
    /// Workspace membership updated.
    WorkspaceMembershipUpdated,
    /// Workspace unarchived.
    WorkspaceUnarchived,
    /// Workspace updated.
    WorkspaceUpdated,
}

impl EventKind {
    /// Returns the stable persisted event-kind string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AdminBootstrapCompleted => "admin.bootstrap.completed",
            Self::AdminPasskeyRecoveryIssued => "admin.passkey_recovery_issued",
            Self::AuthApiKeyCreated => "auth.api_key_created",
            Self::AuthApiKeyRevoked => "auth.api_key_revoked",
            Self::AuthApiKeyUpdated => "auth.api_key_updated",
            Self::AuthLogout => "auth.logout",
            Self::AuthPasskeyEnrolled => "auth.passkey_enrolled",
            Self::AuthPasskeyEnrollmentCodeConsumed => "auth.passkey_enrollment_code_consumed",
            Self::AuthPasskeyFreshAuthenticated => "auth.passkey_fresh_authenticated",
            Self::AuthPasskeyLogin => "auth.passkey_login",
            Self::AuthPasskeyRevoked => "auth.passkey_revoked",
            Self::AuthSessionRevoked => "auth.session_revoked",
            Self::CommentCreated => "comment.created",
            Self::CommentDeleted => "comment.deleted",
            Self::CommentEdited => "comment.edited",
            Self::CommentMentioned => "comment.mentioned",
            Self::CommentReplied => "comment.replied",
            Self::CommentThreadReopened => "comment_thread.reopened",
            Self::CommentThreadResolved => "comment_thread.resolved",
            Self::GroupArchived => "group.archived",
            Self::GroupCreated => "group.created",
            Self::GroupMembershipCreated => "group.membership_created",
            Self::GroupMembershipRevoked => "group.membership_revoked",
            Self::GroupMembershipUpdated => "group.membership_updated",
            Self::GroupUnarchived => "group.unarchived",
            Self::GroupUpdated => "group.updated",
            Self::ObjectArchived => "object.archived",
            Self::ObjectAttachmentCreated => "object.attachment_created",
            Self::ObjectCreated => "object.created",
            Self::ObjectNotificationPreferenceChanged => "object.notification_preference_changed",
            Self::ObjectReferencesUpdated => "object.references_updated",
            Self::ObjectUnarchived => "object.unarchived",
            Self::ObjectUpdated => "object.updated",
            Self::ObjectVersionAppended => "object.version_appended",
            Self::ObjectWikilinksReresolved => "object.wikilinks_reresolved",
            Self::ObjectEdgeCreated => "object_edge.created",
            Self::ObjectEdgeRevoked => "object_edge.revoked",
            Self::ObjectGrantCreated => "object_grant.created",
            Self::ObjectGrantRevoked => "object_grant.revoked",
            Self::ObjectGrantUpdated => "object_grant.updated",
            Self::UserCreated => "user.created",
            Self::UserDisabled => "user.disabled",
            Self::UserEnabled => "user.enabled",
            Self::UserUpdated => "user.updated",
            Self::WorkspaceArchived => "workspace.archived",
            Self::WorkspaceCreated => "workspace.created",
            Self::WorkspaceGroupArchived => "workspace.group_archived",
            Self::WorkspaceGroupLinked => "workspace.group_linked",
            Self::WorkspaceGroupUnarchived => "workspace.group_unarchived",
            Self::WorkspaceMembershipCreated => "workspace.membership_created",
            Self::WorkspaceMembershipRevoked => "workspace.membership_revoked",
            Self::WorkspaceMembershipUpdated => "workspace.membership_updated",
            Self::WorkspaceUnarchived => "workspace.unarchived",
            Self::WorkspaceUpdated => "workspace.updated",
        }
    }
}

impl std::fmt::Display for EventKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// API-key provenance captured with an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeyAttribution {
    /// API key that authenticated the transition.
    pub id: Uuid,
    /// Stable user-defined key label captured for audit output.
    pub label: String,
}

/// Event fields accepted by the authoritative event state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventInsert {
    /// Event kind.
    pub event_kind: EventKind,
    /// User that caused the event.
    pub actor_user_id: Uuid,
    /// Delegated API-key provenance, when present.
    pub api_key: Option<ApiKeyAttribution>,
    /// Associated workspace.
    pub workspace_id: Option<Uuid>,
    /// Associated object.
    pub object_id: Option<Uuid>,
    /// Associated object version.
    pub object_version_id: Option<Uuid>,
    /// Associated object edge.
    pub object_edge_id: Option<Uuid>,
    /// Associated object grant.
    pub object_grant_id: Option<Uuid>,
    /// Associated commentary thread.
    pub comment_thread_id: Option<Uuid>,
    /// Associated comment.
    pub comment_id: Option<Uuid>,
    /// Associated group.
    pub group_id: Option<Uuid>,
    /// Target user.
    pub target_user_id: Option<Uuid>,
    /// Event payload.
    pub payload: Value,
}

impl EventInsert {
    /// Creates an event insert with its required fields.
    #[must_use]
    pub const fn new(actor_user_id: Uuid, event_kind: EventKind, payload: Value) -> Self {
        Self {
            event_kind,
            actor_user_id,
            api_key: None,
            workspace_id: None,
            object_id: None,
            object_version_id: None,
            object_edge_id: None,
            object_grant_id: None,
            comment_thread_id: None,
            comment_id: None,
            group_id: None,
            target_user_id: None,
            payload,
        }
    }

    /// Attaches delegated API-key provenance.
    #[must_use]
    pub fn api_key(mut self, api_key: Option<ApiKeyAttribution>) -> Self {
        self.api_key = api_key;
        self
    }

    /// Attaches a workspace target.
    #[must_use]
    pub const fn workspace(mut self, workspace_id: Uuid) -> Self {
        self.workspace_id = Some(workspace_id);
        self
    }
    /// Attaches an object target.
    #[must_use]
    pub const fn object(mut self, object_id: Uuid) -> Self {
        self.object_id = Some(object_id);
        self
    }
    /// Attaches an object-version target.
    #[must_use]
    pub const fn object_version(mut self, object_version_id: Uuid) -> Self {
        self.object_version_id = Some(object_version_id);
        self
    }
    /// Attaches an object-edge target.
    #[must_use]
    pub const fn object_edge(mut self, object_edge_id: Uuid) -> Self {
        self.object_edge_id = Some(object_edge_id);
        self
    }
    /// Attaches an object-grant target.
    #[must_use]
    pub const fn object_grant(mut self, object_grant_id: Uuid) -> Self {
        self.object_grant_id = Some(object_grant_id);
        self
    }
    /// Attaches a commentary-thread target.
    #[must_use]
    pub const fn comment_thread(mut self, comment_thread_id: Uuid) -> Self {
        self.comment_thread_id = Some(comment_thread_id);
        self
    }
    /// Attaches a comment target.
    #[must_use]
    pub const fn comment(mut self, comment_id: Uuid) -> Self {
        self.comment_id = Some(comment_id);
        self
    }
    /// Attaches a group target.
    #[must_use]
    pub const fn group(mut self, group_id: Uuid) -> Self {
        self.group_id = Some(group_id);
        self
    }
    /// Attaches a target user.
    #[must_use]
    pub const fn target_user(mut self, target_user_id: Uuid) -> Self {
        self.target_user_id = Some(target_user_id);
        self
    }
}

/// Stored event projection.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct EventRow {
    /// Event ID.
    pub id: Uuid,
    /// Global event sequence.
    pub sequence_number: i64,
    /// Workspace ID.
    pub workspace_id: Option<Uuid>,
    /// Actor user ID.
    pub actor_user_id: Option<Uuid>,
    /// Actor username.
    pub actor_username: Option<String>,
    /// API key used by the actor.
    pub api_key_id: Option<Uuid>,
    /// API key label captured with the event.
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

/// Filters and ordering for event projections.
#[derive(Debug, Clone, Copy)]
pub struct ListEvents<'a> {
    /// Lower exclusive sequence bound.
    pub after_sequence: Option<i64>,
    /// Upper exclusive sequence bound.
    pub before_sequence: Option<i64>,
    /// Optional exact event kind.
    pub event_kind: Option<&'a str>,
    /// Optional actor user.
    pub actor_user_id: Option<Uuid>,
    /// Optional target user.
    pub target_user_id: Option<Uuid>,
    /// Optional object.
    pub object_id: Option<Uuid>,
    /// Optional group.
    pub group_id: Option<Uuid>,
    /// `asc` or `desc` sequence ordering.
    pub order: EventOrder,
    /// Maximum rows.
    pub limit: i64,
}

/// Appends an audit event inside an existing transaction and returns its ID.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the event.
pub async fn append_event(tx: &mut Transaction<'_, Postgres>, event: EventInsert) -> Result<Uuid> {
    let (api_key_id, api_key_label) =
        event.api_key.map_or((None, None), |api_key| (Some(api_key.id), Some(api_key.label)));

    Ok(sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO kival.events (
            workspace_id, actor_user_id, api_key_id, api_key_label, event_kind,
            object_id, object_version_id, object_edge_id, object_grant_id,
            comment_thread_id, comment_id, group_id, target_user_id, payload
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
        RETURNING id
        "#,
    )
    .bind(event.workspace_id)
    .bind(event.actor_user_id)
    .bind(api_key_id)
    .bind(api_key_label)
    .bind(event.event_kind.as_str())
    .bind(event.object_id)
    .bind(event.object_version_id)
    .bind(event.object_edge_id)
    .bind(event.object_grant_id)
    .bind(event.comment_thread_id)
    .bind(event.comment_id)
    .bind(event.group_id)
    .bind(event.target_user_id)
    .bind(event.payload)
    .fetch_one(&mut **tx)
    .await?)
}

/// Lists global events.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot read the event projection.
pub async fn list_events(
    pool: &PgPool,
    actor_id: Uuid,
    query: ListEvents<'_>,
) -> Result<Vec<EventRow>> {
    Ok(sqlx::query_as::<_, EventRow>(
        r#"
        SELECT
            id, sequence_number, workspace_id, actor_user_id,
            (
                SELECT username
                FROM kival.users
                WHERE id = kival.events.actor_user_id
            ) AS actor_username,
            api_key_id, api_key_label, event_kind, object_id, object_version_id,
            object_edge_id, object_grant_id, comment_thread_id, comment_id, group_id,
            target_user_id, payload, created_at
        FROM kival.events
        WHERE sequence_number > COALESCE($1, 0)
            AND ($2::bigint IS NULL OR sequence_number < $2)
            AND ($3::text IS NULL OR event_kind = $3)
            AND ($4::uuid IS NULL OR actor_user_id = $4)
            AND ($5::uuid IS NULL OR target_user_id = $5)
            AND ($6::uuid IS NULL OR object_id = $6)
            AND ($7::uuid IS NULL OR group_id = $7)
        ORDER BY
            CASE WHEN $8 = 'desc' THEN sequence_number END DESC,
            CASE WHEN $8 = 'asc' THEN sequence_number END ASC
        LIMIT $9
        OFFSET CASE
            WHEN kival.require_capability(
                TRUE,
                EXISTS (
                    SELECT 1
                    FROM kival.global_admins
                    WHERE user_id = $10
                        AND revoked_at IS NULL
                )
            )
            THEN 0
            ELSE 0
        END
        "#,
    )
    .bind(query.after_sequence)
    .bind(query.before_sequence)
    .bind(query.event_kind)
    .bind(query.actor_user_id)
    .bind(query.target_user_id)
    .bind(query.object_id)
    .bind(query.group_id)
    .bind(query.order.as_str())
    .bind(query.limit)
    .bind(actor_id)
    .fetch_all(pool)
    .await?)
}

/// Lists events associated with a workspace.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot read the event projection.
pub async fn list_workspace_events(
    pool: &PgPool,
    workspace_id: Uuid,
    actor_id: Uuid,
    query: ListEvents<'_>,
) -> Result<Vec<EventRow>> {
    Ok(sqlx::query_as::<_, EventRow>(
        r#"
        SELECT
            id, sequence_number, workspace_id, actor_user_id,
            (
                SELECT username
                FROM kival.users
                WHERE id = kival.events.actor_user_id
            ) AS actor_username,
            api_key_id, api_key_label, event_kind, object_id, object_version_id,
            object_edge_id, object_grant_id, comment_thread_id, comment_id, group_id,
            target_user_id, payload, created_at
        FROM kival.events
        WHERE workspace_id = $1
            AND sequence_number > COALESCE($2, 0)
            AND ($3::bigint IS NULL OR sequence_number < $3)
            AND ($4::text IS NULL OR event_kind = $4)
            AND ($5::uuid IS NULL OR actor_user_id = $5)
            AND ($6::uuid IS NULL OR target_user_id = $6)
            AND ($7::uuid IS NULL OR object_id = $7)
            AND ($8::uuid IS NULL OR group_id = $8)
        ORDER BY
            CASE WHEN $9 = 'desc' THEN sequence_number END DESC,
            CASE WHEN $9 = 'asc' THEN sequence_number END ASC
        LIMIT $10
        OFFSET CASE
            WHEN kival.require_capability(
                EXISTS (
                    SELECT 1
                    FROM kival.workspaces
                    WHERE id = $1
                        AND archived_at IS NULL
                ),
                EXISTS (
                    SELECT 1
                    FROM kival.global_admins
                    WHERE user_id = $11
                        AND revoked_at IS NULL
                )
                OR EXISTS (
                    SELECT 1
                    FROM kival.workspace_memberships
                    WHERE workspace_id = $1
                        AND user_id = $11
                        AND workspace_role = 'admin'
                        AND revoked_at IS NULL
                )
            )
            THEN 0
            ELSE 0
        END
        "#,
    )
    .bind(workspace_id)
    .bind(query.after_sequence)
    .bind(query.before_sequence)
    .bind(query.event_kind)
    .bind(query.actor_user_id)
    .bind(query.target_user_id)
    .bind(query.object_id)
    .bind(query.group_id)
    .bind(query.order.as_str())
    .bind(query.limit)
    .bind(actor_id)
    .fetch_all(pool)
    .await?)
}

/// Lists events associated with an object.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot read the event projection.
pub async fn list_object_events(
    pool: &PgPool,
    workspace_id: Uuid,
    object_id: Uuid,
    actor_id: Uuid,
    query: ListEvents<'_>,
) -> Result<Vec<EventRow>> {
    Ok(sqlx::query_as::<_, EventRow>(
        r#"
        SELECT
            id, sequence_number, workspace_id, actor_user_id,
            (
                SELECT username
                FROM kival.users
                WHERE id = kival.events.actor_user_id
            ) AS actor_username,
            api_key_id, api_key_label, event_kind, object_id, object_version_id,
            object_edge_id, object_grant_id, comment_thread_id, comment_id, group_id,
            target_user_id, payload, created_at
        FROM kival.events
        WHERE workspace_id = $1
            AND object_id = $2
            AND kival.user_can_read_object($1, $2, $10)
            AND sequence_number > COALESCE($3, 0)
            AND ($4::bigint IS NULL OR sequence_number < $4)
            AND ($5::text IS NULL OR event_kind = $5)
            AND ($6::uuid IS NULL OR actor_user_id = $6)
            AND ($7::uuid IS NULL OR target_user_id = $7)
            AND ($8::uuid IS NULL OR object_id = $8)
            AND ($9::uuid IS NULL OR group_id = $9)
            AND (
                object_grant_id IS NULL
                OR kival.has_object_permission(
                    $1,
                    $2,
                    $10,
                    'admin'::kival.object_role
                )
            )
        ORDER BY
            CASE WHEN $11 = 'desc' THEN sequence_number END DESC,
            CASE WHEN $11 = 'asc' THEN sequence_number END ASC
        LIMIT $12
        OFFSET CASE WHEN kival.require_read_object($1, $2, $10) THEN 0 ELSE 0 END
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(query.after_sequence)
    .bind(query.before_sequence)
    .bind(query.event_kind)
    .bind(query.actor_user_id)
    .bind(query.target_user_id)
    .bind(query.object_id)
    .bind(query.group_id)
    .bind(actor_id)
    .bind(query.order.as_str())
    .bind(query.limit)
    .fetch_all(pool)
    .await?)
}
