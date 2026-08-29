//! Core object lifecycle and interactive editing commands.

use std::path::{Path, PathBuf};

use argx::{Args, ValueEnum};
use eyre::{Result, WrapErr};
use kival_cli::runner::CliContext;
use kival_sdk::{
    ArchiveStatus, CreateObjectRequest, ListResponse, ObjectListItem, ObjectListOrder,
    ObjectListParams, ObjectResponse, ObjectRole, ObjectVersion, UpdateObjectRequest,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::{
    ObjectTargetArgs,
    display::{print_object_line, print_object_response},
    document::{ObjectDocument, parse_object_document, render_object_document},
    io::{ensure_output_available, parse_body_file_path, resolve_body, write_output_file},
};
use crate::utils::error::CliResult;
use crate::utils::{
    args::{
        CliArchiveListStatus, DEFAULT_LIST_LIMIT, metadata_value,
        validate_flat_metadata, validate_flat_metadata_member,
    },
    credentials::authenticated_client,
    editor::edit_document,
    error::{CliError, CliErrorBody, CliErrorCode},
    input::{
        StructuredInputArgs, deserialize_optional_non_null, read_json_input,
        reject_conflicting_input,
    },
    output::{OutputMode, print_empty_list, print_output, quote_human_string},
};

/// Sort order accepted by `kival objects list`.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliObjectListOrder {
    /// Sort by creation time, newest first.
    Created,
    /// Sort by last update time, newest first.
    Updated,
}

impl From<CliObjectListOrder> for ObjectListOrder {
    fn from(order: CliObjectListOrder) -> Self {
        match order {
            CliObjectListOrder::Created => Self::Created,
            CliObjectListOrder::Updated => Self::Updated,
        }
    }
}

/// Arguments for `kival objects list`.
#[derive(Debug, Args)]
pub struct ObjectsListCommand {
    /// Workspace ID.

    pub workspace_id: Uuid,
    /// Archive status filter: active, archived, or all.
    #[argx(long, value_enum, default = CliArchiveListStatus::Active)]
    pub status: CliArchiveListStatus,
    /// Sort order: creation time or last update time, newest first.
    #[argx(long, value_enum, default = CliObjectListOrder::Created)]
    pub order: CliObjectListOrder,
    /// Restrict by the authenticated user's favorite state.
    #[argx(long)]
    pub favorited: Option<bool>,
    /// Restrict by the authenticated user's personal pin state.
    #[argx(long)]
    pub pinned: Option<bool>,
    /// Maximum number of objects to return.
    #[argx(long, default = DEFAULT_LIST_LIMIT)]
    pub limit: Option<i64>,
    /// Opaque `response.next_cursor` from the previous page; reuse it with the same filters.
    #[argx(long)]
    pub cursor: Option<String>,
}

/// Arguments for `kival objects get`.
#[derive(Debug, Clone, Copy, Args)]
pub struct ObjectsGetCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
}

/// Arguments for `kival objects body`.
#[derive(Debug, Args)]
pub struct ObjectsBodyCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Write the Markdown body to this file instead of stdout.
    #[argx(long, short = 'o')]
    pub output: Option<PathBuf>,
    /// Overwrite an existing output file.
    #[argx(long, requires = "output")]
    pub force: bool,
}

/// Arguments for `kival objects edit`.
#[derive(Debug, Clone, Copy, Args)]
pub struct ObjectsEditCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
}

/// Result of an external editor session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ObjectEditOutput {
    /// Object edited through the external editor.
    pub object_id: Uuid,
    /// Current version after the edit session.
    pub version_id: Uuid,
    /// Whether the editor changed versioned object state and created a new version.
    pub changed: bool,
}

/// Result of reading an object body.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ObjectBodyOutput {
    /// Object ID whose current body was read.
    pub object_id: Uuid,
    /// Current object version ID.
    pub version_id: Uuid,
    /// Markdown body when emitted through structured output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// File path when the body was written to a file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Number of UTF-8 bytes in the body.
    pub bytes_written: usize,
}

/// Semantic input for creating an object.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateObjectInput {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Object title.
    pub title: String,
    /// Initial object body.
    #[serde(default)]
    pub body: String,
    /// Initial flat object metadata.
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

