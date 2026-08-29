//! Object version history, diff, and restore commands.

use argx::{Args, Subcommand, argx};
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{
    ArchiveStatus, KivalClient, ListResponse, ObjectResponse, ObjectRole, ObjectVersion,
    ObjectVersionWikilinksResponse, UpdateObjectRequest,
};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use super::{
    ObjectTargetArgs,
    display::{print_version_line, print_version_response},
};
use crate::utils::{
    args::{DEFAULT_LIST_LIMIT, list_params},
    credentials::authenticated_client,
    diff::unified_diff,
    error::{CliError, CliErrorCode},
    output::{OutputMode, print_empty_list, print_output, quote_human_string},
};

/// Number of unchanged lines included around each unified-diff change.
const DIFF_CONTEXT_LINES: usize = 3;

/// Arguments for `kival objects diff`.
#[derive(Debug, Clone, Args)]
pub struct ObjectsDiffCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Source version selector: UUID, `g`, `0`, or a negative offset such as `-5`.
    #[argx(long, allow_hyphen_values)]
    pub from: String,
    /// Destination version selector; defaults to current.
    #[argx(long, allow_hyphen_values)]
    pub to: Option<String>,
}

/// A parsed immutable object-version selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VersionSelector {
    /// One exact immutable version ID.
    Exact(Uuid),
    /// The first immutable version of the object.
    Genesis,
    /// An offset from the current-version snapshot; zero is current and negative values are older.
    Relative(i64),
}

/// Structured result of comparing two immutable object-version bodies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[argx(schema)]
pub struct ObjectDiffOutput {
    /// Object whose versions were compared.
    pub object_id: Uuid,
    /// Older or explicitly selected source version.
    pub from_version_id: Uuid,
    /// Newer or explicitly selected destination version.
    pub to_version_id: Uuid,
    /// Whether the two Markdown bodies differ byte-for-byte.
    pub changed: bool,
    /// Unified diff text, empty when the bodies are identical.
    pub diff: String,
}

/// Arguments for `kival objects restore`.
#[derive(Debug, Clone, Args)]
pub struct ObjectsRestoreCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Source version selector: UUID, `g`, `0`, or a negative offset such as `-5`.
    #[argx(long, allow_hyphen_values)]
    pub from: String,
}

/// Structured result of restoring an immutable object-version state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[argx(schema)]
pub struct ObjectRestoreOutput {
    /// Object whose current state was restored.
    pub object_id: Uuid,
    /// Immutable version whose state was selected.
    pub source_version_id: Uuid,
    /// Current version after the restore operation.
    pub version_id: Uuid,
    /// Monotonic current version number after the restore operation.
    pub version_number: i64,
    /// Whether restoring created a new version.
    pub changed: bool,
}

/// Arguments for `kival objects versions`.
#[derive(Debug, Args)]
#[argx(schema)]
pub struct ObjectVersionsCommand {
    /// The version command to run.
    #[argx(subcommand)]
    pub command: ObjectVersionsSubcommand,
}

/// The available `kival objects versions` commands.
#[derive(Debug, Subcommand)]
#[argx(schema)]
pub enum ObjectVersionsSubcommand {
    /// List object versions from newest to oldest.
    #[argx(name = "list")]
    List(ObjectVersionsListCommand),
    /// Get an object version.
    #[argx(name = "get")]
    Get(ObjectVersionsGetCommand),
    /// List server-resolved wikilinks authored in an immutable object version.
    #[argx(name = "wikilinks")]
    Wikilinks(ObjectVersionsWikilinksCommand),
}

/// Arguments for `kival objects versions list`.
#[derive(Debug, Args)]
pub struct ObjectVersionsListCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Maximum number of versions to return.
    #[argx(long, default = DEFAULT_LIST_LIMIT)]
    pub limit: Option<i64>,
    /// Opaque `response.next_cursor` from the previous page; reuse it with the same filters.
    #[argx(long)]
    pub cursor: Option<String>,
}

/// Arguments for `kival objects versions get`.
#[derive(Debug, Clone, Copy, Args)]
pub struct ObjectVersionsGetCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Version ID.
    pub version_id: Uuid,
}

/// Arguments for `kival objects versions wikilinks`.
#[derive(Debug, Clone, Copy, Args)]
pub struct ObjectVersionsWikilinksCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Version ID.
    pub version_id: Uuid,
}

