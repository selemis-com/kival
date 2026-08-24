//! Notification preference, inbox, projection, and retention state bindings.

use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::Result;

/// Database row returned by bounded notification retention.
#[derive(Debug, Clone, Copy, sqlx::FromRow)]
pub struct NotificationRetentionBatch {
    /// Number of notification candidates deleted.
    pub candidates_deleted: i32,
    /// Number of inbox entries deleted.
    pub inbox_deleted: i32,
}

/// Database row returned by one notification projection batch.
#[derive(Debug, Clone, Copy, sqlx::FromRow)]
pub struct NotificationProjectionBatch {
    /// Number of notification candidates processed.
    pub candidates_processed: i32,
    /// Number of inbox notifications changed.
    pub notifications_changed: i32,
    /// Remaining unprojected notification-candidate lag.
    pub remaining_candidate_lag: i64,
}

/// Current projection of one visible inbox entry.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InboxEntryRow {
    /// Row identifier.
    pub id: Uuid,
    /// Monotonic inbox sequence number.
    pub sequence_number: i64,
    /// Recipient user identifier.
    pub recipient_user_id: Uuid,
    /// Workspace identifier.
    pub workspace_id: Uuid,
    /// Workspace name projected into the inbox entry.
    pub workspace_name: String,
    /// Object identifier.
    pub object_id: Option<Uuid>,
    /// Object title projected into the inbox entry.
    pub object_title: Option<String>,
    /// Event that originally created the inbox entry.
    pub source_event_id: Uuid,
    /// Most recent event folded into the inbox entry.
    pub latest_event_id: Uuid,
    /// User responsible for the latest event, when attributable.
    pub actor_user_id: Option<Uuid>,
    /// Username responsible for the latest event, when attributable.
    pub actor_username: Option<String>,
    /// Notification type.
    pub notification_type: String,
    /// Reason the notification was emitted.
    pub reason: String,
    /// Number of events folded into the inbox entry.
    pub event_count: i32,
    /// Comment-thread identifier.
    pub thread_id: Option<Uuid>,
    /// Comment identifier.
    pub comment_id: Option<Uuid>,
    /// Optional comment excerpt projected into the inbox entry.
    pub comment_excerpt: Option<String>,
    /// Timestamp at which the inbox entry was marked read.
    pub read_at: Option<OffsetDateTime>,
    /// Creation timestamp.
    pub created_at: OffsetDateTime,
    /// Last update timestamp.
    pub updated_at: OffsetDateTime,
}

/// Loads a user's notification preference for an object.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn object_notification_preference(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<(Option<bool>, Option<OffsetDateTime>)> {
    Ok(sqlx::query_as(
        r#"
        SELECT preference.ordinary_notifications_enabled, preference.updated_at
        FROM (SELECT 1) singleton
        LEFT JOIN kival.object_notification_preferences preference
            ON preference.user_id = $1
            AND preference.workspace_id = $2
            AND preference.object_id = $3
        OFFSET CASE WHEN kival.require_read_object($2, $3, $1) THEN 0 ELSE 0 END
        "#,
    )
    .bind(user_id)
    .bind(workspace_id)
    .bind(object_id)
    .fetch_one(pool)
    .await?)
}

/// Creates or updates a user's notification preference for an object.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn set_object_notification_preference(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    enabled: bool,
) -> Result<OffsetDateTime> {
    Ok(sqlx::query_scalar(
        r#"
        INSERT INTO kival.object_notification_preferences (
            user_id, workspace_id, object_id, ordinary_notifications_enabled, updated_by
        )
        VALUES ($1, $2, $3, $4, $1)
        ON CONFLICT (user_id, object_id) DO UPDATE
        SET ordinary_notifications_enabled = EXCLUDED.ordinary_notifications_enabled,
            updated_by = EXCLUDED.updated_by
        RETURNING updated_at
        "#,
    )
    .bind(user_id)
    .bind(workspace_id)
    .bind(object_id)
    .bind(enabled)
    .fetch_one(&mut **tx)
    .await?)
}

