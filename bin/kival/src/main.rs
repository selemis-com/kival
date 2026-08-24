//! Entrypoint for `kival`.

use std::ffi::OsString;

use clap::{ArgMatches, Args, CommandFactory, Parser};
use clap_schema::{CliSchema, CommandSchema, schema_handler};
use eyre::Result;
use kival_cli::{
    args::datadir::DatadirArgs, commands::config::ConfigCommand, runner::CliRunner, sigsegv,
};
use kival_config::config;
use url::Url;

use crate::{
    commands::{
        admin::AdminCommand,
        completions::CompletionsCommand,
        events::EventsCommand,
        groups::GroupsCommand,
        objects::ObjectsCommand,
        schema::{SchemaCommand, output_schema_for_path},
        search::SearchCommand,
        server::ServerCommand,
        whoami::WhoamiCommand,
        workspaces::WorkspacesCommand,
    },
    utils::{
        banner::BANNER,
        error::{CliError, CliErrorBody, print_json_error},
        fields::{Projection, parse_projection, validate_projection},
        output::OutputMode,
        version::{LONG_VERSION, SHORT_VERSION},
    },
};

pub mod commands;
pub mod utils;

config! {
    /// The `kival` configuration.
    pub struct ClientConfig {
        /// Kival server root URL.
        pub url: Url = Url::parse("http://127.0.0.1:3000")
            .expect("default client URL must be valid"),

        /// Default API key used when no explicit credential is supplied.
        pub api_key: Option<String> = None,
    }

}

/// Kival client configuration command payload.
#[derive(Debug, Args)]
pub struct ClientConfigCommand {
    /// Shared configuration command arguments.
    #[command(flatten)]
    command: ConfigCommand,
}

#[schema_handler(run)]
impl ClientConfigCommand {
    /// Run `kival config`.
    ///
    /// # Errors
    ///
    /// Returns an error if the effective client configuration cannot be loaded or printed.
    async fn run(self) -> kival_cli::commands::config::Result<()> {
        self.command.run::<ClientConfig>().await
    }
}

/// The available CLI commands for the `kival` CLI.
#[derive(Debug, clap::Subcommand, CommandSchema)]
pub enum Commands {
    /// Manage workspaces, memberships, linked groups, events, and workspace graphs.
    #[command(name = "workspaces")]
    Workspaces(WorkspacesCommand),

    /// Manage workspace objects, versions, relationships, attachments, and access.
    #[command(name = "objects")]
    Objects(ObjectsCommand),

    /// Manage reusable access groups and their memberships.
    #[command(name = "groups")]
    Groups(GroupsCommand),

    /// Inspect activity visible to the current user.
    #[command(name = "events")]
    Events(EventsCommand),

    /// Search indexed visible workspace content using auto, text, literal, or exact matching.
    #[command(name = "search")]
    Search(SearchCommand),

    /// Show the identity associated with the resolved API key.
    #[command(name = "whoami")]
    Whoami(WhoamiCommand),

    /// Check server health and readiness.
    #[command(name = "server")]
    Server(ServerCommand),

    /// Print the effective Kival client configuration as TOML.
    #[command(name = "config")]
    Config(ClientConfigCommand),

    /// Generate shell completion script text.
    #[command(name = "completions")]
    Completions(CompletionsCommand),

    /// Discover commands and JSON Schema contracts.
    #[command(name = "schema")]
    Schema(SchemaCommand),

    /// Manage Kival as admin.
    #[command(name = "admin", hide = true)]
    Admin(AdminCommand),
}

/// CLI client to interact with Kival.
///
/// This is the entrypoint to the executable.
#[derive(Debug, Parser, CliSchema)]
#[command(
    author,
    version = SHORT_VERSION,
    long_version = LONG_VERSION,
    long_about = None,
    before_help = BANNER,
    after_help = "\
Machine-readable usage:
  kival schema                    Inspect the root command and its children
  kival schema <COMMAND>          Inspect a command or command group
  kival schema <COMMAND> --full   Emit a recursive command schema
  kival <COMMAND> --json          Emit machine-readable output

Agent authentication:
  KIVAL_URL=https://kival.example KIVAL_API_KEY=kvl_... kival <COMMAND>",
)]
pub struct Cli {
    /// The command to run.
    #[command(subcommand)]
    pub command: Commands,

    /// Emit JSON output instead of human-readable output, where supported.
    #[arg(short, long, global = true, help_heading = "Output")]
    pub json: bool,

