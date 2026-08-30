//! Object version state bindings.
//!
//! Kival-native object version bodies are stored canonically in
//! `kival.object_versions.body_text`. Blob storage remains available for
//! attachments and imported binary artifacts, but normal textual object reads
//! and writes do not hydrate through blobs.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{KernelError, MembershipRole, ObjectRole, Result, parse_optional_stored};

/// Object version row with canonical text body from `PostgreSQL`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectVersion {
    /// Version ID.
    pub id: Uuid,
    /// Object ID.
    pub object_id: Uuid,
    /// Monotonic version number within the object.
    pub version_number: i64,
    /// Version title.
    pub title: String,
    /// Version body text.
    pub body: String,
    /// Version metadata.
    pub metadata: Value,
    /// User that created this version.
    pub created_by: Option<Uuid>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
}

/// Creator identity and effective access context for an object version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectVersionCreator {
    /// Version whose creator was resolved.
    pub version_id: Uuid,
    /// Creator username.
    pub username: String,
    /// Creator display name.
    pub display_name: String,
    /// Creator's active workspace role, including global administration.
    pub workspace_role: Option<MembershipRole>,
    /// Creator's effective object role.
    pub object_role: Option<ObjectRole>,
}

/// Request to create an object version with an explicit version number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateObjectVersion {
    /// Object ID for the new version.
    pub object_id: Uuid,
    /// Version number to insert.
    pub version_number: i64,
    /// Version title.
    pub title: String,
    /// Version body text.
    pub body: String,
    /// Version metadata object.
    pub metadata: Value,
    /// User that created this version.
    pub created_by: Option<Uuid>,
}

/// Atomic object-version update requested by an already-authorized server mutation.
#[derive(Debug)]
pub struct UpdateObjectVersion {
    /// Workspace containing the object.
    pub workspace_id: Uuid,
    /// Object being updated.
    pub object_id: Uuid,
    /// Optimistic current-version expectation.
    pub expected_current_version_id: Uuid,
    /// Replacement title, when supplied.
    pub title: Option<String>,
    /// Replacement body, when supplied.
    pub body: Option<String>,
    /// Replacement metadata, when supplied.
    pub metadata: Option<Value>,
    /// User creating the immutable version.
    pub created_by: Uuid,
}

/// Result of an atomic object-version update.
#[derive(Debug)]
pub struct UpdatedObjectVersion {
    /// Current immutable version after the transition.
    pub version: ObjectVersion,
    /// Title of the previously-current version.
    pub previous_title: String,
    /// Whether the transition appended and published a new version.
    pub changed: bool,
}

/// Resolves creator identities for readable object versions.
///
/// Disabled users remain resolvable because version authorship is historical. The creator
/// projection is returned only while the actor can read the requested object.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot load the creator projection.
pub async fn list_object_version_creators(
    pool: &sqlx::PgPool,
    actor_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    version_ids: &[Uuid],
) -> Result<Vec<ObjectVersionCreator>> {
    if version_ids.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query_as::<_, (Uuid, String, String, Option<String>, Option<String>)>(
        r#"
        SELECT
            version.id,
            creator.username,
            creator.display_name,
            CASE
                WHEN global_admin.user_id IS NOT NULL THEN 'admin'
                ELSE membership.workspace_role
            END AS workspace_role,
            kival.object_access_role(
                object.workspace_id,
                object.id,
                version.created_by
            )::text AS object_role
        FROM kival.object_versions version
        JOIN kival.objects object
            ON object.id = version.object_id
        JOIN kival.users creator
            ON creator.id = version.created_by
        LEFT JOIN kival.global_admins global_admin
            ON global_admin.user_id = version.created_by
            AND global_admin.revoked_at IS NULL
        LEFT JOIN kival.workspace_memberships membership
            ON membership.workspace_id = object.workspace_id
            AND membership.user_id = version.created_by
            AND membership.revoked_at IS NULL
        WHERE object.workspace_id = $2
            AND object.id = $3
            AND version.id = ANY($4)
            AND kival.user_can_read_object($2, $3, $1)
        OFFSET CASE WHEN kival.require_read_object($2, $3, $1) THEN 0 ELSE 0 END
        "#,
    )
    .bind(actor_id)
    .bind(workspace_id)
    .bind(object_id)
    .bind(version_ids)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|(version_id, username, display_name, workspace_role, object_role)| {
            Ok(ObjectVersionCreator {
                version_id,
                username,
                display_name,
                workspace_role: parse_optional_stored("workspace membership role", workspace_role)?,
                object_role: parse_optional_stored("object role", object_role)?,
            })
        })
        .collect()
}

