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

use argx::{Args, Subcommand};
pub use attachments::{
    ObjectAttachmentContentOutput, ObjectAttachmentsCommand, ObjectAttachmentsContentCommand,
    ObjectAttachmentsGetCommand, ObjectAttachmentsListCommand, ObjectAttachmentsReuseCommand,
    ObjectAttachmentsSubcommand, ObjectAttachmentsUploadCommand,
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

use crate::utils::output::OutputMode;

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
    List(ObjectsListCommand),
    /// Get an object by ID within a workspace.
    Get(ObjectsGetCommand),
    /// Read the current Markdown body of an object.
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
    Edit(ObjectsEditCommand),
    /// Create an object in a workspace.
    ///
    /// With `--input`, `workspace_id` and `title` may be supplied by the JSON document instead of
    /// command-line arguments. Inspect the complete input contract with `kival schema objects
    /// create`.
    ///
    /// Examples: `kival objects create <WORKSPACE_ID> --title "Title"`, add
    /// `--body-file note.md`, pipe Markdown with `--body -`, or provide structured input with
    /// `--input object.json` / `--input -`.
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
    /// Examples: update `--title`, `--body`, or `--metadata-set explored=true`, or provide
    /// structured input with `--input update.json`.
    Update(ObjectsUpdateCommand),
    /// Archive an object while retaining its stored history.
    ///
    /// Archiving changes the object's lifecycle state; it does not delete its versions.
    Archive(ObjectsArchiveCommand),
    /// Restore an archived object to active status.
    Unarchive(ObjectsUnarchiveCommand),
    /// Inspect object version history.
    Versions(ObjectVersionsCommand),
    /// Manage object edges.
    Edges(ObjectEdgesCommand),
    /// List visible incoming relationships and textual references to an object.
    ///
    /// Explicit backlinks are active edges whose target is this object. Textual backlinks are
    /// resolved references from current object versions.
    Backlinks(ObjectsBacklinksCommand),
    /// Manage object attachments.
    Attachments(ObjectAttachmentsCommand),
    /// Manage object grants.
    Grants(ObjectGrantsCommand),
    /// List events for an object in ascending global sequence order.
    ///
    /// `--after-sequence` is exclusive. When multiple filters are supplied, every filter must
    /// match.
    Events(ObjectEventsCommand),
    /// Get a bounded graph neighborhood around an active object.
    ///
    /// Traversal follows active edges between visible active objects. `--direction` controls which
    /// edge directions may be followed. Node and edge limits bound the returned response; JSON
    /// output reports truncation explicitly.
    /// `--no-root` removes the root from the returned node set.
    ///
    /// In human-readable output, each returned edge is printed once and grouped by source.
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