    /// Select comma-separated properties from JSON output.
    ///
    /// Values must correspond to fields in the command's output schema.
    /// Nested paths are available only when explicitly supported.
    /// This is an output projection option, not a search filter.
    #[arg(
        short,
        long,
        global = true,
        requires = "json",
        value_delimiter = ',',
        help_heading = "Output"
    )]
    pub fields: Vec<String>,

    /// The data directory to use.
    #[command(flatten)]
    pub datadir: DatadirArgs,

    /// Use this API key for the current invocation.
    #[arg(long, global = true, value_name = "KEY", help_heading = "Authentication")]
    pub api_key: Option<String>,

    /// Override the configured Kival server root URL.
    #[arg(long, global = true, env = "KIVAL_URL", value_name = "URL")]
    pub url: Option<Url>,
}

impl Cli {
    /// Execute the CLI command.
    ///
    /// # Errors
    ///
    /// Returns an error if runtime creation or command execution fails.
    pub fn run(self, selected_command_path: &[String]) -> Result<()> {
        let Self { command, json, fields, datadir, api_key, url } = self;

        utils::credentials::set_command_overrides(api_key, url)?;

        match &command {
            Commands::Config(_) if json => {
                return Err(CliError::invalid_argument(
                    "config output is TOML text and does not support --json",
                )
                .into());
            }
            Commands::Completions(_) if json => {
                return Err(CliError::invalid_argument(
                    "completions output is shell script text and does not support --json",
                )
                .into());
            }
            _ => {}
        }

        let projection = parse_projection(&fields)?;
        validate_output_projection(selected_command_path, projection.as_ref())?;
        let output = OutputMode::from_options(json, projection);

        match command {
            Commands::Completions(command) => command.run(&output),
            Commands::Schema(command) => {
                command.run()?;
                Ok(())
            }

            command => {
                let runner = create_runner(&datadir)?;

                match command {
                    Commands::Config(command) => Ok(runner.run_until_ctrl_c(command.run())?),
                    Commands::Whoami(command) => {
                        Ok(runner.run_command_until_ctrl_c(async move |ctx| {
                            command.run(ctx, output).await?;
                            Ok::<(), eyre::Report>(())
                        })?)
                    }
                    Commands::Admin(command) => {
                        Ok(runner.run_command_until_ctrl_c(|ctx| command.run(ctx, output))?)
                    }
                    Commands::Events(command) => {
                        Ok(runner.run_command_until_ctrl_c(|ctx| command.run(ctx, output))?)
                    }
                    Commands::Groups(command) => {
                        Ok(runner.run_command_until_ctrl_c(|ctx| command.run(ctx, output))?)
                    }
                    Commands::Search(command) => {
                        Ok(runner.run_command_until_ctrl_c(async move |ctx| {
                            command.run(ctx, output).await?;
                            Ok::<(), eyre::Report>(())
                        })?)
                    }
                    Commands::Server(command) => {
                        Ok(runner.run_command_until_ctrl_c(|ctx| command.run(ctx, output))?)
                    }
                    Commands::Objects(command) => {
                        Ok(runner.run_command_until_ctrl_c(|ctx| command.run(ctx, output))?)
                    }
                    Commands::Workspaces(command) => {
                        Ok(runner.run_command_until_ctrl_c(|ctx| command.run(ctx, output))?)
                    }
                    Commands::Completions(_) | Commands::Schema(_) => unreachable!(
                        "schema and completions are handled before runtime initialization"
                    ),
                }
            }
        }
    }
}

/// Builds the runtime-backed CLI runner for commands that need Kival client context.
fn create_runner(datadir: &DatadirArgs) -> Result<CliRunner> {
    let datadir_path = datadir.resolve_datadir();
    Ok(CliRunner::try_default_runtime(datadir_path.into())?)
}

/// Validates a parsed projection against the selected command's declared output schema.
///
/// # Errors
///
/// Returns a stable CLI error when the selected command has no declared JSON output or when the
/// projection is invalid for the declared output schema.
fn validate_output_projection(
    selected_command_path: &[String],
    projection: Option<&Projection>,
) -> Result<()> {
    let Some(projection) = projection else {
        return Ok(());
    };

    if selected_command_path == ["schema"] {
        return Ok(());
    }

    let schema = output_schema_for_path(selected_command_path)?.ok_or_else(|| {
        CliError::invalid_projection(
            "Selected command does not support JSON field projection.",
            None,
        )
    })?;

    validate_projection(&schema, projection)
}