#[argx(handler = run)]
impl ObjectsDiffCommand {
    /// Run `kival objects diff`.
    ///
    /// # Errors
    ///
    /// Returns an error if a selector is invalid or out of range, or if the selected versions
    /// cannot be read.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ObjectDiffOutput, CliError> {
        let from_raw = self.from.as_str();
        let to_raw = self.to.as_deref().unwrap_or("0");
        let from_selector = parse_version_selector(from_raw)?;
        let to_selector = parse_version_selector(to_raw)?;
        let client = authenticated_client(&ctx)?;
        let current = client.get_object(self.target.workspace_id, self.target.object_id).await?;
        let current_version = current.current_version.ok_or_else(|| {
            CliError::invalid_argument("object has no current version to compare")
        })?;

        let from_version = resolve_version_selector(
            &client,
            self.target,
            &current_version,
            from_selector,
            from_raw,
        )
        .await?;
        let to_version =
            resolve_version_selector(&client, self.target, &current_version, to_selector, to_raw)
                .await?;

        let result = object_diff_output(self.target.object_id, &from_version, &to_version);
        print_output(output, &result, || {
            print!("{}", result.diff);
        })?;
        Ok(result)
    }
}

#[argx(handler = run)]
impl ObjectsRestoreCommand {
    /// Run `kival objects restore`.
    ///
    /// # Errors
    ///
    /// Returns an error if the source selector is invalid, the object cannot be edited, the
    /// selected version cannot be read, or optimistic concurrency detects a newer current version.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ObjectRestoreOutput, CliError> {
        let selector = parse_version_selector(&self.from)?;
        let client = authenticated_client(&ctx)?;
        let current = client.get_object(self.target.workspace_id, self.target.object_id).await?;
        let current_version = restorable_version(&current)?;
        let source =
            resolve_version_selector(&client, self.target, current_version, selector, &self.from)
                .await?;
        if same_versioned_state(current_version, &source) {
            let refreshed =
                client.get_object(self.target.workspace_id, self.target.object_id).await?;
            let unchanged = validate_restore_noop(current_version.id, &refreshed)?;
            return Ok(print_restore_result(
                output,
                self.target.object_id,
                source.id,
                unchanged,
                false,
            )?);
        }

        let response = client
            .update_object(
                self.target.workspace_id,
                self.target.object_id,
                restore_update_request(current_version.id, &source),
            )
            .await?;
        let restored = response.current_version.as_ref().ok_or_else(|| {
            CliError::invalid_argument("object has no current version after restore")
        })?;

        Ok(print_restore_result(output, self.target.object_id, source.id, restored, true)?)
    }
}

/// Returns the current version when an object is eligible for restoration.
///
/// # Errors
///
/// Returns a stable CLI error when the object is archived, the caller lacks editor access, or the
/// object has no current version.
fn restorable_version(response: &ObjectResponse) -> Result<&ObjectVersion> {
    if response.object.status != ArchiveStatus::Active {
        return Err(CliError {
            code: CliErrorCode::ObjectArchived,
            message: "Archived objects cannot be restored from version history.".to_owned(),
            details: None,
        }
        .into());
    }

    if !matches!(response.effective_role, ObjectRole::Editor | ObjectRole::Admin) {
        return Err(CliError {
            code: CliErrorCode::PermissionDenied,
            message: "Object requires editor or admin access to restore a version.".to_owned(),
            details: None,
        }
        .into());
    }

    response.current_version.as_ref().ok_or_else(|| {
        CliError::invalid_argument("object has no current version to restore").into()
    })
}

/// Returns whether two immutable versions represent the same versioned object state.
fn same_versioned_state(left: &ObjectVersion, right: &ObjectVersion) -> bool {
    left.title == right.title && left.body == right.body && left.metadata == right.metadata
}

/// Validates that an unchanged restore still refers to the same current version.
///
/// # Errors
///
/// Returns an error if the object is no longer restorable or its current version changed after the
/// restore operation first read it.
fn validate_restore_noop(
    expected_current_version_id: Uuid,
    response: &ObjectResponse,
) -> Result<&ObjectVersion> {
    let current = restorable_version(response)?;
    if current.id != expected_current_version_id {
        return Err(CliError {
            code: CliErrorCode::VersionConflict,
            message: "Object changed while the restore operation was being validated.".to_owned(),
            details: Some(json!({
                "expected_current_version_id": expected_current_version_id,
                "actual_current_version_id": current.id,
            })),
        }
        .into());
    }

    Ok(current)
}