/// Semantic input for updating object metadata.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateObjectInput {
    /// Exact current version the update was based on.
    pub expected_current_version_id: Uuid,
    /// New object title.
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    pub title: Option<String>,
    /// New object body.
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    pub body: Option<String>,
    /// New flat object metadata.
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    pub metadata: Option<Map<String, Value>>,
}

/// Arguments for `kival objects create`.
#[derive(Debug, Args)]
pub struct ObjectsCreateCommand {
    /// Structured input source.
    #[argx(flatten)]
    pub input_source: StructuredInputArgs,
    /// Workspace ID. Required unless supplied by `--input`.

    pub workspace_id: Option<Uuid>,
    /// Object title. Required unless supplied by `--input`.
    #[argx(long)]
    pub title: Option<String>,
    /// Initial object body, or `-` to read the body from standard input.
    #[argx(long, conflicts = "body_file")]
    pub body: Option<String>,
    /// Read the initial Markdown body from PATH.
    #[argx(long, value_parser = parse_body_file_path, conflicts = ["body", "input"])]
    pub body_file: Option<PathBuf>,
    /// Initial object metadata as a flat JSON object with scalar or scalar-list values.
    #[argx(long)]
    pub metadata: Option<String>,
}

/// Arguments for `kival objects update`.
#[derive(Debug, Args)]
pub struct ObjectsUpdateCommand {
    /// Structured input source.
    #[argx(flatten)]
    pub input_source: StructuredInputArgs,
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Exact current version the update was based on. Required unless supplied by `--input`.
    #[argx(long)]
    pub expected_current_version_id: Option<Uuid>,
    /// New object title.
    #[argx(long)]
    pub title: Option<String>,
    /// New object body, or `-` to read the body from standard input.
    #[argx(long, conflicts = "body_file")]
    pub body: Option<String>,
    /// Read the new Markdown body from PATH.
    #[argx(long, value_parser = parse_body_file_path, conflicts = ["body", "input"])]
    pub body_file: Option<PathBuf>,
    /// New object metadata as a flat JSON object with scalar or scalar-list values.
    #[argx(long)]
    pub metadata: Option<String>,
    /// Set one metadata key to a JSON scalar or scalar list, preserving all other metadata.
    ///
    /// Uses `KEY=JSON`; string values must be JSON strings, for example:
    /// `--metadata-set 'kind="note"'`.
    #[argx(long, conflicts = ["metadata", "input"])]
    pub metadata_set: Vec<String>,
    /// Remove one metadata key while preserving all other current metadata.
    #[argx(long, conflicts = ["metadata", "input"])]
    pub metadata_remove: Vec<String>,
}

/// Arguments for `kival objects archive`.
#[derive(Debug, Clone, Copy, Args)]
pub struct ObjectsArchiveCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
}

/// Arguments for `kival objects unarchive`.
#[derive(Debug, Clone, Copy, Args)]
pub struct ObjectsUnarchiveCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
}

