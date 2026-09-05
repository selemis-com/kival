//! Entrypoint for `kival`.

use std::{ffi::OsString, fmt, path::Path};

use argx::{Defaults, Environment, Parser, Subcommand, Toml};
use eyre::Result;
use kival_cli::{args::datadir::DatadirArgs, runner::CliRunner, sigsegv};
use url::Url;

use crate::{
    commands::{
        admin::AdminCommand, comments::CommentsCommand, completions::CompletionsCommand,
        events::EventsCommand, groups::GroupsCommand, objects::ObjectsCommand,
        search::SearchCommand, server::ServerCommand, whoami::WhoamiCommand,
        workspaces::WorkspacesCommand,
    },
    utils::{
        error::{CliFailure, RenderedCommandError, erase_command_error, print_json_error},
        fields::parse_projection,
        output::{OutputFormat, OutputMode},
        version::{LONG_VERSION, SHORT_VERSION},
    },
};

pub mod commands;
pub mod utils;

/// Configuration used by the Kival CLI client.
#[derive(Clone, argx::Config)]
#[argx(prefix = "KIVAL")]
pub struct ClientConfig {
    /// Kival server root URL.
    #[argx(default = Url::parse("http://localhost:3000").expect("default client URL must be valid"))]
    pub url: Url,

    /// Default API key used when no explicit credential is supplied.
    #[argx(default)]
    pub api_key: Option<String>,
}

impl fmt::Debug for ClientConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientConfig")
            .field("url", &self.url)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl ClientConfig {
    /// Loads effective client configuration from defaults, an optional TOML file, and environment.
    ///
    /// # Errors
    ///
    /// Returns an error when a configured source cannot be read or resolved.
    pub fn load(path: &Path) -> Result<Self> {
        let loader = Self::loader().layer(Defaults);
        if path.try_exists()? {
            Ok(loader.layer(Environment).layer(Toml::new(path)).layer(Environment).resolve()?)
        } else {
            Ok(loader.layer(Environment).resolve()?)
        }
    }
}

/// The available CLI commands for the `kival` CLI.
#[derive(Debug, Subcommand)]
#[argx(schema)]
pub enum Commands {
    /// Manage workspaces, memberships, linked groups, events, and workspace graphs.
    Workspaces(WorkspacesCommand),

    /// Manage workspace objects, versions, relationships, attachments, and access.
    Objects(ObjectsCommand),

    /// Participate in object comments and discussion threads.
    Comments(CommentsCommand),

    /// Manage reusable access groups and their memberships.
    Groups(GroupsCommand),

    /// Inspect activity visible to the current user.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["admin"],
        })
    )]
    Events(EventsCommand),

    /// Search indexed visible workspace content using auto, text, literal, or exact matching.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["objects:read"],
        })
    )]
    Search(SearchCommand),

    /// Show the identity associated with the resolved API key.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
        })
    )]
    Whoami(WhoamiCommand),

    /// Check server health and readiness.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
        })
    )]
    Server(ServerCommand),

    /// Generate shell completion script text.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
        })
    )]
    Completions(CompletionsCommand),

    /// Manage Kival as admin.
    Admin(AdminCommand),
}

/// CLI client to interact with Kival.
///
/// ```text
///     __ __ __            __
///    / //_//_/_  ______ _/ /
///   / ,<  / / | / / __ `/ /
///  / /| |/ /| |/ / /_/ / /
/// /_/ |_/_/ |___/\__,_/_/
/// ```
#[derive(Parser)]
#[argx(name = "kival", version = SHORT_VERSION, long_version = LONG_VERSION, schema)]
pub struct Cli {
    /// The command to run.
    #[argx(subcommand)]
    pub command: Commands,

    /// # Data directory
    #[argx(flatten)]
    pub datadir: DatadirArgs,

    /// Use this API key for the current invocation.
    #[argx(long, global)]
    pub api_key: Option<String>,

    /// Override the configured Kival server root URL.
    #[argx(long, global)]
    pub url: Option<Url>,

    /// Output format.
    #[argx(short = 'O', long, value_enum, global, default = OutputFormat::Text)]
    pub output: OutputFormat,

    /// Select comma-separated fields from JSON output.
    ///
    /// Nested fields use dot-separated paths.
    #[argx(short = 'F', long, global, delimited)]
    pub fields: Vec<String>,
}

