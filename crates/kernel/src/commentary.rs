//! Object commentary state bindings.

use chrono::{DateTime, Utc};
use kival_types::CommentStatus;
use sqlx::{Acquire, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::Result;

/// Stored comment-thread projection.
#[derive(Debug, Clone, Copy, sqlx::FromRow)]
pub struct ThreadRow {
    /// Row identifier.
    pub id: Uuid,
    /// Workspace identifier.
    pub workspace_id: Uuid,
    /// Object identifier.
    pub object_id: Uuid,
    /// User that created the row, when retained.
    pub created_by: Uuid,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Resolution timestamp, when resolved.
    pub resolved_at: Option<DateTime<Utc>>,
    /// User that resolved the thread, when resolved.
    pub resolved_by: Option<Uuid>,
    /// Timestamp after which retention may remove the row.
    pub retention_expires_at: Option<DateTime<Utc>>,
}

/// Stored comment projection with author identity.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CommentRow {
    /// Row identifier.
    pub id: Uuid,
    /// Workspace identifier.
    pub workspace_id: Uuid,
    /// Object identifier.
    pub object_id: Uuid,
    /// Comment-thread identifier.
    pub thread_id: Uuid,
    /// Parent comment identifier for a reply.
    pub parent_comment_id: Option<Uuid>,
    /// Comment author identifier.
    pub author_user_id: Uuid,
    /// Comment author username.
    pub author_username: String,
    /// Comment author display name.
    pub author_display_name: String,
    /// Comment body, absent after deletion or expiry.
    pub body: Option<String>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Last explicit edit timestamp.
    pub edited_at: Option<DateTime<Utc>>,
    /// Deletion timestamp, when deleted.
    pub deleted_at: Option<DateTime<Utc>>,
    /// User that deleted the comment, when retained.
    pub deleted_by: Option<Uuid>,
    /// Retention-expiry timestamp, when expired.
    pub expired_at: Option<DateTime<Utc>>,
    /// Timestamp after which retention may remove the row.
    pub retention_expires_at: Option<DateTime<Utc>>,
}

impl CommentRow {
    /// Returns the effective lifecycle status derived from deletion and retention state.
    #[must_use]
    pub const fn status(&self) -> CommentStatus {
        if self.deleted_at.is_some() {
            CommentStatus::Deleted
        } else if self.expired_at.is_some() {
            CommentStatus::Expired
        } else {
            CommentStatus::Active
        }
    }
}

/// Stored mention projection for a comment.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MentionRow {
    /// Comment identifier.
    pub comment_id: Uuid,
    /// User identifier.
    pub user_id: Uuid,
    /// Username associated with the user.
    pub username: String,
    /// Display name associated with the user.
    pub display_name: String,
}

/// Actor-aware cursor window for reading one commentary thread.
#[derive(Debug, Clone, Copy)]
pub struct CommentPageQuery {
    /// Workspace containing the object.
    pub workspace_id: Uuid,
    /// Object containing the commentary thread.
    pub object_id: Uuid,
    /// User whose current object access authorizes the read.
    pub actor_id: Uuid,
    /// Commentary thread being paginated.
    pub thread_id: Uuid,
    /// Optional lower-exclusive creation timestamp cursor.
    pub cursor_created_at: Option<DateTime<Utc>>,
    /// Optional lower-exclusive row identifier cursor.
    pub cursor_id: Option<Uuid>,
    /// Maximum rows returned by `PostgreSQL`.
    pub limit: i64,
}