/// Builds an optimistic-concurrency update that restores one immutable version state.
fn restore_update_request(current_version_id: Uuid, source: &ObjectVersion) -> UpdateObjectRequest {
    UpdateObjectRequest {
        expected_current_version_id: current_version_id,
        title: Some(source.title.clone()),
        body: Some(source.body.clone()),
        metadata: Some(source.metadata.clone()),
    }
}

/// Prints a restore result in structured or concise human-readable form.
///
/// # Errors
///
/// Returns an error when structured output serialization fails.
fn print_restore_result(
    output: OutputMode,
    object_id: Uuid,
    source_version_id: Uuid,
    current_version: &ObjectVersion,
    changed: bool,
) -> Result<ObjectRestoreOutput> {
    let result = ObjectRestoreOutput {
        object_id,
        source_version_id,
        version_id: current_version.id,
        version_number: current_version.version_number,
        changed,
    };

    print_output(output, &result, || {
        let action = if result.changed { "restored" } else { "unchanged" };
        println!(
            "{} action={action} source_version={} version={} version_number={}",
            result.object_id, result.source_version_id, result.version_id, result.version_number,
        );
    })?;
    Ok(result)
}

/// Parses one object-version selector.
///
/// # Errors
///
/// Returns an invalid-argument error unless `value` is an immutable version UUID, `g`, `0`, or a
/// negative integer offset from current.
fn parse_version_selector(value: &str) -> Result<VersionSelector> {
    if value == "g" {
        return Ok(VersionSelector::Genesis);
    }

    if let Ok(offset) = value.parse::<i64>() {
        if offset <= 0 && (offset != 0 || value == "0") {
            return Ok(VersionSelector::Relative(offset));
        }
        return Err(CliError::invalid_argument(format!(
            "invalid version selector `{value}`; use a version UUID, `g`, `0`, or a negative offset such as `-3`",
        ))
        .into());
    }

    Uuid::parse_str(value).map(VersionSelector::Exact).map_err(|_| {
        CliError::invalid_argument(format!(
            "invalid version selector `{value}`; use a version UUID, `g`, `0`, or a negative offset such as `-3`",
        ))
        .into()
    })
}

/// Resolves one selector against a fixed current-version snapshot.
///
/// # Errors
///
/// Returns an out-of-range error for a relative selector older than genesis, or propagates an API
/// error when the selected immutable version cannot be read.
async fn resolve_version_selector(
    client: &KivalClient,
    target: ObjectTargetArgs,
    current: &ObjectVersion,
    selector: VersionSelector,
    raw_selector: &str,
) -> Result<ObjectVersion> {
    match selector {
        VersionSelector::Exact(version_id) if version_id == current.id => Ok(current.clone()),
        VersionSelector::Exact(version_id) => {
            Ok(client.get_object_version(target.workspace_id, target.object_id, version_id).await?)
        }
        VersionSelector::Genesis if current.version_number == 1 => Ok(current.clone()),
        VersionSelector::Genesis => {
            Ok(client.get_object_version(target.workspace_id, target.object_id, 1_i64).await?)
        }
        VersionSelector::Relative(0) => Ok(current.clone()),
        VersionSelector::Relative(offset) => {
            let version_number =
                relative_version_number(current.version_number, offset, raw_selector)?;

            Ok(client
                .get_object_version(target.workspace_id, target.object_id, version_number)
                .await?)
        }
    }
}

/// Resolves a relative selector to one monotonic object version number.
///
/// # Errors
///
/// Returns a stable out-of-range error when the requested offset predates genesis.
fn relative_version_number(
    current_version_number: i64,
    offset: i64,
    raw_selector: &str,
) -> Result<i64> {
    current_version_number
        .checked_add(offset)
        .filter(|number| *number >= 1)
        .ok_or_else(|| version_selector_out_of_range(raw_selector, current_version_number).into())
}