#[argx(handler = run)]
impl ObjectsListCommand {
    /// Run `kival objects list`.
    ///
    /// # Errors
    ///
    /// Returns an error if objects cannot be listed.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> CliResult<ListResponse<ObjectListItem>> {
        let client = authenticated_client(&ctx)?;
        let response = client
            .list_objects(
                self.workspace_id,
                &ObjectListParams {
                    limit: Some(self.limit.unwrap_or(DEFAULT_LIST_LIMIT)),
                    cursor: self.cursor,
                    status: self.status.into(),
                    order: self.order.into(),
                    favorited: self.favorited,
                    pinned: self.pinned,
                },
            )
            .await?;
        print_output(output, &response, || {
            if response.items.is_empty() {
                print_empty_list("objects");
            } else {
                for object in &response.items {
                    print_object_line(object, None);
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
impl ObjectsGetCommand {
    /// Run `kival objects get`.
    ///
    /// # Errors
    ///
    /// Returns an error if the object cannot be fetched.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<ObjectResponse> {
        let client = authenticated_client(&ctx)?;
        let response = client.get_object(self.target.workspace_id, self.target.object_id).await?;
        print_output(output, &response, || print_object_response(&response, None))?;
        Ok(response)
    }
}

#[argx(handler = run)]
impl ObjectsBodyCommand {
    /// Run `kival objects body`.
    ///
    /// # Errors
    ///
    /// Returns an error if the object cannot be fetched or the output file cannot be written.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<ObjectBodyOutput> {
        if let Some(path) = self.output.as_deref() {
            ensure_output_available(path, self.force)?;
        }

        let client = authenticated_client(&ctx)?;
        let response = client.get_object(self.target.workspace_id, self.target.object_id).await?;
        let version = response
            .current_version
            .ok_or_else(|| CliError::invalid_argument("object has no current version"))?;
        let bytes_written = version.body.len();

        if let Some(path) = self.output {
            write_output_file(&path, version.body.as_bytes(), self.force)?;
            let result = ObjectBodyOutput {
                object_id: self.target.object_id,
                version_id: version.id,
                body: None,
                output: Some(path.display().to_string()),
                bytes_written,
            };
            print_output(output, &result, || {
                println!(
                    "{} version={} action=written output={} bytes_written={}",
                    result.object_id,
                    result.version_id,
                    quote_human_string(result.output.as_deref().unwrap_or_default()),
                    result.bytes_written,
                );
            })?;
            return Ok(result);
        }

        let result = ObjectBodyOutput {
            object_id: self.target.object_id,
            version_id: version.id,
            body: Some(version.body),
            output: None,
            bytes_written,
        };
        print_output(output, &result, || {
            if let Some(body) = &result.body {
                print!("{body}");
            }
        })?;
        Ok(result)
    }
}

#[argx(handler = run)]
impl ObjectsEditCommand {
    /// Run `kival objects edit`.
    ///
    /// # Errors
    ///
    /// Returns an error when the object cannot be read or edited, the external editor fails, or
    /// the object changes before the edited state can be stored.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<ObjectEditOutput> {
        let client = authenticated_client(&ctx)?;
        let current = client.get_object(self.target.workspace_id, self.target.object_id).await?;
        let version = editable_version(&current)?;
        let initial = object_document(version)?;
        let rendered = render_object_document(&initial);
        let edited = edit_document(self.target.object_id, &rendered)?;
        let parsed = match parse_object_document(edited.document()) {
            Ok(parsed) => parsed,
            Err(error) => {
                return Err(local_edit_error(&error.to_string(), edited.path()).into());
            }
        };
        let request = match update_edit_request(version.id, &initial, parsed) {
            Ok(Some(request)) => request,
            Ok(None) => {
                edited.discard()?;
                return print_edit_result(output, &current, false);
            }
            Err(error) => {
                return Err(local_edit_error(&error.to_string(), edited.path()).into());
            }
        };
        let response = match client
            .update_object(self.target.workspace_id, self.target.object_id, request)
            .await
        {
            Ok(response) => response,
            Err(error) => {
                let body = CliErrorBody::from_client_error(&error);
                return Err(edit_recovery_error(&body, edited.path()).into());
            }
        };

        edited.discard().wrap_err(
            "object was updated, but the temporary object document could not be removed",
        )?;
        print_edit_result(output, &response, true)
    }
}

/// Returns the current version when an object is eligible for interactive editing.
///
/// # Errors
///
/// Returns a stable CLI error when the object is archived, the caller lacks editor access, or the
/// object has no current version.
fn editable_version(response: &ObjectResponse) -> Result<&ObjectVersion> {
    if response.object.status != ArchiveStatus::Active {
        return Err(CliError {
            code: CliErrorCode::ObjectArchived,
            message: "Archived objects cannot be edited.".to_owned(),
            details: None,
        }
        .into());
    }

    if !matches!(response.effective_role, ObjectRole::Editor | ObjectRole::Admin) {
        return Err(CliError {
            code: CliErrorCode::PermissionDenied,
            message: "Object requires editor or admin access to edit.".to_owned(),
            details: None,
        }
        .into());
    }

    response
        .current_version
        .as_ref()
        .ok_or_else(|| CliError::invalid_argument("object has no current version to edit").into())
}

/// Builds the external-editor projection from the current object version.
fn object_document(version: &ObjectVersion) -> Result<ObjectDocument> {
    let metadata = version.metadata.as_object().cloned().ok_or_else(|| {
        CliError::invalid_argument("current object metadata is not a JSON object")
    })?;
    Ok(ObjectDocument { title: version.title.clone(), metadata, body: version.body.clone() })
}

/// Validates edited state and builds an optimistic-concurrency update when it changed.
fn update_edit_request(
    version_id: Uuid,
    initial: &ObjectDocument,
    edited: ObjectDocument,
) -> Result<Option<UpdateObjectRequest>> {
    if edited.title.trim().is_empty() {
        return Err(CliError::invalid_argument(
            "object front-matter field `title` must not be blank",
        )
        .into());
    }
    validate_flat_metadata(&edited.metadata)?;

    let title = (edited.title != initial.title).then_some(edited.title);
    let body = (edited.body != initial.body).then_some(edited.body);
    let metadata = (edited.metadata != initial.metadata).then_some(Value::Object(edited.metadata));
    if title.is_none() && body.is_none() && metadata.is_none() {
        return Ok(None);
    }

    Ok(Some(UpdateObjectRequest { expected_current_version_id: version_id, title, body, metadata }))
}

/// Builds a stable local-validation error that retains the recovery path.
fn local_edit_error(message: &str, path: &Path) -> CliError {
    CliError {
        code: CliErrorCode::InvalidArgument,
        message: format!("{message} Edited object remains at `{}`.", path.display()),
        details: Some(json!({
            "recovery_path": path.display().to_string(),
        })),
    }
}

/// Builds a stable CLI error that retains the local recovery path.
fn edit_recovery_error(body: &CliErrorBody, path: &Path) -> CliError {
    CliError {
        code: body.code,
        message: format!("{} Edited object remains at `{}`.", body.message, path.display()),
        details: Some(json!({
            "recovery_path": path.display().to_string(),
        })),
    }
}

/// Builds the structured result of an interactive edit operation.
///
/// # Errors
///
/// Returns an error if the object response unexpectedly lacks a current version.
fn object_edit_output(response: &ObjectResponse, changed: bool) -> Result<ObjectEditOutput> {
    let version_id =
        response.current_version.as_ref().map(|version| version.id).ok_or_else(|| {
            CliError::invalid_argument("object has no current version after edit")
        })?;

    Ok(ObjectEditOutput { object_id: response.object.id, version_id, changed })
}

/// Prints the result of an interactive edit operation.
///
/// # Errors
///
/// Returns an error if the result cannot be built or structured output serialization fails.
fn print_edit_result(
    output: OutputMode,
    response: &ObjectResponse,
    changed: bool,
) -> Result<ObjectEditOutput> {
    let result = object_edit_output(response, changed)?;
    print_output(output, &result, || {
        let action = if result.changed { "updated" } else { "unchanged" };
        println!("{} action={action} version={}", result.object_id, result.version_id);
    })?;
    Ok(result)
}

#[argx(handler = run)]
impl ObjectsCreateCommand {
    /// Run `kival objects create`.
    ///
    /// # Errors
    ///
    /// Returns an error if the object cannot be created.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<ObjectResponse> {
        let input = self.into_input()?;
        let title = input.title.trim();
        if title.is_empty() {
            return Err(CliError::invalid_argument("title must not be empty").into());
        }
        let client = authenticated_client(&ctx)?;
        let response = client
            .create_object(
                input.workspace_id,
                CreateObjectRequest {
                    title: title.to_owned(),
                    body: input.body,
                    metadata: Value::Object(input.metadata),
                },
            )
            .await?;
        print_output(output, &response, || print_object_response(&response, Some("created")))?;
        Ok(response)
    }

    /// Resolves semantic create input from either `--input` or CLI payload fields.
    fn into_input(self) -> Result<CreateObjectInput> {
        reject_conflicting_input(
            &self.input_source.input,
            &[
                ("workspace_id", self.workspace_id.is_some()),
                ("title", self.title.is_some()),
                ("body", self.body.is_some()),
                ("body_file", self.body_file.is_some()),
                ("metadata", self.metadata.is_some()),
            ],
        )?;

        if let Some(input) = self.input_source.input {
            let input: CreateObjectInput = read_json_input(input)?;
            validate_flat_metadata(&input.metadata)?;
            return Ok(input);
        }

        let workspace_id = self
            .workspace_id
            .ok_or_else(|| CliError::invalid_argument("workspace ID is required"))?;
        let title = self.title.ok_or_else(|| CliError::invalid_argument("title is required"))?;
        let metadata = metadata_value(self.metadata.as_deref())?;
        let Value::Object(metadata) = metadata else {
            unreachable!("metadata_value returns an object")
        };

        let body = resolve_body(self.body, self.body_file)?.unwrap_or_default();

        Ok(CreateObjectInput { workspace_id, title, body, metadata })
    }
}

#[argx(handler = run)]
impl ObjectsUpdateCommand {
    /// Run `kival objects update`.
    ///
    /// # Errors
    ///
    /// Returns an error if the object cannot be updated.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<ObjectResponse> {
        let target = self.target;
        let metadata_sets = parse_metadata_sets(&self.metadata_set)?;
        let metadata_remove = self.metadata_remove.clone();
        let has_metadata_mutations = !metadata_sets.is_empty() || !metadata_remove.is_empty();
        let mut input = self.into_input()?;
        if input.title.is_none()
            && input.body.is_none()
            && input.metadata.is_none()
            && !has_metadata_mutations
        {
            return Err(CliError::invalid_argument("at least one field must be provided").into());
        }
        if input.title.as_deref().is_some_and(|title| title.trim().is_empty()) {
            return Err(CliError::invalid_argument("title must not be empty").into());
        }

