//! Object commands.

mod attachments;
mod display;
mod document;
mod error;
mod events;
mod grants;
mod history;
mod io;
mod lifecycle;
mod relations;

use argx::{Args, Subcommand};
pub use attachments::{
    ObjectAttachmentContentOutput, ObjectAttachmentsCommand, ObjectAttachmentsContentCommand,
    ObjectAttachmentsGetCommand, ObjectAttachmentsListCommand, ObjectAttachmentsReuseCommand,
    ObjectAttachmentsSubcommand, ObjectAttachmentsUploadCommand,
};
pub(crate) use error::{
    ObjectCommandError, ObjectError, ObjectErrorCode, ObjectHistoryError, ObjectHistoryErrorCode,
    ObjectScopedErrorCode, object_error_codes,
};
pub use events::ObjectEventsCommand;
use eyre::Result;
pub use grants::{
    ObjectGrantsCommand, ObjectGrantsCreateCommand, ObjectGrantsListCommand,
    ObjectGrantsRevokeCommand, ObjectGrantsSubcommand, ObjectGrantsUpdateCommand,
};
pub use history::{
    ObjectDiffOutput, ObjectRestoreOutput, ObjectVersionsCommand, ObjectVersionsGetCommand,
    ObjectVersionsListCommand, ObjectVersionsSubcommand, ObjectVersionsWikilinksCommand,
    ObjectsDiffCommand, ObjectsRestoreCommand,
};
use kival_cli::runner::CliContext;
pub use lifecycle::{
    CliObjectListOrder, CreateObjectInput, ObjectBodyOutput, ObjectEditOutput,
    ObjectsArchiveCommand, ObjectsBodyCommand, ObjectsCreateCommand, ObjectsEditCommand,
    ObjectsGetCommand, ObjectsListCommand, ObjectsUnarchiveCommand, ObjectsUpdateCommand,
    UpdateObjectInput,
};
pub use relations::{
    CliObjectGraphDirection, ObjectEdgesCommand, ObjectEdgesCreateCommand, ObjectEdgesGetCommand,
    ObjectEdgesListCommand, ObjectEdgesRevokeCommand, ObjectEdgesSubcommand,
    ObjectsBacklinksCommand, ObjectsGraphCommand,
};
use uuid::Uuid;

use crate::utils::{error::erase_command_error, output::OutputMode};

/// Arguments for `kival objects`.
#[derive(Debug, Args)]
#[argx(schema)]
pub struct ObjectsCommand {
    /// The object command to run.
    #[argx(subcommand)]
    pub command: ObjectsSubcommand,
}