/// Builds a stable out-of-range error for a relative version selector.
fn version_selector_out_of_range(selector: &str, version_count: i64) -> CliError {
    let oldest_offset = 1 - version_count;
    let valid_range =
        if oldest_offset == 0 { "0".to_owned() } else { format!("{oldest_offset} through 0") };

    CliError {
        code: CliErrorCode::VersionSelectorOutOfRange,
        message: format!(
            "version selector `{selector}` is out of range; this object has {version_count} versions and valid relative selectors are {valid_range}; use `g` to select the genesis version",
        ),
        details: Some(json!({
            "selector": selector,
            "version_count": version_count,
            "oldest_offset": oldest_offset,
        })),
    }
}

/// Builds structured and unified-diff output for two immutable object versions.
fn object_diff_output(
    object_id: Uuid,
    from_version: &ObjectVersion,
    to_version: &ObjectVersion,
) -> ObjectDiffOutput {
    let relative_path = format!("{object_id}.md");
    let old_path = format!("a/{relative_path}");
    let new_path = format!("b/{relative_path}");
    let diff = unified_diff(
        &from_version.body,
        &to_version.body,
        &old_path,
        &new_path,
        DIFF_CONTEXT_LINES,
    );

    ObjectDiffOutput {
        object_id,
        from_version_id: from_version.id,
        to_version_id: to_version.id,
        changed: from_version.body != to_version.body,
        diff,
    }
}

impl ObjectVersionsCommand {
    /// Run `kival objects versions`.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected version command fails.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            ObjectVersionsSubcommand::List(command) => {
                command.run(ctx, output).await?;
            }
            ObjectVersionsSubcommand::Get(command) => {
                command.run(ctx, output).await?;
            }
            ObjectVersionsSubcommand::Wikilinks(command) => {
                command.run(ctx, output).await?;
            }
        }
        Ok(())
    }
}

#[argx(handler = run)]
impl ObjectVersionsListCommand {
    /// Run `kival objects versions list`.
    ///
    /// # Errors
    ///
    /// Returns an error if versions cannot be listed.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ListResponse<ObjectVersion>, CliError> {
        let client = authenticated_client(&ctx)?;
        let response = client
            .list_object_versions(
                self.target.workspace_id,
                self.target.object_id,
                &list_params(self.limit, self.cursor),
            )
            .await?;
        print_output(output, &response, || {
            if response.items.is_empty() {
                print_empty_list("versions");
            } else {
                for version in &response.items {
                    print_version_line(version);
                }
            }
            if let Some(cursor) = &response.next_cursor {
                println!("\nNext cursor: {cursor}");
            }
        })?;
        Ok(response)
    }
}

#[argx(handler = run)]
impl ObjectVersionsGetCommand {
    /// Run `kival objects versions get`.
    ///
    /// # Errors
    ///
    /// Returns an error if the version cannot be fetched.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ObjectVersion, CliError> {
        let client = authenticated_client(&ctx)?;
        let version = client
            .get_object_version(self.target.workspace_id, self.target.object_id, self.version_id)
            .await?;
        print_output(output, &version, || print_version_response(&version))?;
        Ok(version)
    }
}

#[argx(handler = run)]
impl ObjectVersionsWikilinksCommand {
    /// Run `kival objects versions wikilinks`.
    ///
    /// # Errors
    ///
    /// Returns an error if the version's server-resolved wikilinks cannot be fetched.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ObjectVersionWikilinksResponse, CliError> {
        let client = authenticated_client(&ctx)?;
        let items = client
            .get_object_version_wikilinks(
                self.target.workspace_id,
                self.target.object_id,
                self.version_id,
            )
            .await?;
        let response = ObjectVersionWikilinksResponse { items };
        print_output(output, &response, || {
            if response.items.is_empty() {
                print_empty_list("wikilinks");
            } else {
                for wikilink in &response.items {
                    let display = wikilink
                        .display_text
                        .as_deref()
                        .map(|value| format!(" display_text={}", quote_human_string(value)))
                        .unwrap_or_default();
                    let target = wikilink
                        .target_object_id
                        .map(|id| format!(" target_object_id={id}"))
                        .unwrap_or_else(|| " target_object_id=unresolved".to_owned());
                    println!(
                        "raw_target={}{}{}",
                        quote_human_string(&wikilink.raw_target),
                        display,
                        target,
                    );
                }
            }
        })?;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use kival_sdk::ObjectResource;
    use time::OffsetDateTime;

