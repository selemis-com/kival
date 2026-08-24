//! Machine-readable command discovery for Kival.

use clap::Parser;
use clap_schema::{CliSchema, SchemaDocument, SchemaRequest, schema_handler};
use eyre::Result;

use crate::{
    Cli,
    utils::output::{OutputMode, print_output},
};

/// Arguments for `kival schema`.
#[derive(Debug, Parser)]
#[command(after_long_help = "\
Examples:\n  kival schema\n  kival schema objects\n  kival schema objects get\n  kival schema workspaces list\n  kival schema workspaces update\n\nOmit the command path to inspect the root command. Pass a command group path to inspect\nthat group and its immediate children, or pass an executable command path to inspect its\ncomplete contract. Use --full to resolve the complete command subtree recursively.")]
pub struct SchemaCommand {
    /// Return the complete recursive schema tree below the selected command.
    #[arg(long)]
    pub full: bool,

    /// Command path segments to inspect.
    #[arg(value_name = "COMMAND")]
    pub command_path: Vec<String>,
}

#[schema_handler(run)]
impl SchemaCommand {
    /// Run `kival schema`.
    ///
    /// # Errors
    ///
    /// Returns an error if the requested schema path does not exist or output fails.
    pub fn run(self, output: OutputMode) -> Result<SchemaDocument> {
        let contract = Cli::schema()?;
        let request = SchemaRequest::new(self.command_path).with_full(self.full);
        let document = contract.schema(&request)?;
        let compact = serde_json::to_string(&document)?;
        print_output(output, &document, || println!("{compact}"))?;
        Ok(document)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_for_path(path: &[&str]) -> SchemaDocument {
        let contract = Cli::schema().expect("CLI schema should build");
        contract
            .schema(&SchemaRequest::new(path.iter().copied()))
            .expect("command schema should build")
    }

    #[test]
    fn visible_commands_use_clap_schema_discovery_and_typed_outputs() {
        let document = schema_for_path(&["objects", "get"]);

        assert_eq!(document.command.path, ["objects", "get"]);
        assert!(document.command.invocable);
        assert!(document.command.output.is_some());
    }

    #[test]
    fn command_output_schemas_use_kival_wire_formats() {
        let document = schema_for_path(&["objects", "get"]);
        let output = document.command.output.expect("objects get has JSON output");

        assert_eq!(output["$defs"]["ObjectVersion"]["properties"]["metadata"]["type"], "object");
        assert_eq!(
            output["$defs"]["ObjectResource"]["properties"]["created_at"]["format"],
            "date-time"
        );
    }

    #[test]
    fn config_command_has_no_output_schema_and_includes_shared_arguments() {
        let document = schema_for_path(&["config"]);

        assert!(document.command.invocable);
        assert!(document.command.output.is_none());
        assert!(document.command.options.iter().any(|argument| argument.name == "--config"));
    }

    #[test]
    fn admin_commands_are_discoverable() {
        let root = schema_for_path(&[]);
        assert!(root.subcommands.iter().any(|command| match command {
            clap_schema::SchemaSubcommand::Summary(summary) => summary.path == ["admin"],
            clap_schema::SchemaSubcommand::Resolved(child) => child.command.path == ["admin"],
            _ => false,
        }));

        let get = schema_for_path(&["admin", "users", "get"]);
        assert!(get.command.invocable);
        assert!(get.command.output.is_some());
    }

    #[test]
    fn schema_command_is_discoverable() {
        let document = schema_for_path(&[]);
        assert!(document.subcommands.iter().any(|command| match command {
            clap_schema::SchemaSubcommand::Summary(summary) => summary.path == ["schema"],
            clap_schema::SchemaSubcommand::Resolved(child) => child.command.path == ["schema"],
            _ => false,
        }));
    }
}
