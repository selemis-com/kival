//! Object commands.

mod attachments;
mod display;
mod document;
mod events;
mod grants;
mod history;
mod io;
mod lifecycle;
mod relations;

pub use attachments::{
    ObjectAttachmentContentOutput, ObjectAttachmentsCommand, ObjectAttachmentsContentCommand,
    ObjectAttachmentsGetCommand, ObjectAttachmentsListCommand, ObjectAttachmentsReuseCommand,
    ObjectAttachmentsSubcommand, ObjectAttachmentsUploadCommand,
};
use clap::{Parser, Subcommand};
use clap_schema::CommandSchema;
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

use crate::utils::output::OutputMode;

/// Arguments for `kival objects`.
#[derive(Debug, Parser, CommandSchema)]
pub struct ObjectsCommand {
    /// The object command to run.
    #[command(subcommand)]
    pub command: ObjectsSubcommand,
}

/// The available `kival objects` commands.
#[derive(Debug, Subcommand, CommandSchema)]
pub enum ObjectsSubcommand {
    /// List visible objects in a workspace, newest first.
    ///
    /// Objects are ordered by creation time by default. Use `--order updated` to order by last
    /// update time instead. Active objects are returned by default; use `--status` to select
    /// archived objects or both lifecycle states.
    #[command(name = "list")]
    List(ObjectsListCommand),
    /// Get an object by ID within a workspace.
    #[command(name = "get")]
    Get(ObjectsGetCommand),
    /// Read the current Markdown body of an object.
    #[command(name = "body")]
    Body(ObjectsBodyCommand),
    /// Compare Markdown bodies between immutable object versions.
    #[command(
        name = "diff",
        after_help = "Version selectors:\n  UUID: exact immutable version\n  g: genesis (first version)\n  0: current version\n  -N: N versions before current\n\n--from is required. --to defaults to the current version. Relative selectors are resolved against the same current-version snapshot, and out-of-range offsets fail rather than clamping.\n\nThe default text output is a standard unified diff with three context lines, suitable for tools such as delta, patch, and git apply.\n\nExamples:\n  kival objects diff <WORKSPACE_ID> <OBJECT_ID> --from -1\n  kival objects diff <WORKSPACE_ID> <OBJECT_ID> --from -5\n  kival objects diff <WORKSPACE_ID> <OBJECT_ID> --from g --to -2\n  kival objects diff <WORKSPACE_ID> <OBJECT_ID> --from <VERSION_ID> --to <VERSION_ID>\n  kival objects diff <WORKSPACE_ID> <OBJECT_ID> --from -1 | delta"
    )]
    Diff(ObjectsDiffCommand),
    /// Restore an immutable version as a new current version of the same object.
    #[command(
        name = "restore",
        after_help = "Version selectors:\n  UUID: exact immutable version\n  g: genesis (first version)\n  0: current version\n  -N: N versions before current\n\nRestore never rewrites history. The selected title, body, and metadata are appended as a new current version. Permissions and other non-versioned object state are unchanged. If the selected state is already current, the operation succeeds without creating a redundant version.\n\nExamples:\n  kival objects restore <WORKSPACE_ID> <OBJECT_ID> --from -1\n  kival objects restore <WORKSPACE_ID> <OBJECT_ID> --from g\n  kival objects restore <WORKSPACE_ID> <OBJECT_ID> --from <VERSION_ID>"
    )]
    Restore(ObjectsRestoreCommand),
    /// Edit the current object through Markdown and front matter in an external text editor.
    #[command(
        name = "edit",
        after_help = "Behavior:\n  Editable fields: title, metadata mapping, and Markdown body.\n  Changed state: create one new version when the editor exits.\n  Unchanged state: exit without creating a version.\n  Editor order: KIVAL_EDITOR, VISUAL, EDITOR, then the platform default.\n  The editor must block until editing is complete (for example: code --wait).\n\nExamples:\n  kival objects edit <WORKSPACE_ID> <OBJECT_ID>\n  KIVAL_EDITOR=\"code --wait\" kival objects edit <WORKSPACE_ID> <OBJECT_ID>"
    )]
    Edit(ObjectsEditCommand),
    /// Create an object in a workspace.
    ///
    /// With `--input`, `workspace_id` and `title` may be supplied by the JSON
    /// document instead of command-line arguments. Inspect the complete input contract with
    /// `kival schema objects create`.
    #[command(
        name = "create",
        after_help = "Examples:\n  kival objects create <WORKSPACE_ID> --title \"Title\"\n  kival objects create <WORKSPACE_ID> --title \"Title\" --body-file note.md\n  cat note.md | kival objects create <WORKSPACE_ID> --title \"Title\" --body -\n  kival objects create --input object.json\n  cat object.json | kival objects create --input -"
    )]
    Create(ObjectsCreateCommand),
    /// Update an object, recording changed state as a new version.
    ///
    /// Omitted title, body, and metadata fields are inherited from the current version.
    #[command(
        name = "update",
        after_help = "Concurrency:\n  Every update must name the exact current version it was based on with --expected-current-version-id.\n  Kival returns a conflict instead of applying an update when that version is no longer current.\n\nMetadata:\n  Metadata is flat: values are JSON scalars or one-dimensional scalar arrays.\n  --metadata replaces the complete metadata object.\n  --metadata-set/--metadata-remove preserve other metadata.\n\nExamples:\n  kival objects update <WORKSPACE_ID> <OBJECT_ID> --expected-current-version-id <VERSION_ID> --title \"New title\"\n  kival objects update <WORKSPACE_ID> <OBJECT_ID> --expected-current-version-id <VERSION_ID> --body \"Updated body\"\n  kival objects update <WORKSPACE_ID> <OBJECT_ID> --expected-current-version-id <VERSION_ID> --metadata-set explored=true\n  kival objects update <WORKSPACE_ID> <OBJECT_ID> --input update.json"
    )]
    Update(ObjectsUpdateCommand),
    /// Archive an object while retaining its stored history.
    ///
    /// Archiving changes the object's lifecycle state; it does not delete its versions.
    #[command(name = "archive")]
    Archive(ObjectsArchiveCommand),
    /// Restore an archived object to active status.
    #[command(name = "unarchive")]
    Unarchive(ObjectsUnarchiveCommand),
    /// Inspect object version history.
    #[command(name = "versions")]
    Versions(ObjectVersionsCommand),
    /// Manage object edges.
    #[command(name = "edges")]
    Edges(ObjectEdgesCommand),
    /// List visible incoming relationships and textual references to an object.
    ///
    /// Explicit backlinks are active edges whose target is this object. Textual backlinks are
    /// resolved references from current object versions.
    #[command(name = "backlinks")]
    Backlinks(ObjectsBacklinksCommand),
    /// Manage object attachments.
    #[command(name = "attachments")]
    Attachments(ObjectAttachmentsCommand),
    /// Manage object grants.
    #[command(name = "grants")]
    Grants(ObjectGrantsCommand),
    /// List events for an object in ascending global sequence order.
    ///
    /// `--after-sequence` is exclusive. When multiple filters are supplied, every filter must
    /// match.
    #[command(name = "events")]
    Events(ObjectEventsCommand),
    /// Get a bounded graph neighborhood around an active object.
    ///
    /// Traversal follows active edges between visible active objects. `--direction` controls which
    /// edge directions may be followed. Node and edge limits bound the returned response; JSON
    /// output reports truncation explicitly.
    /// `--no-root` removes the root from the returned node set.
    ///
    /// In human-readable output, each returned edge is printed once and grouped by source.
    #[command(name = "graph")]
    Graph(ObjectsGraphCommand),
}