        let client = authenticated_client(&ctx)?;
        if has_metadata_mutations {
            let response = client.get_object(target.workspace_id, target.object_id).await?;
            let version = response
                .current_version
                .ok_or_else(|| CliError::invalid_argument("object has no current version"))?;
            if version.id != input.expected_current_version_id {
                return Err(CliError {
                    code: CliErrorCode::VersionConflict,
                    message: "Object changed since the expected version.".to_owned(),
                    details: Some(json!({
                        "expected_current_version_id": input.expected_current_version_id,
                        "actual_current_version_id": version.id,
                    })),
                }
                .into());
            }
            let Value::Object(mut metadata) = version.metadata else {
                return Err(eyre::eyre!("current object metadata is not a JSON object"));
            };
            apply_metadata_mutations(&mut metadata, &metadata_sets, &metadata_remove)?;
            validate_flat_metadata(&metadata)?;
            input.metadata = Some(metadata);
        }

        let request = UpdateObjectRequest {
            expected_current_version_id: input.expected_current_version_id,
            title: input.title.map(|title| title.trim().to_owned()),
            body: input.body,
            metadata: input.metadata.map(Value::Object),
        };
        let response = client.update_object(target.workspace_id, target.object_id, request).await?;
        print_output(output, &response, || print_object_response(&response, Some("updated")))?;
        Ok(response)
    }

