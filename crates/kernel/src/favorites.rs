//! Personal workspace and object favorite state bindings.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::Result;

/// Pins a workspace for a user and returns the stable pin creation timestamp.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot persist the pin.
pub async fn pin_workspace(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<DateTime<Utc>> {
    Ok(sqlx::query_scalar::<_, DateTime<Utc>>(
        r#"
        INSERT INTO kival.workspace_pins (user_id, workspace_id)
        VALUES ($1, $2)
        ON CONFLICT (user_id, workspace_id)
        DO UPDATE SET workspace_id = EXCLUDED.workspace_id
        RETURNING created_at
        "#,
    )
    .bind(user_id)
    .bind(workspace_id)
    .fetch_one(pool)
    .await?)
}

/// Removes a workspace pin when present.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot update the pin state.
pub async fn unpin_workspace(pool: &PgPool, user_id: Uuid, workspace_id: Uuid) -> Result<()> {
    sqlx::query(
        r#"
        DELETE FROM kival.workspace_pins
        WHERE user_id = $1
            AND workspace_id = $2
        "#,
    )
    .bind(user_id)
    .bind(workspace_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Pins an object for a user and returns the stable pin creation timestamp.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot persist the pin.
pub async fn pin_object(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<DateTime<Utc>> {
    Ok(sqlx::query_scalar::<_, DateTime<Utc>>(
        r#"
        INSERT INTO kival.object_pins (user_id, workspace_id, object_id)
        VALUES ($1, $2, $3)
        ON CONFLICT (user_id, object_id)
        DO UPDATE SET workspace_id = EXCLUDED.workspace_id
        RETURNING created_at
        "#,
    )
    .bind(user_id)
    .bind(workspace_id)
    .bind(object_id)
    .fetch_one(pool)
    .await?)
}

/// Removes an object pin when present.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot update the pin state.
pub async fn unpin_object(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        DELETE FROM kival.object_pins
        WHERE user_id = $1
            AND workspace_id = $2
            AND object_id = $3
        "#,
    )
    .bind(user_id)
    .bind(workspace_id)
    .bind(object_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Marks an object as a favorite for a user.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot persist the favorite.
pub async fn favorite_object(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO kival.object_favorites (user_id, workspace_id, object_id)
        VALUES ($1, $2, $3)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(user_id)
    .bind(workspace_id)
    .bind(object_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Removes an object favorite when present.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot update the favorite state.
pub async fn unfavorite_object(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        DELETE FROM kival.object_favorites
        WHERE user_id = $1
            AND workspace_id = $2
            AND object_id = $3
        "#,
    )
    .bind(user_id)
    .bind(workspace_id)
    .bind(object_id)
    .execute(pool)
    .await?;
    Ok(())
}