/// Lists inbox entries visible to a recipient.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn list_inbox_entries(
    pool: &PgPool,
    recipient_user_id: Uuid,
    before_sequence: Option<i64>,
    unread_only: bool,
    workspace_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<InboxEntryRow>> {
    Ok(sqlx::query_as(
        r#"
        SELECT
            inbox.id,
            inbox.sequence_number,
            inbox.recipient_user_id,
            inbox.workspace_id,
            workspace.name AS workspace_name,
            inbox.object_id,
            object_current_version.title AS object_title,
            inbox.source_event_id,
            inbox.latest_event_id,
            inbox.actor_user_id,
            actor.username AS actor_username,
            inbox.notification_type,
            inbox.reason,
            inbox.event_count,
            inbox.thread_id,
            inbox.comment_id,
            left(comment.body, 500) AS comment_excerpt,
            inbox.read_at,
            inbox.created_at,
            inbox.updated_at
        FROM kival.inbox_notifications inbox
        JOIN kival.workspaces workspace
            ON workspace.id = inbox.workspace_id
            AND workspace.archived_at IS NULL
        LEFT JOIN kival.objects object
            ON object.workspace_id = inbox.workspace_id
            AND object.id = inbox.object_id
            AND object.archived_at IS NULL
        LEFT JOIN kival.object_versions object_current_version
            ON object_current_version.object_id = object.id
            AND object_current_version.id = object.current_version_id
        LEFT JOIN kival.users actor
            ON actor.id = inbox.actor_user_id
            AND actor.disabled_at IS NULL
        LEFT JOIN kival.comments comment
            ON comment.workspace_id = inbox.workspace_id
            AND comment.object_id = inbox.object_id
            AND comment.id = inbox.comment_id
        WHERE inbox.recipient_user_id = $1
            AND inbox.archived_at IS NULL
            AND inbox.expires_at > now()
            AND ($2::bigint IS NULL OR inbox.sequence_number < $2)
            AND (NOT $3 OR inbox.read_at IS NULL)
            AND ($4::uuid IS NULL OR inbox.workspace_id = $4)
            AND kival.inbox_notification_is_visible(
                $1, inbox.workspace_id, inbox.object_id, inbox.reason
              )
        ORDER BY inbox.sequence_number DESC
        LIMIT $5
        "#,
    )
    .bind(recipient_user_id)
    .bind(before_sequence)
    .bind(unread_only)
    .bind(workspace_id)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

/// Counts unread inbox entries for a recipient.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn inbox_unread_count(pool: &PgPool, recipient_user_id: Uuid) -> Result<i64> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT count(*)
        FROM kival.inbox_notifications inbox
        WHERE inbox.recipient_user_id = $1
            AND inbox.archived_at IS NULL
            AND inbox.read_at IS NULL
            AND inbox.expires_at > now()
            AND kival.inbox_notification_is_visible(
                $1, inbox.workspace_id, inbox.object_id, inbox.reason
              )
        "#,
    )
    .bind(recipient_user_id)
    .fetch_one(pool)
    .await?)
}

/// Marks one inbox entry read or unread.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn update_inbox_entry_read_state(
    tx: &mut Transaction<'_, Postgres>,
    inbox_entry_id: Uuid,
    recipient_user_id: Uuid,
    read: bool,
) -> Result<Option<InboxEntryRow>> {
    let query = r#"
        WITH updated AS (
            UPDATE kival.inbox_notifications inbox
            SET read_at = CASE
                    WHEN $3 THEN COALESCE(inbox.read_at, now())
                    ELSE NULL
                END
            WHERE inbox.id = $1
                AND inbox.recipient_user_id = $2
                AND inbox.archived_at IS NULL
                AND inbox.expires_at > now()
                AND kival.inbox_notification_is_visible(
                    $2, inbox.workspace_id, inbox.object_id, inbox.reason
                  )
            RETURNING inbox.*
        )
        SELECT
            updated.id,
            updated.sequence_number,
            updated.recipient_user_id,
            updated.workspace_id,
            workspace.name AS workspace_name,
            updated.object_id,
            object_current_version.title AS object_title,
            updated.source_event_id,
            updated.latest_event_id,
            updated.actor_user_id,
            actor.username AS actor_username,
            updated.notification_type,
            updated.reason,
            updated.event_count,
            updated.thread_id,
            updated.comment_id,
            left(comment.body, 500) AS comment_excerpt,
            updated.read_at,
            updated.created_at,
            updated.updated_at
        FROM updated
        JOIN kival.workspaces workspace
            ON workspace.id = updated.workspace_id
        LEFT JOIN kival.objects object
            ON object.workspace_id = updated.workspace_id
            AND object.id = updated.object_id
        LEFT JOIN kival.object_versions object_current_version
            ON object_current_version.object_id = object.id
            AND object_current_version.id = object.current_version_id
        LEFT JOIN kival.users actor
            ON actor.id = updated.actor_user_id
            AND actor.disabled_at IS NULL
        LEFT JOIN kival.comments comment
            ON comment.workspace_id = updated.workspace_id
            AND comment.object_id = updated.object_id
            AND comment.id = updated.comment_id
        "#;
    Ok(sqlx::query_as(query)
        .bind(inbox_entry_id)
        .bind(recipient_user_id)
        .bind(read)
        .fetch_optional(&mut **tx)
        .await?)
}