impl fmt::Debug for Cli {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cli")
            .field("command", &self.command)
            .field("datadir", &self.datadir)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("url", &self.url)
            .field("output", &self.output)
            .field("fields", &self.fields)
            .finish()
    }
}

impl Cli {
    /// Execute the CLI command.
    ///
    /// # Errors
    ///
    /// Returns an error if runtime creation or command execution fails.
    pub fn run(self) -> Result<()> {
        let Self { command, datadir, api_key, url, output, fields } = self;

        let projection = parse_projection(&fields)?;
        if projection.is_some() && output != OutputFormat::Json {
            return Err(CliFailure::invalid_argument("--fields requires --output json").into());
        }
        let output = OutputMode::from_options(output, projection);

        utils::credentials::set_command_overrides(api_key, url)?;

        if matches!(command, Commands::Completions(_)) && output != OutputMode::Text {
            return Err(CliFailure::invalid_argument(
                "completions output is shell script text and does not support structured output",
            )
            .into());
        }

        match command {
            Commands::Completions(command) => {
                Ok(command.run(&output).map_err(erase_command_error)?)
            }
            command => {
                let runner = create_runner(&datadir)?;

                match command {
                    Commands::Whoami(command) => {
                        Ok(runner.run_command_until_ctrl_c(async move |ctx| {
                            command.run(ctx, output).await.map_err(erase_command_error)?;
                            Ok::<(), eyre::Report>(())
                        })?)
                    }
                    Commands::Admin(command) => {
                        Ok(runner.run_command_until_ctrl_c(|ctx| command.run(ctx, output))?)
                    }
                    Commands::Comments(command) => {
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
                            command.run(ctx, output).await.map_err(erase_command_error)?;
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
                    Commands::Completions(_) => {
                        unreachable!("completions are handled before runtime initialization")
                    }
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

/// Returns whether raw CLI arguments request JSON output before parsing succeeds.
fn json_output_requested(args: impl IntoIterator<Item = OsString>) -> bool {
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }

        let Some(arg) = arg.to_str() else {
            continue;
        };

        if let Some(value) = arg.strip_prefix("--output=") {
            if value == "json" {
                return true;
            }
            continue;
        }

        if arg == "--output" || arg == "-O" {
            if args.next().is_some_and(|value| value == "json") {
                return true;
            }
            continue;
        }

        if let Some(value) = arg.strip_prefix("-O")
            && value.strip_prefix('=').unwrap_or(value) == "json"
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

    let json_requested = json_output_requested(std::env::args_os().skip(1));
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) if error.exit_code() == 0 => error.exit(),
        Err(error) => {
            if json_requested {
                let error_body = CliFailure::invalid_argument(error.to_string());
                print_json_error(&error_body);
                std::process::exit(error.exit_code());
            }
            error.exit();
        }
    };
    let json_errors = cli.output == OutputFormat::Json;
    if let Err(error) = cli.run() {
        if json_errors {
            if let Some(error) = error.downcast_ref::<RenderedCommandError>() {
                print_json_error(error.value());
            } else {
                print_json_error(&CliFailure::from_report(&error));
            }
        } else {
            eprintln!("Error: {error}");
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_config_debug_redacts_api_key() {
        let config = ClientConfig {
            url: Url::parse("https://kival.example.com").unwrap(),
            api_key: Some("kvl_super_secret".to_owned()),
        };

        let rendered = format!("{config:?}");
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("kvl_super_secret"));
    }

    #[test]
    fn raw_output_scan_detects_json_forms() {
        for args in [
            vec!["--output", "json"],
            vec!["--output=json"],
            vec!["-O", "json"],
            vec!["-Ojson"],
            vec!["-O=json"],
        ] {
            assert!(json_output_requested(args.into_iter().map(OsString::from)));
        }
    }

    #[test]
    fn raw_output_scan_stops_at_double_dash() {
        assert!(!json_output_requested(["--", "--output=json"].into_iter().map(OsString::from),));
    }

    #[test]
    fn admin_is_a_normal_command() {
        let cli = Cli::try_parse_from(["kival", "admin", "users", "list"])
            .expect("admin should be visible and parse normally");
        assert!(matches!(cli.command, Commands::Admin(_)));
    }
}
