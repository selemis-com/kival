//! Machine-readable command discovery for Kival.

use clap::Parser;
use clap_schema::{CliSchema, SchemaRequest, schema_handler};
use eyre::Result;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::{
    Cli,
    commands::{
        admin::UpdateUserInput,
        groups::{CreateGroupInput, UpdateGroupInput},
        objects::{CreateObjectInput, UpdateObjectInput},
        workspaces::UpdateWorkspaceInput,
    },
    utils::{
        error::{CliError, CliErrorResponse},
        output::{OutputMode, print_output},
    },
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

/// Kival's machine-facing command document.
///
/// Command topology, invocation metadata, and successful output schemas come from `clap_schema`.
/// Kival augments that discovery tree with its structured JSON `--input` contracts and adds its
/// stable error envelope at the document root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SchemaDocument {
    /// Clap-derived discovery augmented with Kival-owned command metadata.
    #[serde(flatten)]
    pub discovery: DiscoveryDocument,
    /// Stable Kival JSON error envelope schema.
    pub error_schema: Value,
}

/// One resolved Kival command discovery node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct DiscoveryDocument {
    /// Complete Clap-derived contract for the selected command.
    #[serde(flatten)]
    pub command: clap_schema::CommandInfo,
    /// Direct child commands at the requested resolution depth.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub subcommands: Vec<SchemaSubcommand>,
    /// Application-specific structured JSON input contract, when the command accepts `--input`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_input: Option<StructuredInputSchema>,
}

/// One child entry in Kival's augmented discovery tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaSubcommand {
    /// Compact child reference used by shallow discovery.
    Summary(clap_schema::SchemaCommandSummary),
    /// Recursively resolved child used by full discovery.
    Resolved(Box<DiscoveryDocument>),
}

/// Structured JSON input metadata for commands that accept `--input`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct StructuredInputSchema {
    /// CLI option that selects structured input.
    pub option: &'static str,
    /// Structured input format.
    pub format: &'static str,
    /// Path token that selects standard input.
    pub stdin: &'static str,
    /// JSON Schema for the application-level structured input value.
    pub schema: Value,
}

#[schema_handler(run)]
impl SchemaCommand {
    /// Run `kival schema`.
    ///
    /// # Errors
    ///
    /// Returns an error if the requested schema path does not exist or output fails.
    pub fn run(self, output: OutputMode) -> Result<SchemaDocument> {
        let document = schema_for_path_with_options(&self.command_path, self.full)?;
        let compact = serde_json::to_string(&document)?;
        print_output(output, &document, || println!("{compact}"))?;
        Ok(document)
    }
}

/// Returns a Kival schema document for a command path.
///
/// # Errors
///
/// Returns an invalid command path error for unknown paths, or an internal schema error
/// when the Clap command tree and compile-time registrations disagree.
#[cfg(test)]
fn schema_for_path(path: &[String]) -> Result<SchemaDocument> {
    schema_for_path_with_options(path, false)
}

/// Returns the declared successful JSON output schema for an executable command path.
///
/// # Errors
///
/// Returns an invalid command path error for unknown or non-executable paths.
pub(crate) fn output_schema_for_path(path: &[String]) -> Result<Option<Value>> {
    let contract = Cli::schema()?;
    let path_refs = path.iter().map(String::as_str).collect::<Vec<_>>();
    let command = contract.command(&path_refs).map_err(map_schema_path_error)?;
    if !command.invocable {
        return Err(CliError::invalid_command_path(format!(
            "command path is not executable: {}",
            path.join(" ")
        ))
        .into());
    }
    Ok(command.output)
}

/// Returns a Kival schema document with optional recursive child resolution.
///
/// # Errors
///
/// Returns an invalid command path error for unknown paths, or an internal schema error
/// when the Clap command tree and compile-time registrations disagree.
fn schema_for_path_with_options(path: &[String], full: bool) -> Result<SchemaDocument> {
    let contract = Cli::schema()?;
    let request = SchemaRequest::new(path.iter().cloned()).with_full(full);
    let discovery = contract.schema(&request).map_err(map_schema_path_error)?;

    Ok(SchemaDocument {
        discovery: augment_discovery(discovery)?,
        error_schema: schema_fragment_for::<CliErrorResponse>(),
    })
}

