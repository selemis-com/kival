//! Derived references parsed from object version bodies.

use std::collections::BTreeSet;

use sqlx::{Acquire, Postgres, Transaction};
use uuid::Uuid;

use crate::Result;

/// Kind of internal object reference found in version content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObjectReferenceKind {
    /// Double-bracket title reference.
    Wikilink,
    /// Markdown link with a Kival object URL.
    KivalObjectLink,
}

impl ObjectReferenceKind {
    /// Returns the database representation.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Wikilink => "wikilink",
            Self::KivalObjectLink => "kival_object_link",
        }
    }
}

/// Parsed internal object reference before resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedObjectReference {
    /// Raw target text from the source content.
    raw_target: String,
    /// Optional display text.
    display_text: Option<String>,
    /// Parsed reference kind.
    kind: ObjectReferenceKind,
    /// Inclusive UTF-8 byte offset in the source body.
    span_start: i32,
    /// Exclusive UTF-8 byte offset in the source body.
    span_end: i32,
    /// Object ID encoded by a Kival URL, when present.
    target_object_id: Option<Uuid>,
    /// Workspace ID encoded by a fully scoped Kival URL, when present.
    target_workspace_id: Option<Uuid>,
}

/// Counts produced by derived-reference recomputation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectReferenceUpdate {
    /// Number of references resolved to objects.
    pub resolved_count: usize,
    /// Number of references that remain unresolved.
    pub unresolved_count: usize,
    /// Number of wikilinks that are ambiguous because multiple active objects match.
    pub ambiguous_count: usize,
    /// Number of old reference rows marked stale.
    pub stale_count: usize,
}

impl ObjectReferenceUpdate {
    /// Returns whether recomputation changed or found any references worth recording.
    #[must_use]
    pub const fn changed(self) -> bool {
        self.resolved_count > 0
            || self.unresolved_count > 0
            || self.ambiguous_count > 0
            || self.stale_count > 0
    }
}

/// Counts produced by event-driven wikilink re-resolution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReferenceReresolutionSummary {
    /// Total reference rows changed.
    pub updated_count: usize,
    /// Changed rows that are now resolved.
    pub resolved_count: usize,
    /// Changed rows that are now unresolved.
    pub unresolved_count: usize,
    /// Changed rows that are now ambiguous.
    pub ambiguous_count: usize,
}

impl ReferenceReresolutionSummary {
    /// Returns whether any reference row changed.
    #[must_use]
    pub const fn changed(self) -> bool {
        self.updated_count > 0
    }
}

/// Result of maintaining outgoing references and affected incoming wikilinks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectReferenceMaintenance {
    /// Result of rebuilding references for the current source version.
    pub reference_update: ObjectReferenceUpdate,
    /// Result of re-resolving wikilinks affected by title namespace changes.
    pub reresolution: ReferenceReresolutionSummary,
}

/// Wikilink derived from one immutable object version.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ObjectVersionWikilinkRow {
    /// Normalized title target authored inside the double brackets.
    pub raw_target: String,
    /// Optional display text authored after the `|` separator.
    pub display_text: Option<String>,
    /// Resolved target object when it exists and is readable by the requesting user.
    pub target_object_id: Option<Uuid>,
}

/// Resolution state for a title-based wikilink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WikilinkResolution {
    /// Exactly one active object matches.
    Resolved(Uuid),
    /// No active object matches.
    Unresolved,
    /// More than one active object matches.
    Ambiguous,
}

impl WikilinkResolution {
    /// Returns the target object ID, when uniquely resolved.
    const fn target_object_id(self) -> Option<Uuid> {
        match self {
            Self::Resolved(target_object_id) => Some(target_object_id),
            Self::Unresolved | Self::Ambiguous => None,
        }
    }

    /// Returns the database status.
    const fn status(self) -> &'static str {
        match self {
            Self::Resolved(_) => "resolved",
            Self::Unresolved => "unresolved",
            Self::Ambiguous => "ambiguous",
        }
    }
}

/// Parses supported internal object references from body text.
///
/// Malformed constructs are ignored. Spans are UTF-8 byte offsets.
///
/// # Panics
///
/// This function does not panic for malformed input.
#[must_use]
fn parse_object_references(body: &str) -> Vec<ParsedObjectReference> {
    let mut references = parse_wikilinks(body);
    references.extend(parse_kival_links(body));
    references.sort_by_key(|reference| (reference.span_start, reference.span_end));
    references
}