/// Shared workspace/object selector for commands that target an object.
#[derive(Debug, Copy, Clone, Parser)]
pub struct ObjectTargetArgs {
    /// Workspace ID.
    #[arg(value_name = "WORKSPACE_ID")]
    pub workspace_id: Uuid,
    /// Object ID.
    #[arg(value_name = "OBJECT_ID")]
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
                command.run(ctx, output).await?;
                Ok(())
            }
            ObjectsSubcommand::Get(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            ObjectsSubcommand::Body(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            ObjectsSubcommand::Diff(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            ObjectsSubcommand::Restore(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            ObjectsSubcommand::Edit(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            ObjectsSubcommand::Create(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            ObjectsSubcommand::Update(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            ObjectsSubcommand::Archive(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            ObjectsSubcommand::Unarchive(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            ObjectsSubcommand::Versions(command) => command.run(ctx, output).await,
            ObjectsSubcommand::Edges(command) => command.run(ctx, output).await,
            ObjectsSubcommand::Backlinks(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            ObjectsSubcommand::Attachments(command) => command.run(ctx, output).await,
            ObjectsSubcommand::Grants(command) => command.run(ctx, output).await,
            ObjectsSubcommand::Events(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            ObjectsSubcommand::Graph(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies restore accepts negative relative selectors as option values.
    #[test]
    fn restore_command_accepts_negative_selector() {
        let workspace_id = Uuid::from_u128(1).to_string();
        let object_id = Uuid::from_u128(2).to_string();

        let command = ObjectsCommand::try_parse_from([
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