/// Returns the canonical parsed command path from raw CLI arguments.
///
/// # Errors
///
/// Returns an invalid argument error if clap cannot parse the arguments.
fn command_path_from_args(args: impl IntoIterator<Item = OsString>) -> Result<Vec<String>> {
    let matches = Cli::command()
        .try_get_matches_from(args)
        .map_err(|error| CliError::invalid_argument(error.to_string()))?;

    Ok(command_path_from_matches(&matches))
}

/// Returns the selected subcommand path from clap matches.
fn command_path_from_matches(matches: &ArgMatches) -> Vec<String> {
    let mut path = Vec::new();
    let mut matches = matches;

    while let Some((name, subcommand_matches)) = matches.subcommand() {
        path.push(name.to_owned());
        matches = subcommand_matches;
    }

    path
}

/// Returns whether raw CLI args request JSON output before Clap parsing succeeds.
fn json_flag_requested(args: impl IntoIterator<Item = OsString>) -> bool {
    for arg in args {
        if arg == "--" {
            return false;
        }

        let Some(arg) = arg.to_str() else {
            continue;
        };
        if arg == "--json" || arg.strip_prefix("--json=").is_some() {
            return true;
        }
        if arg.starts_with('-') && !arg.starts_with("--") && arg.chars().skip(1).any(|ch| ch == 'j')
        {
            return true;
        }
    }

    false
}

