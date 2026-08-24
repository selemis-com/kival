//! User state bindings.

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{KernelError, Result, UserListStatus, UserStatus, parse_stored};

/// User created by an administrative provisioning workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedUser {
    /// Database user identifier.
    pub id: Uuid,
    /// Canonical username stored by `PostgreSQL`.
    pub username: String,
}

/// Locked active user identity used by membership transitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveUserIdentity {
    /// User ID.
    pub id: Uuid,
    /// Username.
    pub username: String,
    /// Display name.
    pub display_name: String,
}

/// Inserts a user as part of an administrative transaction.
///
/// This deliberately exposes a Rust API rather than an HTTP endpoint. Callers
/// remain responsible for authorization, audit events, enrollment, and
/// committing their surrounding transaction.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects or cannot persist the user.
pub async fn create_user(
    transaction: &mut Transaction<'_, Postgres>,
    username: &str,
    display_name: &str,
) -> Result<CreatedUser> {
    let (id, username) = sqlx::query_as::<_, (Uuid, String)>(
        r#"
        INSERT INTO kival.users (username, display_name)
        VALUES ($1, $2)
        RETURNING id, username
        "#,
    )
    .bind(username)
    .bind(display_name)
    .fetch_one(&mut **transaction)
    .await?;

    Ok(CreatedUser { id, username })
}

/// Stored user lifecycle row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRow {
    /// User ID.
    pub id: Uuid,
    /// Username.
    pub username: String,
    /// Display name.
    pub display_name: String,
    /// Lifecycle status.
    pub status: UserStatus,
    /// Creation timestamp.
    pub created_at: time::OffsetDateTime,
    /// Last update timestamp.
    pub updated_at: time::OffsetDateTime,
    /// Disable timestamp.
    pub disabled_at: Option<time::OffsetDateTime>,
    /// User that disabled this user.
    pub disabled_by: Option<Uuid>,
}

/// Raw `PostgreSQL` projection used before constrained values are converted to typed kernel
/// vocabulary.
#[derive(sqlx::FromRow)]
struct StoredUserRow {
    /// Stored row identifier.
    id: Uuid,
    /// Stored username.
    username: String,
    /// Stored display name.
    display_name: String,
    /// Stored lifecycle status before typed parsing.
    status: String,
    /// Stored creation timestamp.
    created_at: time::OffsetDateTime,
    /// Stored update timestamp.
    updated_at: time::OffsetDateTime,
    /// Stored disable timestamp, when present.
    disabled_at: Option<time::OffsetDateTime>,
    /// Stored disabling-user identifier, when retained.
    disabled_by: Option<Uuid>,
}

impl TryFrom<StoredUserRow> for UserRow {
    type Error = KernelError;

    fn try_from(row: StoredUserRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            username: row.username,
            display_name: row.display_name,
            status: parse_stored("user status", row.status)?,
            created_at: row.created_at,
            updated_at: row.updated_at,
            disabled_at: row.disabled_at,
            disabled_by: row.disabled_by,
        })
    }
}

/// Loads one active user.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot load the user.
pub async fn fetch_active_user(pool: &sqlx::PgPool, user_id: Uuid) -> Result<UserRow> {
    sqlx::query_as::<_, StoredUserRow>(
        r#"
        SELECT id, username, display_name, status, created_at, updated_at, disabled_at, disabled_by
        FROM kival.users
        WHERE id = $1
            AND disabled_at IS NULL
        "#,
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?
    .try_into()
}

/// Resolves an active user by normalized username.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot resolve the user.
pub async fn active_user_id_by_username(
    pool: &sqlx::PgPool,
    username: &str,
) -> Result<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT id
        FROM kival.users
        WHERE username_normalized = lower($1)
            AND disabled_at IS NULL
        "#,
    )
    .bind(username)
    .fetch_optional(pool)
    .await?)
}

/// Pins and resolves an active user referenced by another resource.
///
/// `FOR SHARE` prevents disabling the user while the referencing transition commits without
/// serializing unrelated references to the same user.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot resolve the user.
pub(crate) async fn lock_active_user_for_reference(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Option<Uuid>,
    username: Option<&str>,
) -> Result<ActiveUserIdentity> {
    let (id, username, display_name) = sqlx::query_as::<_, (Uuid, String, String)>(
        r#"
        SELECT id, username, display_name
        FROM kival.users
        WHERE disabled_at IS NULL
            AND (
            ($1::uuid IS NOT NULL AND id = $1)
            OR ($1::uuid IS NULL AND username_normalized = lower($2))
        ) FOR SHARE
        "#,
    )
    .bind(user_id)
    .bind(username)
    .fetch_one(&mut **tx)
    .await?;
    Ok(ActiveUserIdentity { id, username, display_name })
}