/// Adds Kival-owned metadata to a Clap discovery node.
fn augment_discovery(document: clap_schema::SchemaDocument) -> Result<DiscoveryDocument> {
    let clap_schema::SchemaDocument { command, subcommands, .. } = document;
    let structured_input = structured_input_schema(&command.path);
    let subcommands =
        subcommands.into_iter().map(augment_subcommand).collect::<Result<Vec<_>>>()?;

    Ok(DiscoveryDocument { command, subcommands, structured_input })
}

/// Adds Kival-owned metadata to one Clap child discovery entry.
fn augment_subcommand(subcommand: clap_schema::SchemaSubcommand) -> Result<SchemaSubcommand> {
    match subcommand {
        clap_schema::SchemaSubcommand::Summary(summary) => Ok(SchemaSubcommand::Summary(summary)),
        clap_schema::SchemaSubcommand::Resolved(child) => {
            Ok(SchemaSubcommand::Resolved(Box::new(augment_discovery(*child)?)))
        }
        _ => Err(eyre::eyre!("unsupported clap_schema subcommand representation")),
    }
}

/// Maps user-selected unknown paths to Kival's stable CLI error surface while preserving contract
/// construction failures as internal errors.
fn map_schema_path_error(error: clap_schema::Error) -> eyre::Report {
    if let clap_schema::Error::UnknownCommand { path } = &error {
        return CliError::invalid_command_path(format!("unknown command path: {}", path.join(" ")))
            .into();
    }
    error.into()
}

/// Generates a standalone Kival-owned schema fragment.
fn schema_fragment_for<T>() -> Value
where
    T: JsonSchema,
{
    let mut schema = schemars::schema_for!(T).to_value();
    if let Value::Object(root) = &mut schema {
        root.remove("$schema");
        root.remove("title");
    }
    schema
}