/// Resolves one version creator for an already-admitted object mutation.
///
/// This projection deliberately performs no authorization check: the server has already admitted
/// the mutation, and response hydration must not become a second authorization decision.
///
/// # Errors
///
/// Returns an error if `PostgreSQL` cannot load the creator projection.
pub async fn fetch_object_version_creator_for_mutation(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    version_id: Uuid,
) -> Result<Option<ObjectVersionCreator>> {
    let row = sqlx::query_as::<_, (String, String, Option<String>, Option<String>)>(
        r#"
        SELECT
            creator.username,
            creator.display_name,
            CASE
                WHEN global_admin.user_id IS NOT NULL THEN 'admin'
                ELSE membership.workspace_role
            END AS workspace_role,
            kival.object_access_role(
                object.workspace_id,
                object.id,
                stored_version.created_by
            )::text AS object_role
        FROM kival.object_versions stored_version
        JOIN kival.objects object
            ON object.id = stored_version.object_id
        JOIN kival.users creator
            ON creator.id = stored_version.created_by
        LEFT JOIN kival.global_admins global_admin
            ON global_admin.user_id = stored_version.created_by
            AND global_admin.revoked_at IS NULL
        LEFT JOIN kival.workspace_memberships membership
            ON membership.workspace_id = object.workspace_id
            AND membership.user_id = stored_version.created_by
            AND membership.revoked_at IS NULL
        WHERE object.workspace_id = $1
            AND object.id = $2
            AND stored_version.id = $3
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(version_id)
    .fetch_optional(&mut **tx)
    .await?;

    row.map(|(username, display_name, workspace_role, object_role)| {
        Ok(ObjectVersionCreator {
            version_id,
            username,
            display_name,
            workspace_role: parse_optional_stored("workspace membership role", workspace_role)?,
            object_role: parse_optional_stored("object role", object_role)?,
        })
    })
    .transpose()
}

/// Stored object version row.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
struct StoredObjectVersionRow {
    /// Version ID.
    id: Uuid,
    /// Object ID.
    object_id: Uuid,
    /// Monotonic version number within the object.
    version_number: i64,
    /// Version title.
    title: String,
    /// Version body text.
    body_text: String,
    /// Version metadata.
    metadata: Value,
    /// User that created this version.
    created_by: Option<Uuid>,
    /// Creation timestamp.
    created_at: DateTime<Utc>,
}

/// Create an object version.
///
/// # Errors
///
/// Returns an error if the insert fails.
pub(crate) async fn create_object_version(
    tx: &mut Transaction<'_, Postgres>,
    request: CreateObjectVersion,
) -> Result<ObjectVersion> {
    let stored = sqlx::query_as::<_, StoredObjectVersionRow>(
        r#"
        INSERT INTO kival.object_versions (
            object_id,
            version_number,
            title,
            body_text,
            metadata,
            created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING
            id,
            object_id,
            version_number,
            title,
            body_text,
            metadata,
            created_by,
            created_at
        "#,
    )
    .bind(request.object_id)
    .bind(request.version_number)
    .bind(request.title)
    .bind(request.body)
    .bind(request.metadata)
    .bind(request.created_by)
    .fetch_one(&mut **tx)
    .await?;

    Ok(stored.into_object_version())
}

/// Applies one optimistic object-version update as a single kernel transition.
///
/// The transition owns object lifecycle locking, current-version validation, semantic no-op
/// detection, version append, and current-version publication. Authorization is intentionally
/// outside the kernel transition.
///
/// # Errors
///
/// Returns an error if the object is inactive, has no current version, changed since the expected
/// version, or `PostgreSQL` rejects the transition.
pub async fn update_object_version(
    tx: &mut Transaction<'_, Postgres>,
    request: UpdateObjectVersion,
) -> Result<UpdatedObjectVersion> {
    let locked = crate::objects::lock_active_object(tx, request.workspace_id, request.object_id)
        .await?
        .ok_or(KernelError::ResourceNotFound)?;
    let current_version_id =
        locked.current_version_id.ok_or(KernelError::ObjectHasNoCurrentVersion)?;

    if request.expected_current_version_id != current_version_id {
        return Err(KernelError::ObjectVersionConflict);
    }

    let current = fetch_object_version_in_tx(tx, request.object_id, current_version_id).await?;
    let previous_title = current.title.clone();
    let title = request.title.unwrap_or_else(|| current.title.clone());
    let body = request.body.unwrap_or_else(|| current.body.clone());
    let metadata = request.metadata.unwrap_or_else(|| current.metadata.clone());

    if title == current.title && body == current.body && metadata == current.metadata {
        return Ok(UpdatedObjectVersion { version: current, previous_title, changed: false });
    }

    let stored = sqlx::query_as::<_, StoredObjectVersionRow>(
        r#"
        INSERT INTO kival.object_versions (
            object_id,
            version_number,
            title,
            body_text,
            metadata,
            created_by
        )
        SELECT
            $1,
            COALESCE(MAX(version_number), 0) + 1,
            $2,
            $3,
            $4,
            $5
        FROM kival.object_versions
        WHERE object_id = $1
        RETURNING
            id,
            object_id,
            version_number,
            title,
            body_text,
            metadata,
            created_by,
            created_at
        "#,
    )
    .bind(request.object_id)
    .bind(title)
    .bind(body)
    .bind(metadata)
    .bind(request.created_by)
    .fetch_one(&mut **tx)
    .await?;
    let version = stored.into_object_version();
    crate::objects::set_active_object_version(
        tx,
        request.workspace_id,
        request.object_id,
        version.id,
    )
    .await?;

    Ok(UpdatedObjectVersion { version, previous_title, changed: true })
}

/// Fetch one object version row.
///
/// # Errors
///
/// Returns an error if the row cannot be loaded.
pub async fn fetch_object_version(
    pool: &sqlx::PgPool,
    actor_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    version_id: Uuid,
) -> Result<ObjectVersion> {
    let stored = sqlx::query_as::<_, StoredObjectVersionRow>(
        r#"
        SELECT
            id,
            object_id,
            version_number,
            title,
            body_text,
            metadata,
            created_by,
            created_at
        FROM kival.object_versions
        WHERE object_id = $3
            AND id = $4
            AND kival.user_can_read_object($2, $3, $1)
        OFFSET CASE WHEN kival.require_read_object($2, $3, $1) THEN 0 ELSE 0 END
        "#,
    )
    .bind(actor_id)
    .bind(workspace_id)
    .bind(object_id)
    .bind(version_id)
    .fetch_one(pool)
    .await?;

    Ok(stored.into_object_version())
}

/// Fetch one object version row inside an existing transaction.
///
/// # Errors
///
/// Returns an error if the row cannot be loaded.
pub async fn fetch_object_version_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    object_id: Uuid,
    version_id: Uuid,
) -> Result<ObjectVersion> {
    let stored = sqlx::query_as::<_, StoredObjectVersionRow>(
        r#"
        SELECT
            id,
            object_id,
            version_number,
            title,
            body_text,
            metadata,
            created_by,
            created_at
        FROM kival.object_versions
        WHERE object_id = $1
            AND id = $2
        "#,
    )
    .bind(object_id)
    .bind(version_id)
    .fetch_one(&mut **tx)
    .await?;

    Ok(stored.into_object_version())
}

/// Fetch one object version row by its monotonic version number.
///
/// # Errors
///
/// Returns an error if the row cannot be loaded.
pub async fn fetch_object_version_by_number(
    pool: &sqlx::PgPool,
    actor_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    version_number: i64,
) -> Result<ObjectVersion> {
    let stored = sqlx::query_as::<_, StoredObjectVersionRow>(
        r#"
        SELECT
            id,
            object_id,
            version_number,
            title,
            body_text,
            metadata,
            created_by,
            created_at
        FROM kival.object_versions
        WHERE object_id = $3
            AND version_number = $4
            AND kival.user_can_read_object($2, $3, $1)
        OFFSET CASE WHEN kival.require_read_object($2, $3, $1) THEN 0 ELSE 0 END
        "#,
    )
    .bind(actor_id)
    .bind(workspace_id)
    .bind(object_id)
    .bind(version_number)
    .fetch_one(pool)
    .await?;

    Ok(stored.into_object_version())
}

/// List object versions.
///
/// `before_version_number` implements descending version pagination. Pass
/// `None` to start at the latest version.
///
/// # Errors
///
/// Returns an error if rows cannot be loaded.
pub async fn list_object_versions(
    pool: &sqlx::PgPool,
    actor_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    before_version_number: Option<i64>,
    limit: i64,
) -> Result<Vec<ObjectVersion>> {
    let stored_versions = sqlx::query_as::<_, StoredObjectVersionRow>(
        r#"
        SELECT
            id,
            object_id,
            version_number,
            title,
            body_text,
            metadata,
            created_by,
            created_at
        FROM kival.object_versions
        WHERE object_id = $3
            AND kival.user_can_read_object($2, $3, $1)
            AND (
              $4::bigint IS NULL
            OR version_number < $4
          )
        ORDER BY version_number DESC
        LIMIT $5
        OFFSET CASE WHEN kival.require_read_object($2, $3, $1) THEN 0 ELSE 0 END
        "#,
    )
    .bind(actor_id)
    .bind(workspace_id)
    .bind(object_id)
    .bind(before_version_number)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(stored_versions.into_iter().map(StoredObjectVersionRow::into_object_version).collect())
}

impl StoredObjectVersionRow {
    /// Converts a database row into the kernel representation.
    fn into_object_version(self) -> ObjectVersion {
        ObjectVersion {
            id: self.id,
            object_id: self.object_id,
            version_number: self.version_number,
            title: self.title,
            body: self.body_text,
            metadata: self.metadata,
            created_by: self.created_by,
            created_at: self.created_at,
        }
    }
}
