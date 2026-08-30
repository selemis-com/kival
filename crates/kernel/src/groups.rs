//! Group state bindings.

use sqlx::{PgPool, Postgres, Transaction};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{ArchiveListStatus, ArchiveStatus, KernelError, Result, parse_stored};

/// Stored group row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRow {
    /// Group ID.
    pub id: Uuid,
    /// Group name.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Lifecycle status.
    pub status: ArchiveStatus,
    /// Creator.
    pub created_by: Option<Uuid>,
    /// Archiver.
    pub archived_by: Option<Uuid>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Archive timestamp.
    pub archived_at: Option<DateTime<Utc>>,
}

/// Raw `PostgreSQL` projection used before constrained values are converted to typed kernel
/// vocabulary.
#[derive(sqlx::FromRow)]
struct StoredGroupRow {
    /// Stored row identifier.
    id: Uuid,
    /// Stored name.
    name: String,
    /// Stored optional description.
    description: Option<String>,
    /// Stored lifecycle status before typed parsing.
    status: String,
    /// Stored creator identifier, when retained.
    created_by: Option<Uuid>,
    /// Stored archiver identifier, when retained.
    archived_by: Option<Uuid>,
    /// Stored creation timestamp.
    created_at: DateTime<Utc>,
    /// Stored update timestamp.
    updated_at: DateTime<Utc>,
    /// Stored archive timestamp, when present.
    archived_at: Option<DateTime<Utc>>,
}

impl TryFrom<StoredGroupRow> for GroupRow {
    type Error = KernelError;

    fn try_from(row: StoredGroupRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            name: row.name,
            description: row.description,
            status: parse_stored("group status", row.status)?,
            created_by: row.created_by,
            archived_by: row.archived_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            archived_at: row.archived_at,
        })
    }
}

/// Pins an active group lifecycle while another resource refers to it.
///
/// `FOR SHARE` prevents archival while allowing unrelated group references to proceed.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot perform the lock.
pub(crate) async fn lock_active_group_for_reference(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM kival.groups
        WHERE id = $1
            AND archived_at IS NULL
        FOR SHARE
        "#,
    )
    .bind(group_id)
    .fetch_optional(&mut **tx)
    .await?
    .is_some())
}

/// Lists groups visible to an actor.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot read group state.
pub async fn list_groups(
    pool: &PgPool,
    cursor_created_at: Option<DateTime<Utc>>,
    cursor_id: Option<Uuid>,
    limit: i64,
    status: ArchiveListStatus,
    actor_id: Uuid,
    query: Option<&str>,
) -> Result<Vec<GroupRow>> {
    sqlx::query_as::<_, StoredGroupRow>(
        r#"
        SELECT g.id, g.name, g.description, g.status, g.created_by, g.archived_by,
            g.created_at, g.updated_at, g.archived_at
        FROM kival.groups g
        WHERE (
            $4 = 'all'
            OR ($4 = 'active' AND g.archived_at IS NULL)
            OR ($4 = 'archived' AND g.archived_at IS NOT NULL)
        )
            AND ($1::timestamptz IS NULL OR (g.created_at, g.id) < ($1, $2))
            AND (
                EXISTS (
                    SELECT 1
                    FROM kival.global_admins ga
                    WHERE ga.user_id = $5
                        AND ga.revoked_at IS NULL
                )
                OR EXISTS (
                    SELECT 1
                    FROM kival.group_memberships gm
                    WHERE gm.group_id = g.id
                        AND gm.user_id = $5
                        AND gm.group_role = 'admin'
                        AND gm.revoked_at IS NULL
                )
            )
            AND ($6::text IS NULL OR strpos(lower(g.name), lower($6)) > 0)
        ORDER BY g.created_at DESC, g.id DESC
        LIMIT $3
        "#,
    )
    .bind(cursor_created_at)
    .bind(cursor_id)
    .bind(limit)
    .bind(status.as_str())
    .bind(actor_id)
    .bind(query)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(TryInto::try_into)
    .collect::<Result<Vec<_>>>()
}

/// Creates a group.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the insert.
pub async fn create_group(
    tx: &mut Transaction<'_, Postgres>,
    name: &str,
    description: Option<&str>,
    actor_id: Uuid,
) -> Result<GroupRow> {
    sqlx::query_as::<_, StoredGroupRow>(
        r#"
        INSERT INTO kival.groups (name, description, created_by)
        VALUES ($1, $2, $3)
        RETURNING
            id, name, description, status, created_by, archived_by,
            created_at, updated_at, archived_at
        "#,
    )
    .bind(name)
    .bind(description)
    .bind(actor_id)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}

/// Fetches a group in any lifecycle state.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot read group state.
pub async fn fetch_group(pool: &PgPool, actor_id: Uuid, group_id: Uuid) -> Result<GroupRow> {
    sqlx::query_as::<_, StoredGroupRow>(
        r#"
        SELECT
            id, name, description, status, created_by, archived_by,
            created_at, updated_at, archived_at
        FROM kival.groups
        WHERE id = $1
            AND kival.user_can_read_group($1, $2)
        OFFSET CASE WHEN kival.require_read_group($1, $2) THEN 0 ELSE 0 END
        "#,
    )
    .bind(group_id)
    .bind(actor_id)
    .fetch_one(pool)
    .await?
    .try_into()
}

/// Updates mutable fields of an active group.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition.
pub async fn update_group(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    name: Option<&str>,
    description_present: bool,
    description: Option<String>,
) -> Result<GroupRow> {
    sqlx::query_as::<_, StoredGroupRow>(
        r#"
        UPDATE kival.groups
        SET name = COALESCE($2, name),
            description = CASE WHEN $3 THEN $4 ELSE description END
        WHERE id = $1
            AND archived_at IS NULL
        RETURNING
            id, name, description, status, created_by, archived_by,
            created_at, updated_at, archived_at
        "#,
    )
    .bind(group_id)
    .bind(name)
    .bind(description_present)
    .bind(description)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}

/// Archives an active group.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition.
pub async fn archive_group(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    actor_id: Uuid,
) -> Result<GroupRow> {
    sqlx::query_as::<_, StoredGroupRow>(
        r#"
        UPDATE kival.groups
        SET status = 'archived',
            archived_at = now(),
            archived_by = $2
        WHERE id = $1
            AND archived_at IS NULL
        RETURNING
            id, name, description, status, created_by, archived_by,
            created_at, updated_at, archived_at
        "#,
    )
    .bind(group_id)
    .bind(actor_id)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}

/// Unarchives a group.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition.
pub async fn unarchive_group(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
) -> Result<GroupRow> {
    sqlx::query_as::<_, StoredGroupRow>(
        r#"
        UPDATE kival.groups
        SET status = 'active',
            archived_at = NULL,
            archived_by = NULL
        WHERE id = $1
            AND archived_at IS NOT NULL
        RETURNING
            id, name, description, status, created_by, archived_by,
            created_at, updated_at, archived_at
        "#,
    )
    .bind(group_id)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}