/// Returns Kival's application-specific structured JSON input contract for a command.
///
/// `clap_schema` reflects `--input` as a path-valued CLI option. The JSON document read from that
/// path is a Kival-owned deserialization contract, so its semantic Rust type is associated here.
fn structured_input_schema(path: &[String]) -> Option<StructuredInputSchema> {
    let schema = if path == ["admin", "users", "update"] {
        schema_fragment_for::<UpdateUserInput>()
    } else if path == ["groups", "create"] {
        schema_fragment_for::<CreateGroupInput>()
    } else if path == ["groups", "update"] {
        schema_fragment_for::<UpdateGroupInput>()
    } else if path == ["objects", "create"] {
        schema_fragment_for::<CreateObjectInput>()
    } else if path == ["objects", "update"] {
        schema_fragment_for::<UpdateObjectInput>()
    } else if path == ["workspaces", "update"] {
        schema_fragment_for::<UpdateWorkspaceInput>()
    } else {
        return None;
    };

    Some(StructuredInputSchema { option: "--input", format: "json", stdin: "-", schema })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::error::CliErrorCode;

    #[test]
    fn object_response_schema_has_title_properties() {
        let schema = output_schema_for_path(&["objects".to_owned(), "get".to_owned()])
            .expect("objects get schema should build")
            .expect("objects get has JSON output");

        assert!(schema["$defs"]["ObjectResource"]["properties"]["title"].is_object());
        assert!(schema["$defs"]["ObjectVersion"]["properties"]["title"].is_object());
        assert!(
            schema["$defs"]["ObjectResource"]["required"]
                .as_array()
                .unwrap()
                .contains(&Value::String("title".to_owned()))
        );
    }

    #[test]
    fn visible_commands_use_clap_schema_discovery_and_typed_outputs() {
        let document = schema_for_path(&["objects".to_owned(), "get".to_owned()])
            .expect("objects get schema should build");

        assert_eq!(document.discovery.command.path, ["objects", "get"]);
        assert!(document.discovery.command.invocable);
        assert!(document.discovery.command.output.is_some());
        assert!(document.discovery.structured_input.is_none());
    }

    #[test]
    fn command_output_schemas_use_kival_wire_formats() {
        let document = schema_for_path(&["objects".to_owned(), "get".to_owned()])
            .expect("objects get schema should build");
        let output = document.discovery.command.output.expect("objects get has JSON output");

        assert_eq!(output["$defs"]["ObjectVersion"]["properties"]["metadata"]["type"], "object");
        assert_eq!(
            output["$defs"]["ObjectResource"]["properties"]["created_at"]["format"],
            "date-time"
        );
    }

    #[test]
    fn config_command_has_no_output_schema_and_includes_shared_arguments() {
        let document = schema_for_path(&["config".to_owned()]).expect("config schema should build");

        assert!(document.discovery.command.invocable);
        assert!(document.discovery.command.output.is_none());
        assert!(
            document.discovery.command.options.iter().any(|argument| argument.name == "--config")
        );
    }

    #[test]
    fn objects_update_exposes_structured_json_input() {
        let document = schema_for_path(&["objects".to_owned(), "update".to_owned()])
            .expect("objects update schema should build");
        let structured =
            document.discovery.structured_input.expect("objects update accepts --input");

        assert_eq!(structured.option, "--input");
        assert_eq!(structured.format, "json");
        assert_eq!(structured.stdin, "-");
        assert!(structured.schema["properties"]["metadata"].is_object());
        assert!(structured.schema["allOf"].is_array());
    }

    #[test]
    fn structured_input_commands_expose_structured_json_contracts() {
        for path in [
            ["groups", "create"],
            ["groups", "update"],
            ["objects", "create"],
            ["objects", "update"],
            ["workspaces", "update"],
        ] {
            let path = path.map(str::to_owned);
            let document = schema_for_path(&path).expect("structured input schema should build");

            assert!(
                document.discovery.structured_input.is_some(),
                "{} should expose structured input",
                path.join(" ")
            );
        }
    }

    #[test]
    fn field_projection_uses_the_published_output_schema() {
        let path = ["objects".to_owned(), "get".to_owned()];

        let projected = output_schema_for_path(&path).expect("output schema should build");
        let discovered =
            schema_for_path(&path).expect("discovery schema should build").discovery.command.output;

        assert_eq!(projected, discovered);
    }

    #[test]
    fn full_discovery_includes_structured_input_metadata() {
        let document = schema_for_path_with_options(&["objects".to_owned()], true)
            .expect("full objects schema should build");
        let create = document
            .discovery
            .subcommands
            .iter()
            .find_map(|command| match command {
                SchemaSubcommand::Resolved(child)
                    if child.command.path == ["objects", "create"] =>
                {
                    Some(child)
                }
                SchemaSubcommand::Summary(_) | SchemaSubcommand::Resolved(_) => None,
            })
            .expect("objects create should be resolved");

        assert!(create.structured_input.is_some());
    }

    #[test]
    fn admin_commands_are_discoverable() {
        let root = schema_for_path(&[]).expect("root schema should build");
        assert!(root.discovery.subcommands.iter().any(|command| match command {
            SchemaSubcommand::Summary(summary) => summary.path == ["admin"],
            SchemaSubcommand::Resolved(child) => child.command.path == ["admin"],
        }));

        let get = schema_for_path(&["admin".to_owned(), "users".to_owned(), "get".to_owned()])
            .expect("admin users get schema should build");
        assert!(get.discovery.command.invocable);
        assert!(get.discovery.command.output.is_some());

        let update =
            schema_for_path(&["admin".to_owned(), "users".to_owned(), "update".to_owned()])
                .expect("admin users update schema should build");
        assert!(update.discovery.structured_input.is_some());
    }

    #[test]
    fn schema_command_is_discoverable() {
        let document = schema_for_path(&[]).expect("root schema should build");
        assert!(document.discovery.subcommands.iter().any(|command| match command {
            SchemaSubcommand::Summary(summary) => summary.path == ["schema"],
            SchemaSubcommand::Resolved(child) => child.command.path == ["schema"],
        }));
    }

    #[test]
    fn error_schema_contains_public_cli_error_codes() {
        let document = schema_for_path(&["whoami".to_owned()]).expect("whoami schema should build");
        let serialized = serde_json::to_string(&document.error_schema).unwrap();
        for code in CliErrorCode::PUBLIC_STRINGS {
            assert!(serialized.contains(code));
        }
    }
}