/// Maintains references for an object's current immutable version and affected titles.
///
/// The complete wikilink title set is derived before any namespace lock is acquired. Titles from
/// the stored source body and caller-supplied namespace changes are normalized, deduplicated, and
/// locked in one global order so unrelated title sets remain concurrent without cross-call
/// deadlocks.
///
/// # Errors
///
/// Returns an error if the source is not the object's current version, or if resolution or
/// persistence fails.
pub async fn maintain_object_references(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    source_object_id: Uuid,
    source_version_id: Uuid,
    affected_titles: &[String],
) -> Result<ObjectReferenceMaintenance> {
    let mut savepoint = (&mut **tx).begin().await?;
    let result = maintain_object_references_in_savepoint(
        &mut savepoint,
        workspace_id,
        source_object_id,
        source_version_id,
        affected_titles,
    )
    .await;

    match result {
        Ok(maintenance) => {
            savepoint.commit().await?;
            Ok(maintenance)
        }
        Err(error) => {
            savepoint.rollback().await?;
            Err(error)
        }
    }
}

/// Maintains one source version and affected title namespace inside a savepoint.
async fn maintain_object_references_in_savepoint(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    source_object_id: Uuid,
    source_version_id: Uuid,
    affected_titles: &[String],
) -> Result<ObjectReferenceMaintenance> {
    let references =
        current_object_references(tx, workspace_id, source_object_id, source_version_id).await?;
    let affected_titles = normalized_titles(affected_titles);
    let mut lock_titles = wikilink_titles(&references);
    lock_titles.extend(affected_titles.iter().cloned());
    lock_wikilink_titles(tx, workspace_id, &lock_titles).await?;

    let reference_update = recompute_object_references_from_parsed(
        tx,
        workspace_id,
        source_object_id,
        source_version_id,
        references,
    )
    .await?;
    let reresolution = re_resolve_locked_wikilinks(tx, workspace_id, &affected_titles).await?;

    Ok(ObjectReferenceMaintenance { reference_update, reresolution })
}

/// Loads and parses the stored body for the active object's current version.
async fn current_object_references(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    source_object_id: Uuid,
    source_version_id: Uuid,
) -> Result<Vec<ParsedObjectReference>> {
    let locked = crate::objects::lock_active_object(tx, workspace_id, source_object_id)
        .await?
        .ok_or(crate::KernelError::ResourceNotFound)?;
    if locked.current_version_id != Some(source_version_id) {
        return Err(crate::KernelError::ResourceNotFound);
    }

    let body = sqlx::query_scalar::<_, String>(
        r#"
        SELECT body_text
        FROM kival.object_versions
        WHERE object_id = $1
            AND id = $2
        "#,
    )
    .bind(source_object_id)
    .bind(source_version_id)
    .fetch_one(&mut **tx)
    .await?;

    Ok(parse_object_references(&body))
}