    use super::*;

    /// Verifies object diff selectors have a deliberately small revision vocabulary.
    #[test]
    fn version_selector_parses_exact_genesis_and_relative_versions() {
        let version_id = Uuid::from_u128(42);

        assert_eq!(
            parse_version_selector(&version_id.to_string()).unwrap(),
            VersionSelector::Exact(version_id)
        );
        assert_eq!(parse_version_selector("g").unwrap(), VersionSelector::Genesis);
        assert_eq!(parse_version_selector("0").unwrap(), VersionSelector::Relative(0));
        assert_eq!(parse_version_selector("-5").unwrap(), VersionSelector::Relative(-5));
    }

    /// Verifies positive offsets and malformed selector spellings are rejected.
    #[test]
    fn version_selector_rejects_forward_and_malformed_values() {
        for selector in ["1", "5", "-0", "genesis", "latest", "nope"] {
            let error =
                parse_version_selector(selector).unwrap_err().downcast::<CliError>().unwrap();
            assert_eq!(error.code, CliErrorCode::InvalidArgument, "selector={selector}");
        }
    }

    /// Verifies relative offsets resolve from current and never clamp past genesis.
    #[test]
    fn relative_version_selector_errors_when_it_predates_genesis() {
        assert_eq!(relative_version_number(3, 0, "0").unwrap(), 3);
        assert_eq!(relative_version_number(3, -1, "-1").unwrap(), 2);
        assert_eq!(relative_version_number(3, -2, "-2").unwrap(), 1);

        let error =
            relative_version_number(3, -4, "-4").unwrap_err().downcast::<CliError>().unwrap();
        assert_eq!(error.code, CliErrorCode::VersionSelectorOutOfRange);
        assert_eq!(
            error.details,
            Some(serde_json::json!({
                "selector": "-4",
                "version_count": 3,
                "oldest_offset": -2,
            }))
        );
    }

    /// Verifies restore copies the complete versioned state and guards the current version.
    #[test]
    fn restore_request_copies_state_with_optimistic_concurrency() {
        let object_id = Uuid::from_u128(1);
        let current_id = Uuid::from_u128(2);
        let mut source = object_version(object_id, Uuid::from_u128(3), 1, "old body");
        source.title = "Old title".to_owned();
        source.metadata = serde_json::json!({ "state": "old" });

        let request = restore_update_request(current_id, &source);

        assert_eq!(request.expected_current_version_id, current_id);
        assert_eq!(request.title.as_deref(), Some("Old title"));
        assert_eq!(request.body.as_deref(), Some("old body"));
        assert_eq!(request.metadata, Some(serde_json::json!({ "state": "old" })));
    }

    /// Verifies restore no-op detection compares only the state that is actually versioned.
    #[test]
    fn restore_state_equality_ignores_version_identity() {
        let object_id = Uuid::from_u128(1);
        let mut left = object_version(object_id, Uuid::from_u128(2), 1, "same body");
        let mut right = object_version(object_id, Uuid::from_u128(3), 7, "same body");
        left.title = "Same title".to_owned();
        right.title = "Same title".to_owned();
        left.metadata = serde_json::json!({ "same": true });
        right.metadata = serde_json::json!({ "same": true });

        assert!(same_versioned_state(&left, &right));

        right.body = "different".to_owned();
        assert!(!same_versioned_state(&left, &right));
    }

    /// Verifies unchanged restores are revalidated against the exact current version originally
    /// read.
    #[test]
    fn restore_noop_validation_rejects_concurrent_version_change() {
        let response = object_response(ArchiveStatus::Active, ObjectRole::Editor, true);
        let actual_version_id = response.current_version.as_ref().unwrap().id;

        assert!(validate_restore_noop(actual_version_id, &response).is_ok());

        let expected_version_id = Uuid::from_u128(99);
        let error = validate_restore_noop(expected_version_id, &response)
            .unwrap_err()
            .downcast::<CliError>()
            .unwrap();
        assert_eq!(error.code, CliErrorCode::VersionConflict);
        assert_eq!(
            error.details,
            Some(serde_json::json!({
                "expected_current_version_id": expected_version_id,
                "actual_current_version_id": actual_version_id,
            }))
        );
    }