/// Locks one active user by ID.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot lock the user.
pub async fn lock_active_user_by_id(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<Option<UserRow>> {
    sqlx::query_as::<_, StoredUserRow>(
        r#"
        SELECT id, username, display_name, status, created_at, updated_at, disabled_at, disabled_by
        FROM kival.users
        WHERE id = $1
            AND disabled_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?
    .map(TryInto::try_into)
    .transpose()
}

/// Lists users by lifecycle state and optional name query.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot load user state.
pub async fn list_users(
    pool: &sqlx::PgPool,
    actor_id: Uuid,
    cursor_created_at: Option<time::OffsetDateTime>,
    cursor_id: Option<Uuid>,
    limit: i64,
    status: UserListStatus,
    query: Option<&str>,
) -> Result<Vec<UserRow>> {
    sqlx::query_as::<_, StoredUserRow>(
        r#"
        SELECT id, username, display_name, status, created_at, updated_at, disabled_at, disabled_by
        FROM kival.users
        WHERE (
            $4 = 'all'
            OR ($4 = 'active' AND disabled_at IS NULL)
            OR ($4 = 'disabled' AND disabled_at IS NOT NULL)
        )
            AND ($1::timestamptz IS NULL OR (created_at, id) < ($1, $2))
            AND (
                $5::text IS NULL
                OR strpos(lower(display_name), lower($5)) > 0
                OR strpos(lower(username), lower($5)) > 0
            )
        ORDER BY created_at DESC, id DESC
        LIMIT $3
        OFFSET CASE
            WHEN kival.require_capability(
                TRUE,
                EXISTS (
                    SELECT 1
                    FROM kival.global_admins
                    WHERE user_id = $6
                        AND revoked_at IS NULL
                )
            )
            THEN 0
            ELSE 0
        END
        "#,
    )
    .bind(cursor_created_at)
    .bind(cursor_id)
    .bind(limit)
    .bind(status.as_str())
    .bind(query)
    .bind(actor_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(TryInto::try_into)
    .collect::<Result<Vec<_>>>()
}

/// Loads a user, allowing disabled users only when requested.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot load the user.
pub async fn fetch_user(pool: &sqlx::PgPool, actor_id: Uuid, user_id: Uuid) -> Result<UserRow> {
    sqlx::query_as::<_, StoredUserRow>(
        r#"
        SELECT id, username, display_name, status, created_at, updated_at, disabled_at, disabled_by
        FROM kival.users
        WHERE id = $1
            AND (
                disabled_at IS NULL
                OR EXISTS (
                    SELECT 1
                    FROM kival.global_admins
                    WHERE user_id = $2
                        AND revoked_at IS NULL
                )
            )
        OFFSET CASE
            WHEN kival.require_capability(
                TRUE,
                $1 = $2
                OR EXISTS (
                    SELECT 1
                    FROM kival.global_admins
                    WHERE user_id = $2
                        AND revoked_at IS NULL
                )
            )
            THEN 0
            ELSE 0
        END
        "#,
    )
    .bind(user_id)
    .bind(actor_id)
    .fetch_one(pool)
    .await?
    .try_into()
}

/// Updates the display name of an active user.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition.
pub async fn update_user_display_name(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    display_name: Option<&str>,
) -> Result<UserRow> {
    sqlx::query_as::<_, StoredUserRow>(
        r#"
        UPDATE kival.users
        SET display_name = COALESCE($2, display_name)
        WHERE id = $1
            AND disabled_at IS NULL
        RETURNING
            id, username, display_name, status,
            created_at, updated_at, disabled_at, disabled_by
        "#,
    )
    .bind(user_id)
    .bind(display_name)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}

/// Disables an active user through a normal authenticated transition.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition.
pub async fn disable_user(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    actor_id: Uuid,
) -> Result<UserRow> {
    sqlx::query_as::<_, StoredUserRow>(
        r#"
        UPDATE kival.users
        SET status = 'disabled',
            disabled_at = now(),
            disabled_by = $2,
            disabled_by_operator = false
        WHERE id = $1
            AND disabled_at IS NULL
        RETURNING
            id, username, display_name, status,
            created_at, updated_at, disabled_at, disabled_by
        "#,
    )
    .bind(user_id)
    .bind(actor_id)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}

/// Enables a disabled user without changing credentials or access state.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition.
pub async fn enable_user(tx: &mut Transaction<'_, Postgres>, user_id: Uuid) -> Result<UserRow> {
    sqlx::query_as::<_, StoredUserRow>(
        r#"
        UPDATE kival.users
        SET status = 'active',
            disabled_at = NULL,
            disabled_by = NULL,
            disabled_by_operator = false
        WHERE id = $1
            AND disabled_at IS NOT NULL
        RETURNING
            id, username, display_name, status,
            created_at, updated_at, disabled_at, disabled_by
        "#,
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await?
    .try_into()
}