fn main() {
    // Install the SIGSEGV handler to print a backtrace on segmentation faults.
    sigsegv::install();

    // Enable Rust backtraces by default for better debugging, unless the user has explicitly set
    // RUST_BACKTRACE in their environment, in which case we respect their choice.
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        // SAFETY: this runs at process startup before Kival spawns threads or
        // hands environment access to other code.
        unsafe {
            std::env::set_var("RUST_BACKTRACE", "1");
        }
    }

    // Parse command-line arguments and run the appropriate command. If any error occurs, print it
    // and exit with a non-zero status code.
    let raw_args = std::env::args_os().collect::<Vec<_>>();
    let json_requested = json_flag_requested(raw_args.iter().skip(1).cloned());

    let cli = match Cli::try_parse_from(raw_args.clone()) {
        Ok(cli) => cli,
        Err(err) => {
            if err.exit_code() == 0 {
                let _ = err.print();
                std::process::exit(0);
            }

            if json_requested {
                print_json_error(&CliErrorBody::from_clap_error(&err));
                std::process::exit(err.exit_code());
            }

            let _ = err.print();
            std::process::exit(err.exit_code());
        }
    };

    let json_errors = cli.json || matches!(&cli.command, Commands::Schema(_));

    let selected_command_path = match command_path_from_args(raw_args) {
        Ok(path) => path,
        Err(err) => {
            if json_errors {
                print_json_error(&CliErrorBody::from_report(&err));
            } else {
                eprintln!("Error: {err}");
            }
            std::process::exit(1);
        }
    };

    if let Err(err) = cli.run(&selected_command_path) {
        if json_errors {
            print_json_error(&CliErrorBody::from_report(&err));
        } else {
            eprintln!("Error: {err}");
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use clap::{CommandFactory, Parser};

    use crate::{
        Cli, command_path_from_args, json_flag_requested,
        utils::error::{CliErrorBody, CliErrorCode},
    };

    #[test]
    fn raw_json_flag_scan_handles_clusters_and_double_dash() {
        assert!(json_flag_requested(std::iter::once(OsString::from("-j"))));
        assert!(json_flag_requested(std::iter::once(OsString::from("-vj"))));
        assert!(json_flag_requested(std::iter::once(OsString::from("--json"))));
        assert!(!json_flag_requested(["--", "--json"].into_iter().map(OsString::from)));
    }

    #[test]
    fn fields_requires_json() {
        let error = Cli::try_parse_from([
            "kival",
            "objects",
            "get",
            "00000000-0000-0000-0000-000000000000",
            "00000000-0000-0000-0000-000000000000",
            "--fields",
            "id",
        ])
        .unwrap_err();
        let body = CliErrorBody::from_clap_error(&error);

        assert_eq!(body.code, CliErrorCode::InvalidArgument);
    }

    #[test]
    fn fields_parse_as_global_comma_delimited_values() {
        let cli = Cli::try_parse_from([
            "kival",
            "objects",
            "get",
            "00000000-0000-0000-0000-000000000000",
            "00000000-0000-0000-0000-000000000000",
            "--json",
            "--fields",
            "id,current_version.id",
        ])
        .expect("--json --fields should parse");

        assert!(cli.json);
        assert_eq!(cli.fields, ["id", "current_version.id"]);
    }

    /// Verifies object listing accepts explicit creation and update ordering.
    #[test]
    fn object_list_command_parses_order() {
        let workspace = "00000000-0000-0000-0000-000000000000";

        assert!(Cli::try_parse_from(["kival", "objects", "list", workspace]).is_ok());
        assert!(
            Cli::try_parse_from(["kival", "objects", "list", workspace, "--order", "created"])
                .is_ok()
        );
        assert!(
            Cli::try_parse_from(["kival", "objects", "list", workspace, "--order", "updated"])
                .is_ok()
        );
        assert!(
            Cli::try_parse_from(["kival", "objects", "list", workspace, "--order", "recent"])
                .is_err()
        );
    }

    #[test]
    fn admin_user_lifecycle_commands_parse() {
        let user_id = "00000000-0000-0000-0000-000000000001";

        assert!(Cli::try_parse_from(["kival", "admin", "users", "disable", user_id]).is_ok());
        assert!(Cli::try_parse_from(["kival", "admin", "users", "enable", user_id]).is_ok());
    }

    /// Verifies body sources and body export options parse with unambiguous conflicts.
    #[test]
    fn object_body_options_parse_and_conflict() {
        let workspace = "00000000-0000-0000-0000-000000000000";
        let object = "00000000-0000-0000-0000-000000000001";
        let version = "00000000-0000-0000-0000-000000000002";

        assert!(
            Cli::try_parse_from([
                "kival",
                "objects",
                "create",
                workspace,
                "--title",
                "Title",
                "--body-file",
                "body.md",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "kival",
                "objects",
                "update",
                workspace,
                object,
                "--expected-current-version-id",
                version,
                "--body",
                "-",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from(["kival", "objects", "update", workspace, object, "--body", "-"])
                .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "kival", "objects", "body", workspace, object, "--output", "body.md", "--force",
            ])
            .is_ok()
        );

        assert!(
            Cli::try_parse_from([
                "kival",
                "objects",
                "update",
                workspace,
                object,
                "--expected-current-version-id",
                version,
                "--body-file",
                "-",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "kival",
                "objects",
                "update",
                workspace,
                object,
                "--expected-current-version-id",
                version,
                "--body",
                "inline",
                "--body-file",
                "body.md",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "kival",
                "objects",
                "update",
                workspace,
                object,
                "--input",
                "update.json",
                "--body-file",
                "body.md",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from(["kival", "objects", "body", workspace, object, "--force",])
                .is_err()
        );
    }

    /// Verifies metadata mutation conveniences parse and conflict with full replacement/input.
    #[test]
    fn object_metadata_mutation_options_parse_and_conflict() {
        let workspace = "00000000-0000-0000-0000-000000000000";
        let object = "00000000-0000-0000-0000-000000000001";
        let version = "00000000-0000-0000-0000-000000000002";

        assert!(
            Cli::try_parse_from([
                "kival",
                "objects",
                "update",
                workspace,
                object,
                "--expected-current-version-id",
                version,
                "--metadata-set",
                "explored=true",
                "--metadata-remove",
                "timestamp",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "kival",
                "objects",
                "update",
                workspace,
                object,
                "--expected-current-version-id",
                version,
                "--metadata",
                r#"{"explored":true}"#,
                "--metadata-remove",
                "timestamp",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "kival",
                "objects",
                "update",
                workspace,
                object,
                "--input",
                "update.json",
                "--metadata-set",
                "explored=true",
            ])
            .is_err()
        );
    }

    /// Verifies a Markdown body file does not replace the structural fields required for creation.
    #[test]
    fn object_create_body_file_still_requires_structural_fields() {
        assert!(
            Cli::try_parse_from([
                "kival",
                "objects",
                "create",
                "00000000-0000-0000-0000-000000000000",
                "--body-file",
                "body.md",
            ])
            .is_err()
        );
    }

    /// Verifies object diff requires a source selector and accepts exact and relative selectors.
    #[test]
    fn object_diff_command_requires_from_and_parses_version_selection() {
        let workspace = "00000000-0000-0000-0000-000000000000";
        let object = "00000000-0000-0000-0000-000000000001";
        let from = "00000000-0000-0000-0000-000000000002";
        let to = "00000000-0000-0000-0000-000000000003";

        assert!(Cli::try_parse_from(["kival", "objects", "diff", workspace, object]).is_err());
        assert!(
            Cli::try_parse_from(["kival", "objects", "diff", workspace, object, "--from", "-5",])
                .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "kival", "objects", "diff", workspace, object, "--from", "g", "--to", "-2",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "kival", "objects", "diff", workspace, object, "--from", from, "--to", to,
            ])
            .is_ok()
        );
        assert!(Cli::try_parse_from(["kival", "objects", "diff", workspace]).is_err());
        assert!(
            Cli::try_parse_from(["kival", "objects", "diff", workspace, object, "--to", to,])
                .is_err()
        );
        assert!(
            Cli::try_parse_from(["kival", "objects", "diff", workspace, object, from]).is_err()
        );
        assert!(
            Cli::try_parse_from([
                "kival", "objects", "diff", workspace, object, "--from", "-1", "-U5",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "kival",
                "objects",
                "diff",
                workspace,
                object,
                "--from",
                "-1",
                "--context",
                "5",
            ])
            .is_err()
        );
    }

    /// Verifies the object editor command requires an explicit workspace and object target.
    #[test]
    fn object_edit_command_parses_explicit_object_target() {
        let workspace = "00000000-0000-0000-0000-000000000000";
        let object = "00000000-0000-0000-0000-000000000001";

        assert!(Cli::try_parse_from(["kival", "objects", "edit", workspace, object]).is_ok());
        assert!(Cli::try_parse_from(["kival", "objects", "edit", workspace]).is_err());
        assert!(Cli::try_parse_from(["kival", "objects", "edit"]).is_err());
        assert!(Cli::try_parse_from(["kival", "edit", workspace, object]).is_err());
    }

    #[test]
    fn create_payload_fields_are_required_unless_input_is_present() {
        assert!(Cli::try_parse_from(["kival", "groups", "create"]).is_err());
        assert!(Cli::try_parse_from(["kival", "groups", "create", "--name", "Team"]).is_ok());
        assert!(
            Cli::try_parse_from(["kival", "groups", "create", "--input", "group.json"]).is_ok()
        );

        assert!(Cli::try_parse_from(["kival", "objects", "create"]).is_err());
        assert!(
            Cli::try_parse_from([
                "kival",
                "objects",
                "create",
                "00000000-0000-0000-0000-000000000000",
                "--title",
                "Title",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from(["kival", "objects", "create", "--input", "object.json"]).is_ok()
        );
    }

    #[test]
    fn whoami_replaces_the_auth_command_group() {
        assert!(Cli::try_parse_from(["kival", "whoami"]).is_ok());
        assert!(Cli::try_parse_from(["kival", "auth", "me"]).is_err());
        assert!(Cli::try_parse_from(["kival", "auth", "whoami"]).is_err());

        let mut root = Cli::command();
        let commands = root.get_subcommands().map(clap::Command::get_name).collect::<Vec<_>>();

        assert!(commands.contains(&"whoami"));
        assert!(!commands.contains(&"auth"));

        let help = root.render_long_help().to_string();
        assert!(help.lines().any(|line| line.trim_start().starts_with("whoami")));
        assert!(!help.lines().any(|line| line.trim_start().starts_with("auth ")));
    }

    #[test]
    fn workspace_create_is_not_exposed() {
        assert!(Cli::try_parse_from(["kival", "workspaces", "create"]).is_err());
        assert!(Cli::try_parse_from(["kival", "workspaces", "list"]).is_ok());

        let mut root = Cli::command();
        let workspaces = root
            .find_subcommand_mut("workspaces")
            .expect("workspaces command should remain available");
        let subcommands =
            workspaces.get_subcommands().map(clap::Command::get_name).collect::<Vec<_>>();

        assert!(!subcommands.contains(&"create"));
        assert!(subcommands.contains(&"list"));
        assert!(subcommands.contains(&"get"));
        assert!(subcommands.contains(&"update"));

        let help = workspaces.render_long_help().to_string();
        assert!(!help.lines().any(|line| line.trim_start().starts_with("create")));
    }

    #[test]
    fn invalid_fields_are_rejected_before_command_execution() {
        let args = [
            "kival",
            "objects",
            "create",
            "00000000-0000-0000-0000-000000000000",
            "--title",
            "Title",
            "--json",
            "--fields",
            "nope",
        ];
        let cli = Cli::try_parse_from(args).expect("command should parse");
        let path = command_path_from_args(args.into_iter().map(OsString::from))
            .expect("command path should parse");
        let error = cli.run(&path).unwrap_err();
        let body = CliErrorBody::from_report(&error);

        assert_eq!(body.code, CliErrorCode::InvalidField);
    }
}