    /// Resolves semantic update input from either `--input` or CLI payload fields.
    fn into_input(self) -> Result<UpdateObjectInput> {
        if self.metadata.is_some()
            && (!self.metadata_set.is_empty() || !self.metadata_remove.is_empty())
        {
            return Err(CliError::invalid_argument(
                "--metadata cannot be combined with --metadata-set or --metadata-remove",
            )
            .into());
        }

        reject_conflicting_input(
            &self.input_source.input,
            &[
                ("expected_current_version_id", self.expected_current_version_id.is_some()),
                ("title", self.title.is_some()),
                ("body", self.body.is_some()),
                ("body_file", self.body_file.is_some()),
                ("metadata", self.metadata.is_some()),
                ("metadata_set", !self.metadata_set.is_empty()),
                ("metadata_remove", !self.metadata_remove.is_empty()),
            ],
        )?;

        if let Some(input) = self.input_source.input {
            let input: UpdateObjectInput = read_json_input(input)?;
            if let Some(metadata) = input.metadata.as_ref() {
                validate_flat_metadata(metadata)?;
            }
            if input.title.is_none() && input.body.is_none() && input.metadata.is_none() {
                return Err(CliError::input_invalid_value(
                    "at least one input field must be provided".into(),
                )
                .into());
            }
            return Ok(input);
        }

        let metadata = match self.metadata.as_deref() {
            Some(metadata) => {
                let Value::Object(metadata) = metadata_value(Some(metadata))? else {
                    unreachable!("metadata_value returns an object")
                };
                Some(metadata)
            }
            None => None,
        };

        let body = resolve_body(self.body, self.body_file)?;

        let expected_current_version_id = self
            .expected_current_version_id
            .ok_or_else(|| CliError::invalid_argument("expected current version ID is required"))?;

        Ok(UpdateObjectInput { expected_current_version_id, title: self.title, body, metadata })
    }
}