/// Marks matching inbox entries read through an optional sequence.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn mark_inbox_read(
    tx: &mut Transaction<'_, Postgres>,
    recipient_user_id: Uuid,
    workspace_id: Option<Uuid>,
    through_sequence: Option<i64>,
) -> Result<i64> {
    Ok(sqlx::query_scalar(
        r#"
        WITH changed AS (
            UPDATE kival.inbox_notifications inbox
            SET read_at = now()
            WHERE inbox.recipient_user_id = $1
                AND inbox.archived_at IS NULL
                AND inbox.read_at IS NULL
                AND inbox.expires_at > now()
                AND ($2::uuid IS NULL OR inbox.workspace_id = $2)
                AND ($3::bigint IS NULL OR inbox.sequence_number <= $3)
                AND kival.inbox_notification_is_visible(
                    $1, inbox.workspace_id, inbox.object_id, inbox.reason
                  )
            RETURNING 1
        )
        SELECT count(*)
        FROM changed
        "#,
    )
    .bind(recipient_user_id)
    .bind(workspace_id)
    .bind(through_sequence)
    .fetch_one(&mut **tx)
    .await?)
}

/// Publishes a transactional inbox-update notification.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn publish_inbox_updated(
    tx: &mut Transaction<'_, Postgres>,
    recipient_user_id: Uuid,
    workspace_id: Uuid,
    object_id: Option<Uuid>,
    event_id: Uuid,
    inbox_entry_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        SELECT kival.publish_realtime_invalidation(
            $1, 'inbox.updated', $2, $3, $4, $5
        )
        "#,
    )
    .bind(recipient_user_id)
    .bind(workspace_id)
    .bind(object_id)
    .bind(event_id)
    .bind(inbox_entry_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Publishes a transactional inbox-update notification for a recipient.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn publish_inbox_updated_for_user(
    tx: &mut Transaction<'_, Postgres>,
    recipient_user_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        SELECT kival.publish_realtime_invalidation($1, 'inbox.updated')
        "#,
    )
    .bind(recipient_user_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Applies one bounded notification-retention batch.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn apply_notification_retention(
    pool: &PgPool,
    batch_size: i32,
) -> Result<NotificationRetentionBatch> {
    Ok(sqlx::query_as(
        r#"
        SELECT *
        FROM kival.apply_notification_retention($1)
        "#,
    )
    .bind(batch_size)
    .fetch_one(pool)
    .await?)
}

// These bindings intentionally return `sqlx::Error`: Steda's task error surface already
// accepts database failures directly, while the state operation still remains kernel-owned.
/// Returns whether notification candidates exist for an event.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn notification_candidates_exist_for_event(
    tx: &mut Transaction<'_, Postgres>,
    event_id: Uuid,
) -> std::result::Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM kival.notification_candidates
            WHERE event_id = $1
                AND projected_at IS NULL
        )
        "#,
    )
    .bind(event_id)
    .fetch_one(&mut **tx)
    .await
}

/// Returns whether any notification candidates remain pending.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn pending_notification_candidates_exist(
    pool: &PgPool,
) -> std::result::Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM kival.notification_candidates
            WHERE projected_at IS NULL
        )
        "#,
    )
    .fetch_one(pool)
    .await
}

/// Processes one bounded notification-projection batch.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn process_notification_projection_batch(
    pool: &PgPool,
    batch_size: i32,
) -> std::result::Result<NotificationProjectionBatch, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT *
        FROM kival.process_notification_candidate_batch($1)
        "#,
    )
    .bind(batch_size)
    .fetch_one(pool)
    .await
}