/// The available `kival objects` commands.
#[derive(Debug, Subcommand)]
#[argx(schema)]
pub enum ObjectsSubcommand {
    /// List visible objects in a workspace, newest first.
    ///
    /// Objects are ordered by creation time by default. Use `--order updated` to order by last
    /// update time instead. Active objects are returned by default; use `--status` to select
    /// archived objects or both lifecycle states.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["objects:read"],
        })
    )]
    List(ObjectsListCommand),
    /// Get an object by ID within a workspace.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["objects:read"],
        })
    )]
    Get(ObjectsGetCommand),
    /// Read the current Markdown body of an object.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "writesLocalFiles": true,
            "requiredScopes": ["objects:read"],
        })
    )]
    Body(ObjectsBodyCommand),
    /// Compare Markdown bodies between immutable object versions.
    ///
    /// Version selectors are an exact immutable version UUID, `g` for genesis, `0` for the current
    /// version, or `-N` for N versions before current. `--from` is required and `--to` defaults to
    /// current. Relative selectors resolve against the same current-version snapshot, and
    /// out-of-range offsets fail rather than clamping.
    ///
    /// Text output is a standard unified diff with three context lines, suitable for tools such as
    /// `delta`, `patch`, and `git apply`.
    ///
    /// Examples: `kival objects diff <WORKSPACE_ID> <OBJECT_ID> --from -1`,
    /// `kival objects diff <WORKSPACE_ID> <OBJECT_ID> --from g --to -2`, or pipe the result to
    /// `delta`.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["objects:read"],
        })
    )]
    Diff(ObjectsDiffCommand),
    /// Restore an immutable version as a new current version of the same object.
    ///
    /// Version selectors are an exact immutable version UUID, `g` for genesis, `0` for current, or
    /// `-N` for N versions before current. Restore never rewrites history: the selected title,
    /// body, and metadata are appended as a new current version. Permissions and other
    /// non-versioned object state are unchanged. If the selected state is already current, no
    /// redundant version is created.
    ///
    /// Examples: `kival objects restore <WORKSPACE_ID> <OBJECT_ID> --from -1`, `--from g`, or
    /// `--from <VERSION_ID>`.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["objects:write"],
        })
    )]
    Restore(ObjectsRestoreCommand),
    /// Edit the current object through Markdown and front matter in an external text editor.
    ///
    /// Editable fields are title, metadata mapping, and Markdown body. A changed document creates
    /// one new version; an unchanged document exits without creating a version. Editor selection is
    /// `KIVAL_EDITOR`, `VISUAL`, `EDITOR`, then the platform default. The editor must block until
    /// editing is complete, for example `code --wait`.
    ///
    /// Examples: `kival objects edit <WORKSPACE_ID> <OBJECT_ID>` or
    /// `KIVAL_EDITOR="code --wait" kival objects edit <WORKSPACE_ID> <OBJECT_ID>`.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["objects:write"],
        })
    )]
    Edit(ObjectsEditCommand),
    /// Create an object in a workspace.
    ///
    /// With `--input`, the JSON object requires `workspace_id` (UUID) and `title` (string).
    /// Optional properties are `body` (string; defaults to empty) and `metadata` (flat JSON
    /// object; defaults to empty). Unknown properties are rejected. The JSON document replaces
    /// the corresponding command-line payload fields.
    ///
    /// Examples: `kival objects create <WORKSPACE_ID> --title "Title"`, add
    /// `--body-file note.md`, pipe Markdown with `--body -`, or provide structured input with
    /// `--input object.json` / `--input -`.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["objects:write"],
        })
    )]
    Create(ObjectsCreateCommand),
    /// Update an object, recording changed state as a new version.
    ///
    /// Omitted title, body, and metadata fields are inherited from the current version. Every
    /// update must name the exact current version it was based on with
    /// `--expected-current-version-id`; Kival returns a conflict if that version is no longer
    /// current. Metadata is flat: values are JSON scalars or one-dimensional scalar arrays.
    /// `--metadata` replaces the complete metadata object, while `--metadata-set` and
    /// `--metadata-remove` preserve other metadata.
    ///
    /// With `--input`, the JSON object requires `expected_current_version_id` (UUID) and at least
    /// one of `title` (string), `body` (string), or `metadata` (flat JSON object). Omitted
    /// properties remain unchanged and unknown properties are rejected. `metadata` replaces the
    /// complete metadata object.
    ///
    /// Examples: update `--title`, `--body`, or `--metadata-set explored=true`, or provide
    /// structured input with `--input update.json`.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["objects:write"],
        })
    )]
    Update(ObjectsUpdateCommand),
    /// Archive an object while retaining its stored history.
    ///
    /// Archiving changes the object's lifecycle state; it does not delete its versions.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": true,
            "idempotent": false,
            "requiresConfirmation": true,
            "requiredScopes": ["objects:write"],
        })
    )]
    Archive(ObjectsArchiveCommand),
    /// Restore an archived object to active status.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["objects:write"],
        })
    )]
    Unarchive(ObjectsUnarchiveCommand),
    /// Inspect object version history.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["objects:read"],
        })
    )]
    Versions(ObjectVersionsCommand),
    /// Manage object edges.
    Edges(ObjectEdgesCommand),
    /// List visible incoming relationships and textual references to an object.
    ///
    /// Explicit backlinks are active edges whose target is this object. Textual backlinks are
    /// resolved references from current object versions.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["graph:read"],
        })
    )]
    Backlinks(ObjectsBacklinksCommand),
    /// Manage object attachments.
    Attachments(ObjectAttachmentsCommand),
    /// Manage object grants.
    Grants(ObjectGrantsCommand),
    /// List events for an object.
    ///
    /// Sequence bounds are exclusive. Events are returned in ascending order by default. When
    /// multiple filters are supplied, every filter must match.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["events:read"],
        })
    )]
    Events(ObjectEventsCommand),
    /// Get a bounded graph neighborhood around an active object.
    ///
    /// Traversal follows active edges between visible active objects. `--direction` controls which
    /// edge directions may be followed. Node and edge limits bound the returned response; JSON
    /// output reports truncation explicitly.
    /// `--no-root` removes the root from the returned node set.
    ///
    /// In human-readable output, each returned edge is printed once and grouped by source.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["graph:read"],
        })
    )]
    Graph(ObjectsGraphCommand),
}

/// Shared workspace/object selector for commands that target an object.
#[derive(Debug, Copy, Clone, Args)]
pub struct ObjectTargetArgs {
    /// Workspace ID.
    pub workspace_id: Uuid,
    /// Object ID.
    pub object_id: Uuid,
}

impl ObjectsCommand {
    /// Run `kival objects`.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected object command fails.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            ObjectsSubcommand::List(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            ObjectsSubcommand::Get(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            ObjectsSubcommand::Body(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            ObjectsSubcommand::Diff(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            ObjectsSubcommand::Restore(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            ObjectsSubcommand::Edit(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            ObjectsSubcommand::Create(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            ObjectsSubcommand::Update(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            ObjectsSubcommand::Archive(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            ObjectsSubcommand::Unarchive(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            ObjectsSubcommand::Versions(command) => command.run(ctx, output).await,
            ObjectsSubcommand::Edges(command) => command.run(ctx, output).await,
            ObjectsSubcommand::Backlinks(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            ObjectsSubcommand::Attachments(command) => command.run(ctx, output).await,
            ObjectsSubcommand::Grants(command) => command.run(ctx, output).await,
            ObjectsSubcommand::Events(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
            ObjectsSubcommand::Graph(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use argx::Parser as _;

    use super::*;

    /// Verifies restore accepts negative relative selectors as option values.
    #[test]
    fn restore_command_accepts_negative_selector() {
        let workspace_id = Uuid::from_u128(1).to_string();
        let object_id = Uuid::from_u128(2).to_string();

        #[derive(Debug, argx::Parser)]
        struct Parser {
            #[argx(subcommand)]
            command: ObjectsSubcommand,
        }

        let command = Parser::try_parse_from([
            "objects",
            "restore",
            workspace_id.as_str(),
            object_id.as_str(),
            "--from",
            "-3",
        ])
        .unwrap();
        let ObjectsSubcommand::Restore(restore) = command.command else {
            panic!("expected restore command");
        };
        assert_eq!(restore.from, "-3");
    }
}