/// Parses repeated `--metadata-set KEY=JSON` arguments without interpreting metadata keys.
fn parse_metadata_sets(values: &[String]) -> Result<Vec<(String, Value)>> {
    values
        .iter()
        .map(|assignment| {
            let (key, raw_value) = assignment
                .split_once('=')
                .ok_or_else(|| CliError::invalid_argument("--metadata-set must use KEY=JSON"))?;
            let value = serde_json::from_str(raw_value).map_err(|error| {
                CliError::invalid_argument(format!(
                    "metadata value for key {key:?} must be valid JSON: {error}"
                ))
            })?;
            validate_flat_metadata_member(key, &value)?;
            Ok((key.to_owned(), value))
        })
        .collect()
}

/// Applies exact-key metadata mutations to a complete metadata object.
fn apply_metadata_mutations(
    metadata: &mut Map<String, Value>,
    sets: &[(String, Value)],
    removes: &[String],
) -> Result<()> {
    if let Some((key, _)) = sets.iter().find(|(key, _)| removes.contains(key)) {
        return Err(CliError::invalid_argument(format!(
            "metadata key {key:?} cannot be both set and removed"
        ))
        .into());
    }

    for (key, value) in sets {
        metadata.insert(key.clone(), value.clone());
    }
    for key in removes {
        metadata.remove(key);
    }

    Ok(())
}

#[argx(handler = run)]
impl ObjectsArchiveCommand {
    /// Run `kival objects archive`.
    ///
    /// # Errors
    ///
    /// Returns an error if the object cannot be archived.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<ObjectResponse> {
        let client = authenticated_client(&ctx)?;
        let response =
            client.archive_object(self.target.workspace_id, self.target.object_id).await?;
        print_output(output, &response, || {
            print_object_line(&response.object, Some("archived"));
        })?;
        Ok(response)
    }
}

#[argx(handler = run)]
impl ObjectsUnarchiveCommand {
    /// Run `kival objects unarchive`.
    ///
    /// # Errors
    ///
    /// Returns an error if the object cannot be unarchived.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<ObjectResponse> {
        let client = authenticated_client(&ctx)?;
        let response =
            client.unarchive_object(self.target.workspace_id, self.target.object_id).await?;
        print_output(output, &response, || {
            print_object_line(&response.object, Some("unarchived"));
        })?;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use kival_sdk::ObjectResource;
    use time::OffsetDateTime;

    use super::*;

    #[test]
    fn update_object_input_rejects_null_title() {
        let null_title = serde_json::from_value::<UpdateObjectInput>(json!({
            "expected_current_version_id": Uuid::nil(),
            "title": null,
        }))
        .expect_err("title null should be rejected during deserialization");
        assert_eq!(null_title.classify(), serde_json::error::Category::Data);
    }

    /// Verifies create input reads Markdown from a file without altering it.
    #[test]
    fn create_input_reads_body_file_exactly() {
        let path = temp_content_path("create");
        let expected = "# Title\r\n\r\nbody  \n";
        fs::write(&path, expected.as_bytes()).unwrap();

        let input = ObjectsCreateCommand {
            input_source: StructuredInputArgs { input: None },
            workspace_id: Some(Uuid::nil()),
            title: Some("Title".to_owned()),
            body: None,
            body_file: Some(path.clone()),
            metadata: None,
        }
        .into_input()
        .unwrap();

        assert_eq!(input.body, expected);
        let _ = fs::remove_file(path);
    }

