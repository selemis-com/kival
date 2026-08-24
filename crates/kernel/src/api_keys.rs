//! API-key state bindings.

use sqlx::{Acquire, PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{ApiKeyScope, Result, parse_stored};

/// Stored API-key metadata row.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ApiKeyRow {
    /// API key ID.
    pub id: Uuid,
    /// Owning user.
    pub user_id: Uuid,
    /// Stable audit label.
    pub label: String,
    /// Mutable authorization revision.
    pub authorization_revision: i32,
    /// Creation timestamp.
    pub created_at: OffsetDateTime,
    /// Update timestamp.
    pub updated_at: OffsetDateTime,
    /// Optional expiry.
    pub expires_at: Option<OffsetDateTime>,
    /// Optional revocation timestamp.
    pub revoked_at: Option<OffsetDateTime>,
    /// Optional last-used timestamp.
    pub last_used_at: Option<OffsetDateTime>,
}

/// Previous and current authorization data after an API-key update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeyAuthorizationUpdate {
    /// Updated key row.
    pub row: ApiKeyRow,
    /// Scopes before replacement.
    pub previous_scopes: Vec<ApiKeyScope>,
    /// Workspace restrictions before replacement.
    pub previous_workspace_ids: Vec<Uuid>,
}

/// Creates an API key when its optional expiration is in the future.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the insert.
pub async fn create_api_key(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    label: &str,
    token_hash: &[u8],
    expires_at: Option<OffsetDateTime>,
) -> Result<Option<ApiKeyRow>> {
    Ok(sqlx::query_as::<_, ApiKeyRow>(
        r#"
        INSERT INTO kival.api_keys (user_id, label, token_hash, expires_at)
        SELECT $1, $2, $3, $4 WHERE $4::timestamptz IS NULL OR $4 > clock_timestamp()
        RETURNING id, user_id, label, authorization_revision, created_at, updated_at,
                  expires_at, revoked_at, last_used_at
        "#,
    )
    .bind(user_id)
    .bind(label)
    .bind(token_hash)
    .bind(expires_at)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Replaces scope and workspace delegation rows for a key without changing its revision.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects any delegation row.
pub async fn set_api_key_delegation(
    tx: &mut Transaction<'_, Postgres>,
    api_key_id: Uuid,
    scopes: &[ApiKeyScope],
    workspace_ids: &[Uuid],
) -> Result<()> {
    let mut savepoint = (&mut **tx).begin().await?;
    let result =
        set_api_key_delegation_in_savepoint(&mut savepoint, api_key_id, scopes, workspace_ids)
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

/// Applies API-key delegation replacement inside a cancellation-safe savepoint.
async fn set_api_key_delegation_in_savepoint(
    tx: &mut Transaction<'_, Postgres>,
    api_key_id: Uuid,
    scopes: &[ApiKeyScope],
    workspace_ids: &[Uuid],
) -> Result<()> {
    sqlx::query(
        r#"
        DELETE FROM kival.api_key_scopes
        WHERE api_key_id = $1
        "#,
    )
    .bind(api_key_id)
    .execute(&mut **tx)
    .await?;
    for scope in scopes {
        sqlx::query(
            r#"
            INSERT INTO kival.api_key_scopes (api_key_id, scope)
            VALUES ($1, $2)
            "#,
        )
        .bind(api_key_id)
        .bind(scope.as_str())
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query(
        r#"
        DELETE FROM kival.api_key_workspaces
        WHERE api_key_id = $1
        "#,
    )
    .bind(api_key_id)
    .execute(&mut **tx)
    .await?;
    if !workspace_ids.is_empty() {
        sqlx::query(
            r#"
            INSERT INTO kival.api_key_workspaces (api_key_id, workspace_id)
            SELECT $1, workspace_id
            FROM unnest($2::uuid[]) AS delegated(workspace_id)
            "#,
        )
        .bind(api_key_id)
        .bind(workspace_ids)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Returns whether every requested workspace is delegable by the actor.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot evaluate access state.
pub async fn api_key_workspaces_accessible(
    tx: &mut Transaction<'_, Postgres>,
    actor_id: Uuid,
    workspace_ids: &[Uuid],
) -> Result<bool> {
    if workspace_ids.is_empty() {
        return Ok(true);
    }
    let count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT count(*)
        FROM kival.workspaces w
        WHERE w.id = ANY($1)
            AND (
                EXISTS (
                    SELECT 1
                    FROM kival.workspace_memberships wm
                    WHERE wm.workspace_id = w.id
                        AND wm.user_id = $2
                        AND wm.revoked_at IS NULL
                )
                OR EXISTS (
                    SELECT 1
                    FROM kival.global_admins ga
                    WHERE ga.user_id = $2
                        AND ga.revoked_at IS NULL
                )
            )
        "#,
    )
    .bind(workspace_ids)
    .bind(actor_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(count == workspace_ids.len() as i64)
}

/// Lists API keys owned by one user.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot read key state.
pub async fn list_api_keys(
    pool: &PgPool,
    user_id: Uuid,
    cursor_created_at: Option<OffsetDateTime>,
    cursor_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<ApiKeyRow>> {
    Ok(sqlx::query_as::<_, ApiKeyRow>(
        r#"
        SELECT
            id, user_id, label, authorization_revision, created_at, updated_at,
            expires_at, revoked_at, last_used_at
        FROM kival.api_keys
        WHERE user_id = $3
            AND ($1::timestamptz IS NULL OR (created_at, id) < ($1, $2))
        ORDER BY created_at DESC, id DESC
        LIMIT $4
        "#,
    )
    .bind(cursor_created_at)
    .bind(cursor_id)
    .bind(user_id)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

/// Loads scope rows for a set of API keys.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot read key scopes.
pub async fn list_api_key_scopes(pool: &PgPool, ids: &[Uuid]) -> Result<Vec<(Uuid, ApiKeyScope)>> {
    let rows = sqlx::query_as::<_, (Uuid, String)>(
        r#"
        SELECT api_key_id, scope
        FROM kival.api_key_scopes
        WHERE api_key_id = ANY($1)
        ORDER BY api_key_id, scope
        "#,
    )
    .bind(ids)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|(api_key_id, scope)| Ok((api_key_id, parse_stored("API key scope", scope)?)))
        .collect()
}

/// Loads workspace restrictions for a set of API keys.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot read key restrictions.
pub async fn list_api_key_workspaces(pool: &PgPool, ids: &[Uuid]) -> Result<Vec<(Uuid, Uuid)>> {
    Ok(sqlx::query_as::<_, (Uuid, Uuid)>(
        r#"
        SELECT api_key_id, workspace_id
        FROM kival.api_key_workspaces
        WHERE api_key_id = ANY($1)
        ORDER BY api_key_id, workspace_id
        "#,
    )
    .bind(ids)
    .fetch_all(pool)
    .await?)
}

/// Locks one active key owned by a user.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot lock key state.
pub async fn lock_active_api_key(
    tx: &mut Transaction<'_, Postgres>,
    api_key_id: Uuid,
    user_id: Uuid,
) -> Result<Option<ApiKeyRow>> {
    Ok(sqlx::query_as::<_, ApiKeyRow>(
        r#"
        SELECT
            id, user_id, label, authorization_revision, created_at, updated_at,
            expires_at, revoked_at, last_used_at
        FROM kival.api_keys
        WHERE id = $1
            AND user_id = $2
            AND revoked_at IS NULL FOR UPDATE
        "#,
    )
    .bind(api_key_id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Replaces an API key's authorization data and increments its revision.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition.
pub async fn replace_api_key_authorization(
    tx: &mut Transaction<'_, Postgres>,
    api_key_id: Uuid,
    scopes: &[ApiKeyScope],
    workspace_ids: &[Uuid],
) -> Result<ApiKeyAuthorizationUpdate> {
    let mut savepoint = (&mut **tx).begin().await?;
    let result = replace_api_key_authorization_in_savepoint(
        &mut savepoint,
        api_key_id,
        scopes,
        workspace_ids,
    )
    .await;

    match result {
        Ok(update) => {
            savepoint.commit().await?;
            Ok(update)
        }
        Err(error) => {
            savepoint.rollback().await?;
            Err(error)
        }
    }
}

/// Applies API-key authorization replacement inside a cancellation-safe savepoint.
async fn replace_api_key_authorization_in_savepoint(
    tx: &mut Transaction<'_, Postgres>,
    api_key_id: Uuid,
    scopes: &[ApiKeyScope],
    workspace_ids: &[Uuid],
) -> Result<ApiKeyAuthorizationUpdate> {
    let previous_scopes = sqlx::query_scalar::<_, String>(
        r#"
        SELECT scope
        FROM kival.api_key_scopes
        WHERE api_key_id = $1
        ORDER BY scope
        "#,
    )
    .bind(api_key_id)
    .fetch_all(&mut **tx)
    .await?;
    let previous_scopes = previous_scopes
        .into_iter()
        .map(|scope| parse_stored("API key scope", scope))
        .collect::<Result<Vec<_>>>()?;
    let previous_workspace_ids = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT workspace_id
        FROM kival.api_key_workspaces
        WHERE api_key_id = $1
        ORDER BY workspace_id
        "#,
    )
    .bind(api_key_id)
    .fetch_all(&mut **tx)
    .await?;
    set_api_key_delegation_in_savepoint(tx, api_key_id, scopes, workspace_ids).await?;
    let row = sqlx::query_as::<_, ApiKeyRow>(
        r#"
        UPDATE kival.api_keys
        SET authorization_revision = authorization_revision + 1
        WHERE id = $1
        RETURNING id, user_id, label, authorization_revision, created_at, updated_at,
                  expires_at, revoked_at, last_used_at
        "#,
    )
    .bind(api_key_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(ApiKeyAuthorizationUpdate { row, previous_scopes, previous_workspace_ids })
}

/// Revokes one active key owned by a user.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` rejects the transition.
pub async fn revoke_api_key(
    tx: &mut Transaction<'_, Postgres>,
    api_key_id: Uuid,
    user_id: Uuid,
) -> Result<Option<ApiKeyRow>> {
    Ok(sqlx::query_as::<_, ApiKeyRow>(
        r#"
        UPDATE kival.api_keys
        SET revoked_at = now(),
            revoked_by = $2
        WHERE id = $1
            AND user_id = $2
            AND revoked_at IS NULL
        RETURNING id, user_id, label, authorization_revision, created_at, updated_at,
                  expires_at, revoked_at, last_used_at
        "#,
    )
    .bind(api_key_id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Loads one API key owned by a user in any lifecycle state.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot read key state.
pub async fn fetch_api_key(
    tx: &mut Transaction<'_, Postgres>,
    api_key_id: Uuid,
    user_id: Uuid,
) -> Result<ApiKeyRow> {
    Ok(sqlx::query_as::<_, ApiKeyRow>(
        r#"
        SELECT
            id, user_id, label, authorization_revision, created_at, updated_at,
            expires_at, revoked_at, last_used_at
        FROM kival.api_keys
        WHERE id = $1
            AND user_id = $2
        "#,
    )
    .bind(api_key_id)
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await?)
}

/// API-key identity and authorization projection used during request authentication.
#[derive(Debug, Clone)]
pub struct ApiKeyAuthentication {
    /// Row identifier.
    pub id: Uuid,
    /// User identifier.
    pub user_id: Uuid,
    /// Human-readable credential label.
    pub label: String,
    /// Effective API-key scopes.
    pub scopes: Vec<ApiKeyScope>,
    /// Whether the requested workspace is allowed by the API key.
    pub workspace_allowed: bool,
}

/// Resolves one active API key, its active owner, scopes, and optional workspace access.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn authenticate_api_key(
    pool: &PgPool,
    token_hash: &[u8],
    workspace_id: Option<Uuid>,
) -> Result<Option<ApiKeyAuthentication>> {
    let row = sqlx::query_as::<_, (Uuid, Uuid, String, Vec<String>, bool)>(
        r#"
        SELECT
            k.id, u.id, k.label,
            COALESCE(
                array_agg(s.scope ORDER BY s.scope) FILTER (WHERE s.scope IS NOT NULL),
                ARRAY[]::text[]
            ) AS scopes,
            CASE
                WHEN $2::uuid IS NULL THEN true
                ELSE EXISTS (
                    SELECT 1
                    FROM kival.api_key_workspaces w
                    WHERE w.api_key_id = k.id
                        AND w.workspace_id = $2
                )
            END AS workspace_allowed
        FROM kival.api_keys k
        JOIN kival.users u
            ON u.id = k.user_id
        LEFT JOIN kival.api_key_scopes s
            ON s.api_key_id = k.id
        WHERE k.token_hash = $1
            AND k.revoked_at IS NULL
            AND (k.expires_at IS NULL OR k.expires_at > now())
            AND u.disabled_at IS NULL
        GROUP BY k.id, u.id, k.label
        "#,
    )
    .bind(token_hash)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;
    row.map(|(id, user_id, label, scopes, workspace_allowed)| {
        let scopes = scopes
            .into_iter()
            .map(|scope| parse_stored("API key scope", scope))
            .collect::<Result<Vec<_>>>()?;
        Ok(ApiKeyAuthentication { id, user_id, label, scopes, workspace_allowed })
    })
    .transpose()
}

/// Opportunistically updates an active API key's last-used timestamp.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn touch_api_key_last_used(pool: &PgPool, api_key_id: Uuid) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE kival.api_keys
        SET last_used_at = now()
        WHERE id = $1
            AND revoked_at IS NULL
            AND (last_used_at IS NULL OR last_used_at < now() - interval '5 minutes')
        "#,
    )
    .bind(api_key_id)
    .execute(pool)
    .await?;
    Ok(())
}
