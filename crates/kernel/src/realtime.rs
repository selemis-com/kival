//! Realtime credential and resource-authorization state bindings.

use sqlx::PgPool;
use uuid::Uuid;

/// Returns whether a realtime session is still active for its user.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn realtime_session_active(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM kival.sessions session
            JOIN kival.users user_account
                ON user_account.id = session.user_id
            WHERE session.id = $1
                AND session.user_id = $2
                AND session.revoked_at IS NULL
                AND session.expires_at > now()
                AND user_account.disabled_at IS NULL
        )
        "#,
    )
    .bind(session_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
}

/// Returns whether an API key remains valid for realtime access.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn realtime_api_key_active(
    pool: &PgPool,
    user_id: Uuid,
    api_key_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM kival.api_keys api_key
            JOIN kival.users user_account
                ON user_account.id = api_key.user_id
            WHERE api_key.id = $1
                AND api_key.user_id = $2
                AND api_key.revoked_at IS NULL
                AND (api_key.expires_at IS NULL OR api_key.expires_at > now())
                AND user_account.disabled_at IS NULL
                AND EXISTS (
                    SELECT 1
                    FROM kival.api_key_scopes scope
                    WHERE scope.api_key_id = api_key.id
                        AND scope.scope = 'realtime:read'
                  )
        )
        "#,
    )
    .bind(api_key_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
}

/// Returns whether an API key remains authorized for an object realtime stream.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn realtime_api_key_object_authorized(
    pool: &PgPool,
    user_id: Uuid,
    api_key_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM kival.api_keys api_key
            JOIN kival.users user_account
                ON user_account.id = api_key.user_id
            WHERE api_key.id = $1
                AND api_key.user_id = $2
                AND api_key.revoked_at IS NULL
                AND (api_key.expires_at IS NULL OR api_key.expires_at > now())
                AND user_account.disabled_at IS NULL
                AND EXISTS (
                    SELECT 1
                    FROM kival.api_key_scopes scope
                    WHERE scope.api_key_id = api_key.id
                        AND scope.scope = 'realtime:read'
                  )
                        AND EXISTS (
                    SELECT 1
                    FROM kival.api_key_workspaces delegated
                    WHERE delegated.api_key_id = api_key.id
                        AND delegated.workspace_id = $3
                  )
                        AND EXISTS (
                    SELECT 1
                    FROM kival.api_key_scopes resource_scope
                    WHERE resource_scope.api_key_id = api_key.id
                        AND resource_scope.scope IN ('objects:read', 'objects:write')
                  )
                        AND EXISTS (
                    SELECT 1
                    FROM kival.workspaces workspace
                    WHERE workspace.id = $3
                        AND workspace.archived_at IS NULL
                        AND (
                            EXISTS (
                                SELECT 1
                                FROM kival.global_admins global_admin
                                WHERE global_admin.user_id = $2
                                    AND global_admin.revoked_at IS NULL
                            )
                                    OR EXISTS (
                                SELECT 1
                                FROM kival.workspace_memberships membership
                                WHERE membership.workspace_id = workspace.id
                                    AND membership.user_id = $2
                                    AND membership.revoked_at IS NULL
                            )
                          )
                  )
                                    AND EXISTS (
                    SELECT 1
                    FROM kival.objects object
                    WHERE object.workspace_id = $3
                        AND object.id = $4
                        AND (
                            (
                                object.archived_at IS NULL
                        AND kival.has_object_permission(
                                    object.workspace_id, object.id, $2, 'viewer'::kival.object_role
                                )
                            )
                        OR (
                                object.archived_at IS NOT NULL
                        AND kival.has_object_permission(
                                    object.workspace_id, object.id, $2, 'admin'::kival.object_role
                                )
                            )
                          )
                  )
        )
        "#,
    )
    .bind(api_key_id)
    .bind(user_id)
    .bind(workspace_id)
    .bind(object_id)
    .fetch_one(pool)
    .await
}

/// Returns whether a user remains authorized for workspace realtime state.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn realtime_workspace_authorized(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM kival.workspaces workspace
            WHERE workspace.id = $1
                AND workspace.archived_at IS NULL
                AND (
                    EXISTS (
                        SELECT 1
                        FROM kival.global_admins global_admin
                        WHERE global_admin.user_id = $2
                            AND global_admin.revoked_at IS NULL
                    )
                            OR EXISTS (
                        SELECT 1
                        FROM kival.workspace_memberships membership
                        WHERE membership.workspace_id = workspace.id
                            AND membership.user_id = $2
                            AND membership.revoked_at IS NULL
                    )
                  )
        )
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
}

/// Returns whether a user remains authorized for object realtime state.
///
/// # Errors
///
/// Returns an error if the underlying `PostgreSQL` operation fails.
pub async fn realtime_object_authorized(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM kival.objects object
            JOIN kival.workspaces workspace
                ON workspace.id = object.workspace_id
            WHERE object.workspace_id = $1
                AND object.id = $2
                AND workspace.archived_at IS NULL
                AND (
                    (
                        object.archived_at IS NULL
                AND kival.has_object_permission(
                            object.workspace_id, object.id, $3, 'viewer'::kival.object_role
                        )
                    )
                OR (
                        object.archived_at IS NOT NULL
                AND kival.has_object_permission(
                            object.workspace_id, object.id, $3, 'admin'::kival.object_role
                        )
                    )
                  )
        )
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
}