    /// Verifies update input treats a Markdown file as a body-only replacement.
    #[test]
    fn update_input_reads_body_file_as_body() {
        let path = temp_content_path("update");
        let expected = "updated without final newline";
        fs::write(&path, expected.as_bytes()).unwrap();

        let input = ObjectsUpdateCommand {
            input_source: StructuredInputArgs { input: None },
            target: ObjectTargetArgs { workspace_id: Uuid::nil(), object_id: Uuid::nil() },
            expected_current_version_id: Some(Uuid::nil()),
            title: None,
            body: None,
            body_file: Some(path.clone()),
            metadata: None,
            metadata_set: Vec::new(),
            metadata_remove: Vec::new(),
        }
        .into_input()
        .unwrap();

        assert_eq!(input.expected_current_version_id, Uuid::nil());
        assert_eq!(input.body.as_deref(), Some(expected));
        assert!(input.title.is_none());
        assert!(input.metadata.is_none());
        let _ = fs::remove_file(path);
    }

    /// Verifies metadata mutations preserve unrelated keys and exact key spelling.
    #[test]
    fn metadata_mutations_preserve_unrelated_metadata() {
        let sets =
            parse_metadata_sets(&["explored=true".to_owned(), r#" spaced ="value""#.to_owned()])
                .unwrap();
        let Value::Object(mut metadata) = json!({
            "kind": "replacement-target",
            "timestamp": "20260729T094036Z",
            "remove-me": true,
        }) else {
            unreachable!("test metadata is an object")
        };

        apply_metadata_mutations(&mut metadata, &sets, &["remove-me".to_owned()]).unwrap();

        assert_eq!(
            Value::Object(metadata),
            json!({
                "kind": "replacement-target",
                "timestamp": "20260729T094036Z",
                "explored": true,
                " spaced ": "value",
            })
        );
    }

    /// Verifies invalid JSON metadata values are rejected before any object is fetched.
    #[test]
    fn metadata_set_rejects_invalid_json() {
        let error = parse_metadata_sets(&["key=not-json".to_owned()])
            .unwrap_err()
            .downcast::<CliError>()
            .unwrap();

        assert_eq!(error.code, CliErrorCode::InvalidArgument);
    }

    /// Verifies metadata set assignments require an explicit key/value separator.
    #[test]
    fn metadata_set_requires_assignment_syntax() {
        assert!(parse_metadata_sets(&["key".to_owned()]).is_err());
    }

    /// Verifies one key cannot be both set and removed in the same update.
    #[test]
    fn metadata_mutations_reject_set_remove_overlap() {
        let sets = vec![("status".to_owned(), json!("draft"))];
        let mut metadata = Map::new();

        assert!(apply_metadata_mutations(&mut metadata, &sets, &["status".to_owned()]).is_err());
    }

    /// Verifies metadata set values use JSON types rather than string guessing.
    #[test]
    fn metadata_set_values_are_json() {
        let sets = parse_metadata_sets(&[
            "boolean=true".to_owned(),
            "number=2".to_owned(),
            r#"string="2""#.to_owned(),
        ])
        .unwrap();

        assert_eq!(sets[0].1, json!(true));
        assert_eq!(sets[1].1, json!(2));
        assert_eq!(sets[2].1, json!("2"));
    }

    /// Verifies the mutation convenience cannot introduce nested metadata objects.
    #[test]
    fn metadata_set_rejects_nested_object_values() {
        assert!(parse_metadata_sets(&[r#"config={"enabled":true}"#.to_owned()]).is_err());
    }

    /// Verifies the mutation convenience cannot introduce nested metadata arrays.
    #[test]
    fn metadata_set_rejects_nested_array_values() {
        assert!(parse_metadata_sets(&["matrix=[[1,2],[3,4]]".to_owned()]).is_err());
    }

    /// Verifies a one-dimensional list of JSON scalar values remains valid metadata.
    #[test]
    fn metadata_set_accepts_scalar_list_values() {
        let sets = parse_metadata_sets(&[r#"aliases=["one",2,true,null]"#.to_owned()]).unwrap();
        assert_eq!(sets[0].1, json!(["one", 2, true, null]));
    }

    /// Verifies only active objects with editor-or-admin authority are editable.
    #[test]
    fn editable_version_enforces_status_and_role() {
        let active_editor = object_response(ArchiveStatus::Active, ObjectRole::Editor, true);
        assert_eq!(editable_version(&active_editor).unwrap().body, "body");

        let active_admin = object_response(ArchiveStatus::Active, ObjectRole::Admin, true);
        assert!(editable_version(&active_admin).is_ok());

        let active_viewer = object_response(ArchiveStatus::Active, ObjectRole::Viewer, true);
        let viewer_error =
            editable_version(&active_viewer).unwrap_err().downcast::<CliError>().unwrap();
        assert_eq!(viewer_error.code, CliErrorCode::PermissionDenied);

        let archived_admin = object_response(ArchiveStatus::Archived, ObjectRole::Admin, true);
        let archived_error =
            editable_version(&archived_admin).unwrap_err().downcast::<CliError>().unwrap();
        assert_eq!(archived_error.code, CliErrorCode::ObjectArchived);
    }

    /// Verifies objects without a current version are rejected before opening an editor.
    #[test]
    fn editable_version_requires_current_version() {
        let response = object_response(ArchiveStatus::Active, ObjectRole::Editor, false);
        assert!(editable_version(&response).is_err());
    }

    /// Verifies editor saves include changed versioned fields and the exact base version.
    #[test]
    fn update_edit_request_uses_expected_current_version() {
        let response = object_response(ArchiveStatus::Active, ObjectRole::Editor, true);
        let version = response.current_version.as_ref().unwrap();
        let initial = object_document(version).unwrap();
        let mut edited = initial.clone();
        edited.title = "Edited title".to_owned();
        edited.body = "edited".to_owned();
        edited.metadata.insert("status".to_owned(), json!("draft"));

        let request = update_edit_request(version.id, &initial, edited).unwrap().unwrap();

        assert_eq!(request.expected_current_version_id, version.id);
        assert_eq!(request.title.as_deref(), Some("Edited title"));
        assert_eq!(request.body.as_deref(), Some("edited"));
        assert_eq!(request.metadata, Some(json!({"status": "draft"})));
    }

    /// Verifies a semantically unchanged document does not create a redundant version.
    #[test]
    fn update_edit_request_detects_no_op() {
        let response = object_response(ArchiveStatus::Active, ObjectRole::Editor, true);
        let version = response.current_version.as_ref().unwrap();
        let initial = object_document(version).unwrap();

        assert!(update_edit_request(version.id, &initial, initial.clone()).unwrap().is_none());
    }

    /// Verifies edit output reports whether the body created a new version.
    #[test]
    fn object_edit_output_reports_object_version_and_change_state() {
        let response = object_response(ArchiveStatus::Active, ObjectRole::Editor, true);
        let output = object_edit_output(&response, true).unwrap();

        assert_eq!(output.object_id, response.object.id);
        assert_eq!(output.version_id, response.current_version.as_ref().unwrap().id);
        assert!(output.changed);
    }

    /// Verifies update failures preserve both the stable error code and recovery path.
    #[test]
    fn edit_recovery_error_includes_edited_markdown_path() {
        let path = Path::new("/tmp/kival-recovery.md");
        let error = edit_recovery_error(
            &CliErrorBody {
                code: CliErrorCode::VersionConflict,
                message: "Object changed since it was read.".to_owned(),
                details: None,
            },
            path,
        );

        assert_eq!(error.code, CliErrorCode::VersionConflict);
        assert!(error.message.contains("/tmp/kival-recovery.md"));
        assert_eq!(
            error.details,
            Some(serde_json::json!({ "recovery_path": "/tmp/kival-recovery.md" }))
        );
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

    /// Builds a unique-enough temporary path for object-body unit tests.
    fn temp_content_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kival-object-body-{name}-{}", std::process::id()))
    }
}