/// Rebuilds outgoing references after all relevant title locks have been acquired.
async fn recompute_object_references_from_parsed(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    source_object_id: Uuid,
    source_version_id: Uuid,
    references: Vec<ParsedObjectReference>,
) -> Result<ObjectReferenceUpdate> {
    let stale_count = sqlx::query(
        r#"
        UPDATE kival.object_references
        SET status = 'stale',
            updated_at = now()
        WHERE workspace_id = $1
            AND source_object_id = $2
            AND source_version_id <> $3
            AND status <> 'stale'
        "#,
    )
    .bind(workspace_id)
    .bind(source_object_id)
    .bind(source_version_id)
    .execute(&mut **tx)
    .await?
    .rows_affected() as usize;

    sqlx::query(
        r#"
        DELETE FROM kival.object_references
        WHERE workspace_id = $1
            AND source_object_id = $2
            AND source_version_id = $3
        "#,
    )
    .bind(workspace_id)
    .bind(source_object_id)
    .bind(source_version_id)
    .execute(&mut **tx)
    .await?;

    let mut update = ObjectReferenceUpdate {
        resolved_count: 0,
        unresolved_count: 0,
        ambiguous_count: 0,
        stale_count,
    };

    for reference in references {
        let (target_object_id, status) = match reference.kind {
            ObjectReferenceKind::Wikilink => {
                let resolution = resolve_wikilink(tx, workspace_id, &reference.raw_target).await?;
                match resolution {
                    WikilinkResolution::Resolved(_) => update.resolved_count += 1,
                    WikilinkResolution::Unresolved => update.unresolved_count += 1,
                    WikilinkResolution::Ambiguous => update.ambiguous_count += 1,
                }
                (resolution.target_object_id(), resolution.status())
            }
            ObjectReferenceKind::KivalObjectLink => {
                let target_object_id =
                    resolve_kival_object_link(tx, workspace_id, &reference).await?;
                if target_object_id.is_some() {
                    update.resolved_count += 1;
                    (target_object_id, "resolved")
                } else {
                    update.unresolved_count += 1;
                    (None, "unresolved")
                }
            }
        };

        sqlx::query(
            r#"
            INSERT INTO kival.object_references (
                workspace_id,
                source_object_id,
                source_version_id,
                target_object_id,
                raw_target,
                display_text,
                reference_kind,
                span_start,
                span_end,
                status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(workspace_id)
        .bind(source_object_id)
        .bind(source_version_id)
        .bind(target_object_id)
        .bind(reference.raw_target)
        .bind(reference.display_text)
        .bind(reference.kind.as_str())
        .bind(reference.span_start)
        .bind(reference.span_end)
        .bind(status)
        .execute(&mut **tx)
        .await?;
    }

    Ok(update)
}

/// Lists wikilinks derived from one object version.
///
/// Resolved target identifiers are withheld when the requesting user cannot read the target.
/// Unresolved and ambiguous references therefore also have no target identifier.
///
/// # Errors
///
/// Returns an error when the source object cannot be read or the underlying `PostgreSQL` query
/// fails.
pub async fn list_object_version_wikilinks(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    version_id: Uuid,
) -> Result<Vec<ObjectVersionWikilinkRow>> {
    Ok(sqlx::query_as::<_, ObjectVersionWikilinkRow>(
        r#"
        SELECT
            object_reference.raw_target,
            object_reference.display_text,
            CASE
                WHEN target_object.id IS NOT NULL
                    AND kival.has_object_permission(
                        target_object.workspace_id,
                        target_object.id,
                        $4,
                        CASE
                            WHEN target_object.archived_at IS NULL
                                THEN 'viewer'::kival.object_role
                            ELSE 'admin'::kival.object_role
                        END
                    )
                THEN object_reference.target_object_id
                ELSE NULL
            END AS target_object_id
        FROM kival.object_references object_reference
        LEFT JOIN kival.objects target_object
            ON target_object.workspace_id = object_reference.workspace_id
            AND target_object.id = object_reference.target_object_id
        WHERE object_reference.workspace_id = $1
            AND object_reference.source_object_id = $2
            AND object_reference.source_version_id = $3
            AND object_reference.reference_kind = 'wikilink'
        ORDER BY object_reference.span_start, object_reference.id
        OFFSET CASE WHEN kival.require_read_object($1, $2, $4) THEN 0 ELSE 0 END
        "#,
    )
    .bind(workspace_id)
    .bind(object_id)
    .bind(version_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

/// Re-resolves current-version wikilinks affected by title namespace changes.
///
/// Only wikilinks from current source versions are considered. Stale rows and stable Kival
/// object-ID links are never modified. Callers should combine title changes from one logical
/// transition into a single invocation so the complete lock set is acquired in global order.
///
/// # Errors
///
/// Returns an error if title lookup or reference updates fail.
pub async fn re_resolve_current_wikilinks_for_titles(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    affected_titles: &[String],
) -> Result<ReferenceReresolutionSummary> {
    let mut savepoint = (&mut **tx).begin().await?;
    let result = re_resolve_current_wikilinks_for_titles_in_savepoint(
        &mut savepoint,
        workspace_id,
        affected_titles,
    )
    .await;

    match result {
        Ok(summary) => {
            savepoint.commit().await?;
            Ok(summary)
        }
        Err(error) => {
            savepoint.rollback().await?;
            Err(error)
        }
    }
}

/// Re-resolves current wikilinks inside a cancellation-safe savepoint.
async fn re_resolve_current_wikilinks_for_titles_in_savepoint(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    affected_titles: &[String],
) -> Result<ReferenceReresolutionSummary> {
    let titles = normalized_titles(affected_titles);
    lock_wikilink_titles(tx, workspace_id, &titles).await?;
    re_resolve_locked_wikilinks(tx, workspace_id, &titles).await
}

/// Re-resolves titles after their advisory locks have already been acquired.
async fn re_resolve_locked_wikilinks(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    titles: &BTreeSet<String>,
) -> Result<ReferenceReresolutionSummary> {
    let mut summary = ReferenceReresolutionSummary::default();

    for title in titles {
        let resolution = resolve_wikilink(tx, workspace_id, title).await?;
        let changed_statuses = sqlx::query_scalar::<_, String>(
            r#"
            UPDATE kival.object_references reference
            SET target_object_id = $3,
                status = $4,
                updated_at = now()
            FROM kival.objects source_object
            WHERE reference.workspace_id = $1
                AND reference.raw_target = $2
                AND reference.reference_kind = 'wikilink'
                AND reference.status <> 'stale'
                AND source_object.workspace_id = reference.workspace_id
                AND source_object.id = reference.source_object_id
                AND source_object.current_version_id = reference.source_version_id
                AND (
                  reference.target_object_id IS DISTINCT FROM $3
                OR reference.status <> $4
              )
            RETURNING reference.status
            "#,
        )
        .bind(workspace_id)
        .bind(title)
        .bind(resolution.target_object_id())
        .bind(resolution.status())
        .fetch_all(&mut **tx)
        .await?;

        summary.updated_count += changed_statuses.len();
        match resolution {
            WikilinkResolution::Resolved(_) => summary.resolved_count += changed_statuses.len(),
            WikilinkResolution::Unresolved => {
                summary.unresolved_count += changed_statuses.len();
            }
            WikilinkResolution::Ambiguous => {
                summary.ambiguous_count += changed_statuses.len();
            }
        }
    }

    Ok(summary)
}

/// Returns normalized titles in their global advisory-lock order.
fn normalized_titles(titles: &[String]) -> BTreeSet<String> {
    titles
        .iter()
        .map(|title| title.trim())
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Returns title-based references in their global advisory-lock order.
fn wikilink_titles(references: &[ParsedObjectReference]) -> BTreeSet<String> {
    references
        .iter()
        .filter(|reference| reference.kind == ObjectReferenceKind::Wikilink)
        .map(|reference| reference.raw_target.clone())
        .collect()
}

/// Acquires all title locks in one deterministic order.
async fn lock_wikilink_titles(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    titles: &BTreeSet<String>,
) -> Result<()> {
    for title in titles {
        lock_wikilink_title(tx, workspace_id, title).await?;
    }
    Ok(())
}

/// Serializes namespace-dependent title resolution within a transaction.
async fn lock_wikilink_title(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    title: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        SELECT pg_advisory_xact_lock(
            hashtextextended($1::text || ':' || $2, 0)
        )
        "#,
    )
    .bind(workspace_id)
    .bind(title)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

/// Resolves a wikilink title against active objects in one workspace.
async fn resolve_wikilink(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    raw_target: &str,
) -> Result<WikilinkResolution> {
    let matches = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT object.id
        FROM kival.objects object
        JOIN kival.object_versions current_version
            ON current_version.object_id = object.id
            AND current_version.id = object.current_version_id
        WHERE object.workspace_id = $1
            AND current_version.title = $2
            AND object.archived_at IS NULL
        ORDER BY object.id
        LIMIT 2
        "#,
    )
    .bind(workspace_id)
    .bind(raw_target)
    .fetch_all(&mut **tx)
    .await?;

    Ok(match matches.as_slice() {
        [] => WikilinkResolution::Unresolved,
        [target_object_id] => WikilinkResolution::Resolved(*target_object_id),
        [_, _, ..] => WikilinkResolution::Ambiguous,
    })
}

/// Resolves a stable Kival object-ID link within a workspace.
async fn resolve_kival_object_link(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    reference: &ParsedObjectReference,
) -> Result<Option<Uuid>> {
    if reference
        .target_workspace_id
        .is_some_and(|target_workspace_id| target_workspace_id != workspace_id)
    {
        return Ok(None);
    }

    let Some(target_object_id) = reference.target_object_id else {
        return Ok(None);
    };

    sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM kival.objects
        WHERE workspace_id = $1
            AND id = $2
        "#,
    )
    .bind(workspace_id)
    .bind(target_object_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(Into::into)
}

/// Parses double-bracket wikilinks.
fn parse_wikilinks(body: &str) -> Vec<ParsedObjectReference> {
    let mut references = Vec::new();
    let mut cursor = 0;

    while let Some(relative_start) = body[cursor..].find("[[") {
        let start = cursor + relative_start;
        let content_start = start + 2;
        let Some(relative_end) = body[content_start..].find("]]") else {
            break;
        };
        let content_end = content_start + relative_end;
        let end = content_end + 2;
        let content = &body[content_start..content_end];

        if !content.contains("[[") {
            let (raw_target, display_text) = match content.split_once('|') {
                Some((target, display)) => (target.trim(), non_empty_owned(display)),
                None => (content.trim(), None),
            };

            if !raw_target.is_empty() {
                push_reference(
                    &mut references,
                    ParsedObjectReference {
                        raw_target: raw_target.to_owned(),
                        display_text,
                        kind: ObjectReferenceKind::Wikilink,
                        span_start: 0,
                        span_end: 0,
                        target_object_id: None,
                        target_workspace_id: None,
                    },
                    start,
                    end,
                );
            }
        }

        cursor = end;
    }

    references
}

/// Parses Markdown links with supported Kival destinations.
fn parse_kival_links(body: &str) -> Vec<ParsedObjectReference> {
    let mut references = Vec::new();
    let mut cursor = 0;

    while let Some(relative_start) = body[cursor..].find('[') {
        let start = cursor + relative_start;
        if start > 0 && body.as_bytes()[start - 1] == b'!' {
            cursor = start + 1;
            continue;
        }

        let label_start = start + 1;
        let Some(relative_label_end) = body[label_start..].find(']') else {
            break;
        };
        let label_end = label_start + relative_label_end;
        let destination_marker = label_end + 1;
        if body.as_bytes().get(destination_marker) != Some(&b'(') {
            cursor = label_end + 1;
            continue;
        }

        let destination_start = destination_marker + 1;
        let Some(relative_destination_end) = body[destination_start..].find(')') else {
            break;
        };
        let destination_end = destination_start + relative_destination_end;
        let end = destination_end + 1;
        let destination = body[destination_start..destination_end].trim();

        if let Some((target_workspace_id, target_object_id)) = parse_kival_destination(destination)
        {
            push_reference(
                &mut references,
                ParsedObjectReference {
                    raw_target: destination.to_owned(),
                    display_text: non_empty_owned(&body[label_start..label_end]),
                    kind: ObjectReferenceKind::KivalObjectLink,
                    span_start: 0,
                    span_end: 0,
                    target_object_id: Some(target_object_id),
                    target_workspace_id,
                },
                start,
                end,
            );
        }

        cursor = end;
    }

    references
}

/// Parses a supported Kival URL into optional workspace and object IDs.
fn parse_kival_destination(destination: &str) -> Option<(Option<Uuid>, Uuid)> {
    if let Some(object_id) = destination.strip_prefix("kival://objects/") {
        return parse_exact_uuid(object_id).map(|object_id| (None, object_id));
    }

    let rest = destination.strip_prefix("kival://workspaces/")?;
    let (workspace_id, object_id) = rest.split_once("/objects/")?;
    Some((Some(parse_exact_uuid(workspace_id)?), parse_exact_uuid(object_id)?))
}

/// Parses a UUID only when the entire value is the UUID.
fn parse_exact_uuid(value: &str) -> Option<Uuid> {
    (!value.is_empty() && !value.contains('/')).then(|| Uuid::parse_str(value).ok()).flatten()
}

/// Returns a trimmed owned string unless it is empty.
fn non_empty_owned(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// Adds a parsed reference if its byte offsets fit the database representation.
fn push_reference(
    references: &mut Vec<ParsedObjectReference>,
    mut reference: ParsedObjectReference,
    start: usize,
    end: usize,
) {
    let (Ok(span_start), Ok(span_end)) = (i32::try_from(start), i32::try_from(end)) else {
        return;
    };
    reference.span_start = span_start;
    reference.span_end = span_end;
    references.push(reference);
}