/// Lists non-expired comment threads for an object.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn list_comment_threads(
    pool: &PgPool,
    workspace_id: Uuid,
    object_id: Uuid,
    actor_id: Uuid,
    cursor_updated_at: Option<DateTime<Utc>>,
    cursor_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<ThreadRow>> {
    Ok(sqlx::query_as(
        r#"
        SELECT id, workspace_id, object_id, created_by, created_at, updated_at,
               resolved_at, resolved_by, retention_expires_at
        FROM kival.comment_threads
        WHERE workspace_id = $1
            AND object_id = $2
            AND kival.user_can_read_object($1, $2, $3)
            AND (retention_expires_at IS NULL OR retention_expires_at > now())
            AND ($4::timestamptz IS NULL OR (updated_at, id) < ($4, $5))
        ORDER BY updated_at DESC, id DESC
        LIMIT $6
        OFFSET CASE WHEN kival.require_read_object($1, $2, $3) THEN 0 ELSE 0 END
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(actor_id)
    .bind(cursor_updated_at)
    .bind(cursor_id)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

/// Lists users who may be mentioned on an object.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn mention_candidates(
    pool: &PgPool,
    workspace_id: Uuid,
    object_id: Uuid,
    actor_id: Uuid,
    needle: &str,
    limit: i64,
) -> Result<Vec<(Uuid, String, String)>> {
    Ok(sqlx::query_as(
        r#"
        SELECT u.id, u.username, u.display_name
        FROM kival.users u
        WHERE u.status = 'active'
            AND kival.user_can_access_active_object(
                $1, $2, $3, 'viewer'::kival.object_role
            )
            AND kival.has_object_permission($1, $2, u.id, 'viewer'::kival.object_role)
            AND (
                $4 = ''
                OR strpos(u.username_normalized, $4) = 1
                OR strpos(lower(u.display_name), $4) > 0
            )
        ORDER BY CASE
            WHEN u.username_normalized = $4 THEN 0
            WHEN strpos(u.username_normalized, $4) = 1 THEN 1
            ELSE 2
        END,
            u.username_normalized,
            u.id
        LIMIT $5
        OFFSET CASE
            WHEN kival.require_access_active_object(
                $1, $2, $3, 'viewer'::kival.object_role
            ) THEN 0
            ELSE 0
        END
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(actor_id)
    .bind(needle)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

/// Pins the active parent object for a commentary transition.
async fn require_active_commentary_object(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<()> {
    if !crate::objects::lock_active_objects_for_reference(tx, workspace_id, &[object_id]).await? {
        return Err(crate::KernelError::ResourceNotFound);
    }
    Ok(())
}

/// Creates a comment thread for an object.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn create_comment_thread(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    created_by: Uuid,
) -> Result<Uuid> {
    require_active_commentary_object(tx, workspace_id, object_id).await?;
    Ok(sqlx::query_scalar(
        r#"
        INSERT INTO kival.comment_threads (workspace_id, object_id, created_by)
        VALUES ($1, $2, $3)
        RETURNING id
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(created_by)
    .fetch_one(&mut **tx)
    .await?)
}

/// Locks a live comment thread before adding a reply.
///
/// Replies update the thread activity timestamp, so the thread is write-locked immediately rather
/// than first taking a shared lock and later upgrading it. Concurrent replies therefore
/// serialize on the thread row without a lock-upgrade cycle. Root identity and author state are
/// read without a separate comment lock because commentary mutations already serialize on the
/// parent thread.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn lock_thread_for_reply(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    thread_id: Uuid,
) -> Result<Option<(Uuid, Uuid, Option<DateTime<Utc>>)>> {
    require_active_commentary_object(tx, workspace_id, object_id).await?;
    Ok(sqlx::query_as(
        r#"
        SELECT c.id, c.author_user_id, t.resolved_at
        FROM kival.comment_threads t
        JOIN kival.comments c
            ON c.thread_id = t.id
            AND c.parent_comment_id IS NULL
        WHERE t.workspace_id = $1
            AND t.object_id = $2
            AND t.id = $3
            AND (t.retention_expires_at IS NULL OR t.retention_expires_at > now())
        FOR UPDATE OF t
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(thread_id)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Updates a comment body and records the edit time.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn update_comment_body(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    comment_id: Uuid,
    body: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE kival.comments
        SET body = $4,
            edited_at = now(),
            updated_at = now()
        WHERE workspace_id = $1
            AND object_id = $2
            AND id = $3
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(comment_id)
    .bind(body)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Soft-deletes a comment and records the deleting user.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn delete_comment(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    comment_id: Uuid,
    deleted_by: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        WITH deleted_comment AS (
            UPDATE kival.comments
            SET body = NULL,
                deleted_at = now(),
                deleted_by = $4,
                updated_at = now()
            WHERE workspace_id = $1
                AND object_id = $2
                AND id = $3
            RETURNING workspace_id, object_id, id
        )
        DELETE FROM kival.comment_mentions mention
        USING deleted_comment deleted
        WHERE mention.workspace_id = deleted.workspace_id
            AND mention.object_id = deleted.object_id
            AND mention.comment_id = deleted.id
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(comment_id)
    .bind(deleted_by)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Locks a live thread before changing its resolution state.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn lock_thread_resolution(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    thread_id: Uuid,
) -> Result<Option<(Uuid, Option<DateTime<Utc>>)>> {
    require_active_commentary_object(tx, workspace_id, object_id).await?;
    Ok(sqlx::query_as(
        r#"
        SELECT c.author_user_id, t.resolved_at
        FROM kival.comment_threads t
        JOIN kival.comments c
            ON c.thread_id = t.id
            AND c.parent_comment_id IS NULL
        WHERE t.workspace_id = $1
            AND t.object_id = $2
            AND t.id = $3
            AND (t.retention_expires_at IS NULL OR t.retention_expires_at > now())
        FOR UPDATE OF t
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(thread_id)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Sets or clears a thread resolution.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn set_thread_resolved(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    thread_id: Uuid,
    actor_id: Uuid,
    resolved: bool,
) -> Result<()> {
    if resolved {
        sqlx::query(
            r#"
            UPDATE kival.comment_threads
            SET resolved_at = now(),
                resolved_by = $4,
                updated_at = now()
            WHERE workspace_id = $1
                AND object_id = $2
                AND id = $3
            "#,
        )
        .bind(workspace_id)
        .bind(object_id)
        .bind(thread_id)
        .bind(actor_id)
        .execute(&mut **tx)
        .await?;
    } else {
        sqlx::query(
            r#"
            UPDATE kival.comment_threads
            SET resolved_at = NULL,
                resolved_by = NULL,
                updated_at = now()
            WHERE workspace_id = $1
                AND object_id = $2
                AND id = $3
            "#,
        )
        .bind(workspace_id)
        .bind(object_id)
        .bind(thread_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Resolves normalized usernames to active workspace users.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn resolve_mentioned_usernames(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    usernames: &[String],
    object_id: Uuid,
) -> Result<Vec<(Uuid, String)>> {
    Ok(sqlx::query_as(
        r#"
        SELECT u.id, u.username_normalized
        FROM kival.users u
        WHERE u.username_normalized = ANY($2)
            AND u.status = 'active'
            AND kival.has_object_permission($1, $3, u.id, 'viewer'::kival.object_role)
        "#,
    )
    .bind(workspace_id)
    .bind(usernames)
    .bind(object_id)
    .fetch_all(&mut **tx)
    .await?)
}

/// Filters mentioned users to those allowed to view the object.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn allowed_mentioned_user_ids(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    user_ids: &[Uuid],
    object_id: Uuid,
) -> Result<Vec<Uuid>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT u.id
        FROM kival.users u
        WHERE u.id = ANY($2)
            AND u.status = 'active'
            AND kival.has_object_permission($1, $3, u.id, 'viewer'::kival.object_role)
        "#,
    )
    .bind(workspace_id)
    .bind(user_ids)
    .bind(object_id)
    .fetch_all(&mut **tx)
    .await?)
}

/// Creates a comment within a thread.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn create_comment(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    thread_id: Uuid,
    parent_comment_id: Option<Uuid>,
    author_user_id: Uuid,
    body: &str,
) -> Result<Uuid> {
    Ok(sqlx::query_scalar(
        r#"
        INSERT INTO kival.comments (
            workspace_id, object_id, thread_id, parent_comment_id, author_user_id, body
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(thread_id)
    .bind(parent_comment_id)
    .bind(author_user_id)
    .bind(body)
    .fetch_one(&mut **tx)
    .await?)
}

/// Lists user IDs currently mentioned by a comment.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn comment_mention_ids_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    comment_id: Uuid,
) -> Result<Vec<Uuid>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT mentioned_user_id
        FROM kival.comment_mentions
        WHERE comment_id = $1
        "#,
    )
    .bind(comment_id)
    .fetch_all(&mut **tx)
    .await?)
}

/// Requires a comment to belong to the supplied workspace and object.
async fn require_comment_scope(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    comment_id: Uuid,
) -> Result<()> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM kival.comments
        WHERE workspace_id = $1
            AND object_id = $2
            AND id = $3
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(comment_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(())
}

/// Deletes all mention rows for one scoped comment.
async fn delete_comment_mentions(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    comment_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        DELETE FROM kival.comment_mentions
        WHERE workspace_id = $1
            AND object_id = $2
            AND comment_id = $3
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(comment_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Replaces the mention rows associated with a comment.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn replace_comment_mentions(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    comment_id: Uuid,
    mentions: &[Uuid],
) -> Result<()> {
    let mut savepoint = (&mut **tx).begin().await?;
    let result = replace_comment_mentions_in_savepoint(
        &mut savepoint,
        workspace_id,
        object_id,
        comment_id,
        mentions,
    )
    .await;

    match result {
        Ok(()) => {
            savepoint.commit().await?;
            Ok(())
        }
        Err(error) => {
            savepoint.rollback().await?;
            Err(error)
        }
    }
}

/// Applies mention replacement inside a cancellation-safe savepoint.
async fn replace_comment_mentions_in_savepoint(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    comment_id: Uuid,
    mentions: &[Uuid],
) -> Result<()> {
    require_comment_scope(tx, workspace_id, object_id, comment_id).await?;
    delete_comment_mentions(tx, workspace_id, object_id, comment_id).await?;
    if !mentions.is_empty() {
        sqlx::query(
            r#"
            INSERT INTO kival.comment_mentions (
                workspace_id, object_id, comment_id, mentioned_user_id
            )
            SELECT $1, $2, $3, unnest($4::uuid[])
            "#,
        )
        .bind(workspace_id)
        .bind(object_id)
        .bind(comment_id)
        .bind(mentions)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Updates the activity timestamp of a comment thread.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn touch_comment_thread(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE kival.comment_threads
        SET updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(thread_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Comment state locked while applying a commentary transition.
pub type LockedComment =
    (Uuid, Uuid, Option<DateTime<Utc>>, Option<DateTime<Utc>>, Option<DateTime<Utc>>);

/// Locks a live comment and returns the state required for mutation.
///
/// Commentary mutations use one explicit order: parent object lifecycle, then thread, then comment.
/// The thread is locked first because edits and deletions later update its activity timestamp. This
/// avoids relying on `PostgreSQL`'s internal row-mark order for a multi-relation `FOR UPDATE`.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn lock_comment(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    comment_id: Uuid,
) -> Result<Option<LockedComment>> {
    require_active_commentary_object(tx, workspace_id, object_id).await?;

    let thread = sqlx::query_as::<_, (Uuid, Option<DateTime<Utc>>)>(
        r#"
        SELECT c.thread_id, t.resolved_at
        FROM kival.comments c
        JOIN kival.comment_threads t
            ON t.workspace_id = c.workspace_id
            AND t.object_id = c.object_id
            AND t.id = c.thread_id
        WHERE c.workspace_id = $1
            AND c.object_id = $2
            AND c.id = $3
            AND (t.retention_expires_at IS NULL OR t.retention_expires_at > now())
        FOR UPDATE OF t
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(comment_id)
    .fetch_optional(&mut **tx)
    .await?;

    let Some((thread_id, resolved_at)) = thread else {
        return Ok(None);
    };

    let comment = sqlx::query_as::<_, (Uuid, Option<DateTime<Utc>>, Option<DateTime<Utc>>)>(
        r#"
        SELECT
            c.author_user_id,
            c.deleted_at,
            CASE
                WHEN c.deleted_at IS NULL
                    AND c.expired_at IS NULL
                    AND c.retention_expires_at <= now()
                THEN c.retention_expires_at
                ELSE c.expired_at
            END AS expired_at
        FROM kival.comments c
        WHERE c.workspace_id = $1
            AND c.object_id = $2
            AND c.id = $3
            AND c.thread_id = $4
        FOR UPDATE
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(comment_id)
    .bind(thread_id)
    .fetch_optional(&mut **tx)
    .await?;

    Ok(comment.map(|(author_user_id, deleted_at, expired_at)| {
        (thread_id, author_user_id, deleted_at, expired_at, resolved_at)
    }))
}

/// Loads one non-expired comment thread.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn fetch_comment_thread(
    pool: &PgPool,
    workspace_id: Uuid,
    object_id: Uuid,
    actor_id: Uuid,
    thread_id: Uuid,
) -> Result<Option<ThreadRow>> {
    Ok(sqlx::query_as(
        r#"
        SELECT id, workspace_id, object_id, created_by, created_at, updated_at,
               resolved_at, resolved_by, retention_expires_at
        FROM kival.comment_threads
        WHERE workspace_id = $1
            AND object_id = $2
            AND kival.user_can_read_object($1, $2, $3)
            AND id = $4
            AND (retention_expires_at IS NULL OR retention_expires_at > now())
        OFFSET CASE WHEN kival.require_read_object($1, $2, $3) THEN 0 ELSE 0 END
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(actor_id)
    .bind(thread_id)
    .fetch_optional(pool)
    .await?)
}

/// Loads the initial bounded comment set for multiple threads.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn fetch_initial_comment_rows(
    pool: &PgPool,
    workspace_id: Uuid,
    object_id: Uuid,
    actor_id: Uuid,
    thread_ids: &[Uuid],
    limit_per_thread: i64,
) -> Result<Vec<CommentRow>> {
    Ok(sqlx::query_as(
        r#"
        WITH ranked AS (
            SELECT
                c.id, c.workspace_id, c.object_id, c.thread_id, c.parent_comment_id,
                c.author_user_id, u.username AS author_username,
                u.display_name AS author_display_name,
                CASE
                    WHEN c.deleted_at IS NULL
                        AND c.expired_at IS NULL
                        AND c.retention_expires_at <= now()
                    THEN NULL
                    ELSE c.body
                END AS body,
                c.created_at, c.updated_at,
                CASE
                    WHEN c.deleted_at IS NULL
                        AND c.expired_at IS NULL
                        AND c.retention_expires_at <= now()
                    THEN NULL
                    ELSE c.edited_at
                END AS edited_at,
                c.deleted_at, c.deleted_by,
                CASE
                    WHEN c.deleted_at IS NULL
                        AND c.expired_at IS NULL
                        AND c.retention_expires_at <= now()
                    THEN c.retention_expires_at
                    ELSE c.expired_at
                END AS expired_at,
                c.retention_expires_at,
                row_number() OVER (
                    PARTITION BY c.thread_id ORDER BY c.created_at, c.id
                ) AS row_number
            FROM kival.comments c
            JOIN kival.comment_threads t
                ON t.workspace_id = c.workspace_id
                AND t.object_id = c.object_id
                AND t.id = c.thread_id
            JOIN kival.users u
                ON u.id = c.author_user_id
            WHERE c.workspace_id = $1
                AND c.object_id = $2
                AND kival.user_can_read_object($1, $2, $3)
                AND c.thread_id = ANY($4)
                AND (t.retention_expires_at IS NULL OR t.retention_expires_at > now())
        )
        SELECT
            id, workspace_id, object_id, thread_id, parent_comment_id, author_user_id,
            author_username, author_display_name, body, created_at, updated_at, edited_at,
            deleted_at, deleted_by, expired_at, retention_expires_at
        FROM ranked
        WHERE row_number <= $5
        ORDER BY thread_id, created_at, id
        OFFSET CASE WHEN kival.require_read_object($1, $2, $3) THEN 0 ELSE 0 END
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(actor_id)
    .bind(thread_ids)
    .bind(limit_per_thread)
    .fetch_all(pool)
    .await?)
}

/// Returns whether a non-expired comment thread exists.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn comment_thread_exists(
    pool: &PgPool,
    workspace_id: Uuid,
    object_id: Uuid,
    actor_id: Uuid,
    thread_id: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT kival.require_read_object($1, $2, $3) AND EXISTS (
            SELECT 1
            FROM kival.comment_threads
            WHERE workspace_id = $1
                AND object_id = $2
                AND kival.user_can_read_object($1, $2, $3)
                AND id = $4
                AND (retention_expires_at IS NULL OR retention_expires_at > now())
        )
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(actor_id)
    .bind(thread_id)
    .fetch_one(pool)
    .await?)
}

/// Loads one cursor-paginated page of comments for a thread.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn fetch_thread_comment_page_rows(
    pool: &PgPool,
    query: CommentPageQuery,
) -> Result<Vec<CommentRow>> {
    Ok(sqlx::query_as(
        r#"
        SELECT
            c.id, c.workspace_id, c.object_id, c.thread_id, c.parent_comment_id,
            c.author_user_id, u.username AS author_username,
            u.display_name AS author_display_name,
            CASE
                WHEN c.deleted_at IS NULL
                 AND c.expired_at IS NULL
                 AND c.retention_expires_at <= now()
                THEN NULL
                ELSE c.body
            END AS body,
            c.created_at, c.updated_at,
            CASE
                WHEN c.deleted_at IS NULL
                 AND c.expired_at IS NULL
                 AND c.retention_expires_at <= now()
                THEN NULL
                ELSE c.edited_at
            END AS edited_at,
            c.deleted_at, c.deleted_by,
            CASE
                WHEN c.deleted_at IS NULL
                 AND c.expired_at IS NULL
                 AND c.retention_expires_at <= now()
                THEN c.retention_expires_at
                ELSE c.expired_at
            END AS expired_at,
            c.retention_expires_at
        FROM kival.comments c
        JOIN kival.comment_threads t
            ON t.workspace_id = c.workspace_id
            AND t.object_id = c.object_id
            AND t.id = c.thread_id
        JOIN kival.users u
            ON u.id = c.author_user_id
        WHERE c.workspace_id = $1
            AND c.object_id = $2
            AND kival.user_can_read_object($1, $2, $3)
            AND c.thread_id = $4
            AND (t.retention_expires_at IS NULL OR t.retention_expires_at > now())
            AND ($5::timestamptz IS NULL OR (c.created_at, c.id) > ($5, $6))
        ORDER BY c.created_at, c.id
        LIMIT $7
        OFFSET CASE WHEN kival.require_read_object($1, $2, $3) THEN 0 ELSE 0 END
        "#,
    )
    .bind(query.workspace_id)
    .bind(query.object_id)
    .bind(query.actor_id)
    .bind(query.thread_id)
    .bind(query.cursor_created_at)
    .bind(query.cursor_id)
    .bind(query.limit)
    .fetch_all(pool)
    .await?)
}

/// Loads comments for selected threads or one selected comment.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn fetch_comment_rows(
    pool: &PgPool,
    workspace_id: Uuid,
    object_id: Uuid,
    actor_id: Uuid,
    thread_ids: Option<&[Uuid]>,
    comment_id: Option<Uuid>,
) -> Result<Vec<CommentRow>> {
    let thread_ids = thread_ids.map(<[Uuid]>::to_vec);
    Ok(sqlx::query_as(
        r#"
        SELECT
            c.id, c.workspace_id, c.object_id, c.thread_id, c.parent_comment_id,
            c.author_user_id, u.username AS author_username,
            u.display_name AS author_display_name,
            CASE
                WHEN c.deleted_at IS NULL
                 AND c.expired_at IS NULL
                 AND c.retention_expires_at <= now()
                THEN NULL
                ELSE c.body
            END AS body,
            c.created_at, c.updated_at,
            CASE
                WHEN c.deleted_at IS NULL
                 AND c.expired_at IS NULL
                 AND c.retention_expires_at <= now()
                THEN NULL
                ELSE c.edited_at
            END AS edited_at,
            c.deleted_at, c.deleted_by,
            CASE
                WHEN c.deleted_at IS NULL
                 AND c.expired_at IS NULL
                 AND c.retention_expires_at <= now()
                THEN c.retention_expires_at
                ELSE c.expired_at
            END AS expired_at,
            c.retention_expires_at
        FROM kival.comments c
        JOIN kival.comment_threads t
            ON t.workspace_id = c.workspace_id
            AND t.object_id = c.object_id
            AND t.id = c.thread_id
        JOIN kival.users u
            ON u.id = c.author_user_id
        WHERE c.workspace_id = $1
            AND c.object_id = $2
            AND kival.user_can_read_object($1, $2, $3)
            AND (t.retention_expires_at IS NULL OR t.retention_expires_at > now())
            AND ($4::uuid[] IS NULL OR c.thread_id = ANY($4))
            AND ($5::uuid IS NULL OR c.id = $5)
        ORDER BY c.created_at, c.id
        OFFSET CASE WHEN kival.require_read_object($1, $2, $3) THEN 0 ELSE 0 END
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(actor_id)
    .bind(thread_ids)
    .bind(comment_id)
    .fetch_all(pool)
    .await?)
}

/// Loads mention projections for supplied comments while the actor can read their object.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn fetch_comment_mentions(
    pool: &PgPool,
    actor_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    comment_ids: &[Uuid],
) -> Result<Vec<MentionRow>> {
    if comment_ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(sqlx::query_as(
        r#"
        SELECT m.comment_id, u.id AS user_id, u.username, u.display_name
        FROM kival.comment_mentions m
        JOIN kival.comments c
            ON c.id = m.comment_id
        JOIN kival.users u
            ON u.id = m.mentioned_user_id
        WHERE m.workspace_id = $2
            AND m.object_id = $3
            AND m.comment_id = ANY($4)
            AND c.workspace_id = $2
            AND c.object_id = $3
            AND c.deleted_at IS NULL
            AND c.expired_at IS NULL
            AND (c.retention_expires_at IS NULL OR c.retention_expires_at > now())
            AND kival.user_can_read_object($2, $3, $1)
        ORDER BY u.username_normalized, u.id
        OFFSET CASE WHEN kival.require_read_object($2, $3, $1) THEN 0 ELSE 0 END
        "#,
    )
    .bind(actor_id)
    .bind(workspace_id)
    .bind(object_id)
    .bind(comment_ids)
    .fetch_all(pool)
    .await?)
}

/// Loads one thread for an already-admitted commentary mutation.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot load the projection.
pub async fn fetch_comment_thread_for_mutation(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    thread_id: Uuid,
) -> Result<Option<ThreadRow>> {
    Ok(sqlx::query_as(
        r#"
        SELECT id, workspace_id, object_id, created_by, created_at, updated_at,
               resolved_at, resolved_by, retention_expires_at
        FROM kival.comment_threads
        WHERE workspace_id = $1
            AND object_id = $2
            AND id = $3
            AND (retention_expires_at IS NULL OR retention_expires_at > now())
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(thread_id)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Loads a bounded initial comment set for already-admitted mutation response hydration.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot load the projection.
pub async fn fetch_initial_comment_rows_for_mutation(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    thread_id: Uuid,
    limit: i64,
) -> Result<Vec<CommentRow>> {
    Ok(sqlx::query_as(
        r#"
        SELECT
            c.id, c.workspace_id, c.object_id, c.thread_id, c.parent_comment_id,
            c.author_user_id, u.username AS author_username,
            u.display_name AS author_display_name,
            CASE
                WHEN c.deleted_at IS NULL
                 AND c.expired_at IS NULL
                 AND c.retention_expires_at <= now()
                THEN NULL
                ELSE c.body
            END AS body,
            c.created_at, c.updated_at,
            CASE
                WHEN c.deleted_at IS NULL
                 AND c.expired_at IS NULL
                 AND c.retention_expires_at <= now()
                THEN NULL
                ELSE c.edited_at
            END AS edited_at,
            c.deleted_at, c.deleted_by,
            CASE
                WHEN c.deleted_at IS NULL
                 AND c.expired_at IS NULL
                 AND c.retention_expires_at <= now()
                THEN c.retention_expires_at
                ELSE c.expired_at
            END AS expired_at,
            c.retention_expires_at
        FROM kival.comments c
        JOIN kival.comment_threads t
            ON t.workspace_id = c.workspace_id
            AND t.object_id = c.object_id
            AND t.id = c.thread_id
        JOIN kival.users u
            ON u.id = c.author_user_id
        WHERE c.workspace_id = $1
            AND c.object_id = $2
            AND c.thread_id = $3
            AND (t.retention_expires_at IS NULL OR t.retention_expires_at > now())
        ORDER BY c.created_at, c.id
        LIMIT $4
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(thread_id)
    .bind(limit)
    .fetch_all(&mut **tx)
    .await?)
}

/// Loads one comment for already-admitted mutation response hydration.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot load the projection.
pub async fn fetch_comment_for_mutation(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    comment_id: Uuid,
) -> Result<Option<CommentRow>> {
    Ok(sqlx::query_as(
        r#"
        SELECT
            c.id, c.workspace_id, c.object_id, c.thread_id, c.parent_comment_id,
            c.author_user_id, u.username AS author_username,
            u.display_name AS author_display_name,
            CASE
                WHEN c.deleted_at IS NULL
                 AND c.expired_at IS NULL
                 AND c.retention_expires_at <= now()
                THEN NULL
                ELSE c.body
            END AS body,
            c.created_at, c.updated_at,
            CASE
                WHEN c.deleted_at IS NULL
                 AND c.expired_at IS NULL
                 AND c.retention_expires_at <= now()
                THEN NULL
                ELSE c.edited_at
            END AS edited_at,
            c.deleted_at, c.deleted_by,
            CASE
                WHEN c.deleted_at IS NULL
                 AND c.expired_at IS NULL
                 AND c.retention_expires_at <= now()
                THEN c.retention_expires_at
                ELSE c.expired_at
            END AS expired_at,
            c.retention_expires_at
        FROM kival.comments c
        JOIN kival.comment_threads t
            ON t.workspace_id = c.workspace_id
            AND t.object_id = c.object_id
            AND t.id = c.thread_id
        JOIN kival.users u
            ON u.id = c.author_user_id
        WHERE c.workspace_id = $1
            AND c.object_id = $2
            AND c.id = $3
            AND (t.retention_expires_at IS NULL OR t.retention_expires_at > now())
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(comment_id)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Loads mentions for already-admitted mutation response hydration.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot load the projection.
pub async fn fetch_comment_mentions_for_mutation(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    comment_ids: &[Uuid],
) -> Result<Vec<MentionRow>> {
    if comment_ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(sqlx::query_as(
        r#"
        SELECT m.comment_id, u.id AS user_id, u.username, u.display_name
        FROM kival.comment_mentions m
        JOIN kival.comments c
            ON c.id = m.comment_id
        JOIN kival.users u
            ON u.id = m.mentioned_user_id
        WHERE m.workspace_id = $1
            AND m.object_id = $2
            AND m.comment_id = ANY($3)
            AND c.workspace_id = $1
            AND c.object_id = $2
            AND c.deleted_at IS NULL
            AND c.expired_at IS NULL
            AND (c.retention_expires_at IS NULL OR c.retention_expires_at > now())
        ORDER BY u.username_normalized, u.id
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(comment_ids)
    .fetch_all(&mut **tx)
    .await?)
}
