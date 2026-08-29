//! Entrypoint for `kival`.

use argx::{Args, Parser, Subcommand};
use eyre::Result;
use kival_cli::{
    args::datadir::DatadirArgs, commands::config::ConfigCommand, runner::CliRunner, sigsegv,
};
use kival_config::config;
use url::Url;

use crate::{
    commands::{
        admin::AdminCommand, comments::CommentsCommand, completions::CompletionsCommand,
        events::EventsCommand, groups::GroupsCommand, objects::ObjectsCommand,
        search::SearchCommand, server::ServerCommand, whoami::WhoamiCommand,
        workspaces::WorkspacesCommand,
    },
    utils::{
        error::CliError,
        fields::parse_projection,
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
    #[argx(flatten)]
    command: ConfigCommand,
}

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
#[derive(Debug, Subcommand)]
#[argx(schema)]
pub enum Commands {
    /// Manage workspaces, memberships, linked groups, events, and workspace graphs.
    #[argx(name = "workspaces")]
    Workspaces(WorkspacesCommand),

    /// Manage workspace objects, versions, relationships, attachments, and access.
    #[argx(name = "objects")]
    Objects(ObjectsCommand),

    /// Participate in object comments and discussion threads.
    #[argx(name = "comments")]
    Comments(CommentsCommand),

    /// Manage reusable access groups and their memberships.
    #[argx(name = "groups")]
    Groups(GroupsCommand),

    /// Inspect activity visible to the current user.
    #[argx(name = "events")]
    Events(EventsCommand),

    /// Search indexed visible workspace content using auto, text, literal, or exact matching.
    #[argx(name = "search")]
    Search(SearchCommand),

    /// Show the identity associated with the resolved API key.
    #[argx(name = "whoami")]
    Whoami(WhoamiCommand),

    /// Check server health and readiness.
    #[argx(name = "server")]
    Server(ServerCommand),

    /// Print the effective Kival client configuration as TOML.
    #[argx(name = "config")]
    Config(ClientConfigCommand),

    /// Generate shell completion script text.
    #[argx(name = "completions")]
    Completions(CompletionsCommand),

    /// Manage Kival as admin.
    #[argx(name = "admin")]
    Admin(AdminCommand),
}

/// CLI client to interact with Kival.
///
/// This is the entrypoint to the executable.
#[derive(Debug, Parser)]
#[argx(name = "kival", version = SHORT_VERSION, long_version = LONG_VERSION, schema)]
pub struct Cli {
    /// The command to run.
    #[argx(subcommand)]
    pub command: Commands,

    /// Emit JSON output instead of human-readable output, where supported.
    #[argx(short, long, global)]
    pub json: bool,

    /// Select a property from JSON output. May be repeated.
    ///
    /// Nested paths use dot notation. This is an output projection option, not a search filter.
    #[argx(short, long = "field", global, requires = "json")]
    pub field: Vec<String>,

    /// The data directory to use.
    #[argx(flatten)]
    pub datadir: DatadirArgs,

    /// Use this API key for the current invocation.
    #[argx(long, global)]
    pub api_key: Option<String>,

    /// Override the configured Kival server root URL.
    #[argx(long, global)]
    pub url: Option<Url>,
}

impl Cli {
    /// Execute the CLI command.
    ///
    /// # Errors
    ///
    /// Returns an error if runtime creation or command execution fails.
    pub fn run(self) -> Result<()> {
        let Self { command, json, field, datadir, api_key, url } = self;

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

        let projection = parse_projection(&field)?;
        let output = OutputMode::from_options(json, projection);

        match command {
            Commands::Completions(command) => command.run(&output),
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
                    Commands::Completions(_) => unreachable!(
                        "completions are handled before runtime initialization"
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

    // Parse command-line arguments and run the appropriate command.
    if let Err(err) = Cli::parse().run() {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use argx::Parser as _;

    use super::{Cli, Commands};

    #[test]
    fn field_is_repeatable_and_requires_json() {
        assert!(Cli::try_parse_from(["kival", "whoami", "--field", "id"]).is_err());

        let cli = Cli::try_parse_from([
            "kival", "whoami", "--json", "--field", "id", "--field", "username",
        ])
        .expect("repeated fields should parse");
        assert_eq!(cli.field, ["id", "username"]);
    }

    #[test]
    fn admin_is_a_normal_command() {
        let cli = Cli::try_parse_from(["kival", "admin", "users", "list"])
            .expect("admin should be visible and parse normally");
        assert!(matches!(cli.command, Commands::Admin(_)));
    }
}