    /// Verifies object diff output uses stable object paths and version identifiers.
    #[test]
    fn object_diff_output_uses_object_path_and_version_ids() {
        let object_id = Uuid::from_u128(1);
        let from = object_version(object_id, Uuid::from_u128(2), 1, "before\n");
        let to = object_version(object_id, Uuid::from_u128(3), 2, "after\n");

        let output = object_diff_output(object_id, &from, &to);

        assert_eq!(output.object_id, object_id);
        assert_eq!(output.from_version_id, from.id);
        assert_eq!(output.to_version_id, to.id);
        assert!(output.changed);
        assert!(output.diff.starts_with(&format!(
            "diff --git a/{object_id}.md b/{object_id}.md\n--- a/{object_id}.md\n+++ b/{object_id}.md\n"
        )));
    }

    /// Verifies version title and metadata changes do not turn a body-only diff into a change.
    #[test]
    fn object_diff_is_body_only() {
        let object_id = Uuid::from_u128(1);
        let mut from = object_version(object_id, Uuid::from_u128(2), 1, "same body\n");
        let mut to = object_version(object_id, Uuid::from_u128(3), 2, "same body\n");
        from.title = "Old title".to_owned();
        to.title = "New title".to_owned();
        from.metadata = serde_json::json!({ "state": "old" });
        to.metadata = serde_json::json!({ "state": "new" });

        let output = object_diff_output(object_id, &from, &to);

        assert!(!output.changed);
        assert!(output.diff.is_empty());
    }

    /// Verifies restore requires an active object with editor-or-admin authority.
    #[test]
    fn restorable_version_enforces_status_and_role() {
        let active_editor = object_response(ArchiveStatus::Active, ObjectRole::Editor, true);
        assert_eq!(restorable_version(&active_editor).unwrap().body, "body");

        let active_admin = object_response(ArchiveStatus::Active, ObjectRole::Admin, true);
        assert!(restorable_version(&active_admin).is_ok());

        let active_viewer = object_response(ArchiveStatus::Active, ObjectRole::Viewer, true);
        let viewer_error =
            restorable_version(&active_viewer).unwrap_err().downcast::<CliError>().unwrap();
        assert_eq!(viewer_error.code, CliErrorCode::PermissionDenied);

        let archived_admin = object_response(ArchiveStatus::Archived, ObjectRole::Admin, true);
        let archived_error =
            restorable_version(&archived_admin).unwrap_err().downcast::<CliError>().unwrap();
        assert_eq!(archived_error.code, CliErrorCode::ObjectArchived);
    }

    /// Builds an immutable object version for diff unit tests.
    fn object_version(
        object_id: Uuid,
        version_id: Uuid,
        version_number: i64,
        body: &str,
    ) -> ObjectVersion {
        ObjectVersion {
            id: version_id,
            object_id,
            version_number,
            title: "Title".to_owned(),
            body: body.to_owned(),
            metadata: serde_json::json!({}),
            created_by: None,
            created_by_username: None,
            created_by_display_name: None,
            created_by_workspace_role: None,
            created_by_object_role: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    /// Builds an object response for edit-preflight unit tests.
    fn object_response(
        status: ArchiveStatus,
        effective_role: ObjectRole,
        with_version: bool,
    ) -> ObjectResponse {
        let workspace_id = Uuid::now_v7();
        let object_id = Uuid::now_v7();
        let version_id = Uuid::now_v7();
        let now = OffsetDateTime::UNIX_EPOCH;

        ObjectResponse {
            object: ObjectResource {
                id: object_id,
                workspace_id,
                current_version_id: with_version.then_some(version_id),
                title: "Title".to_owned(),
                status,
                created_by: None,
                archived_by: None,
                created_at: now,
                updated_at: now,
                archived_at: (status == ArchiveStatus::Archived).then_some(now),
            },
            current_version: with_version.then(|| ObjectVersion {
                id: version_id,
                object_id,
                version_number: 1,
                title: "Title".to_owned(),
                body: "body".to_owned(),
                metadata: serde_json::json!({}),
                created_by: None,
                created_by_username: None,
                created_by_display_name: None,
                created_by_workspace_role: None,
                created_by_object_role: None,
                created_at: now,
            }),
            effective_role,
        }
    }
}
