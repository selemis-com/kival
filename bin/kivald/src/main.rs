//! Entrypoint for `kivald`.

use std::{
    net::SocketAddr,
    num::{NonZeroU32, NonZeroU64},
};

use argx::{Parser, Subcommand};
use kival_cli::{
    args::{datadir::DatadirArgs, log::LogArgs},
    commands::config::ConfigCommand,
    dotenv,
    runner::CliRunner,
    sigsegv,
};
use kival_config::config;

use crate::{
    commands::{admin::AdminCommand, serve::ServeCommand},
    utils::{
        banner::BANNER,
        version::{LONG_VERSION, SHORT_VERSION},
    },
};

pub mod commands;
mod database;
pub mod utils;

config! {
    /// The `kivald` configuration.
    pub struct ServerConfig {
        /// Address the HTTP server should bind to.
        pub listen: SocketAddr = "127.0.0.1:3000".parse().expect("valid default listen address"),

        /// Canonical URL of this Kival deployment.
        pub canonical_url: String = "http://localhost:3000".to_owned(),

        /// Additional exact browser origins allowed to perform passkey ceremonies.
        pub allowed_origins: Vec<String> = Vec::new(),

        /// Maximum PostgreSQL connections owned by this Kival server process.
        pub database_max_connections: NonZeroU32 = NonZeroU32::new(8).expect("non-zero default"),

        /// Maximum seconds a request waits for an available PostgreSQL connection.
        pub database_acquire_timeout_seconds: NonZeroU64 =
            NonZeroU64::new(5).expect("non-zero default"),

        /// Maximum seconds to wait for graceful shutdown.
        pub graceful_shutdown_timeout_seconds: NonZeroU64 =
            NonZeroU64::new(30).expect("non-zero default"),
    }
}

/// The available CLI commands for the `kivald` CLI.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Manage the `kivald` configuration.
    Config(ConfigCommand),

    /// Manage the `kivald` as admin.
    Admin(AdminCommand),

    /// Start the `kivald`.
    Serve(ServeCommand),
}

/// CLI server to host Kival.
///
/// This is the entrypoint to the executable.
#[derive(Debug, Parser)]
#[argx(name = "kivald", version = SHORT_VERSION, long_version = LONG_VERSION)]
pub struct Cli {
    /// The command to run.
    #[argx(subcommand)]
    pub command: Commands,

    /// The data directory to use.
    #[argx(flatten)]
    pub datadir: DatadirArgs,

    /// The logging configuration.
    #[argx(flatten)]
    pub logs: LogArgs,
}

impl Cli {
    /// Execute the CLI command.
    ///
    /// # Errors
    ///
    /// Returns an error if tracing initialization, runtime creation, or command execution fails.
    pub fn run(self) -> eyre::Result<()> {
        let quiet = self.logs.verbosity.is_quiet();
        let _guard = self.logs.init_tracing()?;

        let datadir_path = self.datadir.resolve_datadir();
        let runner = CliRunner::try_default_runtime(datadir_path.into())?;

        match self.command {
            Commands::Config(command) => {
                Ok(runner.run_until_ctrl_c(command.run::<ServerConfig>())?)
            }
            Commands::Admin(command) => runner.run_command_until_ctrl_c(|ctx| command.run(ctx)),
            Commands::Serve(command) => {
                runner.run_command_until_exit(|ctx| command.run(ctx, quiet))
            }
        }
    }
}

fn main() {
    // Install the SIGSEGV handler to print a backtrace on segmentation faults.
    sigsegv::install();

    // Load environment variables from .env file if it exists.
    // If the file is not found it is ignored and is caught by error handling.
    let _ = dotenv::dotenv().ok();

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
    if let Err(err) = Cli::parse().run() {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::admin::AdminSubcommand;

    #[test]
    fn passkey_admin_bootstrap_arguments_parse() {
        let cli = Cli::try_parse_from([
            "kivald",
            "admin",
            "bootstrap",
            "--username",
            "kival-admin",
            "--display-name",
            "Kival Admin",
            "--canonical-url",
            "https://kival.example",
        ])
        .expect("passkey bootstrap should parse");

        let Commands::Admin(admin) = cli.command else {
            panic!("admin command should parse");
        };
        let AdminSubcommand::Bootstrap(bootstrap) = admin.command else {
            panic!("bootstrap subcommand should parse");
        };
        assert_eq!(bootstrap.username, "kival-admin");
        assert_eq!(bootstrap.display_name, "Kival Admin");
        assert_eq!(admin.canonical_url.as_deref(), Some("https://kival.example"));
    }

    #[test]
    fn serve_accepts_canonical_url_and_allowed_origin_configuration() {
        let cli = Cli::try_parse_from([
            "kivald",
            "serve",
            "--canonical-url",
            "https://kival.example:8443",
            "--allowed-origin",
            "https://kival.internal.example",
            "--allowed-origin",
            "https://kival.lan:8443",
        ])
        .expect("WebAuthn origins should parse");
        let Commands::Serve(serve) = cli.command else {
            panic!("serve command should parse");
        };
        assert_eq!(serve.canonical_url.as_deref(), Some("https://kival.example:8443"));
        assert_eq!(
            serve.allowed_origins,
            Some(vec![
                "https://kival.internal.example".to_owned(),
                "https://kival.lan:8443".to_owned(),
            ])
        );

        for flag in ["--webauthn-origin", "--webauthn-rp-id", "--webauthn-rp-name"] {
            assert!(
                Cli::try_parse_from(["kivald", "serve", flag, "unsupported"]).is_err(),
                "{flag} should no longer be accepted"
            );
        }
    }

    #[test]
    fn serve_accepts_non_zero_database_pool_configuration() {
        let cli = Cli::try_parse_from([
            "kivald",
            "serve",
            "--database-max-connections",
            "24",
            "--database-acquire-timeout-seconds",
            "12",
        ])
        .expect("database pool settings should parse");
        let Commands::Serve(serve) = cli.command else {
            panic!("serve command should parse");
        };
        assert_eq!(serve.database_max_connections.map(NonZeroU32::get), Some(24));
        assert_eq!(serve.database_acquire_timeout_seconds.map(NonZeroU64::get), Some(12));

        for flag in ["--database-max-connections", "--database-acquire-timeout-seconds"] {
            assert!(
                Cli::try_parse_from(["kivald", "serve", flag, "0"]).is_err(),
                "{flag} must reject zero"
            );
        }
    }

    #[test]
    fn serve_metrics_listener_requires_an_explicit_address() {
        let cli = Cli::try_parse_from(["kivald", "serve"]).expect("serve should parse");
        let Commands::Serve(serve) = cli.command else {
            panic!("serve command should parse");
        };
        assert!(serve.metrics.is_none());

        assert!(
            Cli::try_parse_from(["kivald", "serve", "--metrics", "--listen", "127.0.0.1:3000"])
                .is_err()
        );
    }

    #[test]
    fn serve_accepts_explicit_metrics_and_http_listeners() {
        let cli = Cli::try_parse_from([
            "kivald",
            "serve",
            "--metrics",
            "0.0.0.0:9100",
            "--listen",
            "0.0.0.0:3000",
        ])
        .expect("explicit listeners should parse");
        let Commands::Serve(serve) = cli.command else {
            panic!("serve command should parse");
        };

        assert_eq!(
            serve.metrics,
            Some("0.0.0.0:9100".parse().expect("metrics address should parse"))
        );
        assert_eq!(serve.listen, Some("0.0.0.0:3000".parse().expect("HTTP address should parse")));
    }

    #[test]
    fn serve_rejects_invalid_or_removed_listener_options() {
        assert!(Cli::try_parse_from(["kivald", "serve", "--metrics", "not-an-address"]).is_err());
        assert!(Cli::try_parse_from(["kivald", "serve", "--listen", "localhost"]).is_err());
        assert!(Cli::try_parse_from(["kivald", "serve", "--metrics-port", "9001"]).is_err());
        assert!(Cli::try_parse_from(["kivald", "serve", "--server-port", "3000"]).is_err());
    }

    #[test]
    fn passkey_operator_user_creation_arguments_parse() {
        let cli = Cli::try_parse_from([
            "kivald",
            "admin",
            "users",
            "create",
            "--username",
            "kival-user",
            "--display-name",
            "Kival User",
        ])
        .expect("passkey user creation should parse");

        let Commands::Admin(admin) = cli.command else {
            panic!("admin command should parse");
        };
        assert!(matches!(admin.command, AdminSubcommand::Users(_)));
    }
    #[test]
    fn operator_workspace_initialization_is_admin_only_cli_surface() {
        let cli = Cli::try_parse_from([
            "kivald",
            "admin",
            "workspaces",
            "create",
            "--name",
            "Kival Demo",
            "--demo",
            "product-engineering",
        ])
        .expect("operator demo workspace creation should parse");

        let Commands::Admin(admin) = cli.command else {
            panic!("admin command should parse");
        };
        assert!(matches!(admin.command, AdminSubcommand::Workspaces(_)));

        assert!(
            Cli::try_parse_from([
                "kivald",
                "admin",
                "workspaces",
                "create",
                "--name",
                "Invalid",
                "--template",
                "project",
                "--demo",
                "product-engineering",
            ])
            .is_err()
        );
    }

    #[test]
    fn deployment_operator_recovery_accepts_user_identity() {
        let cli = Cli::try_parse_from(["kivald", "admin", "recover", "kival-user"])
            .expect("operator recovery should parse");

        let Commands::Admin(admin) = cli.command else {
            panic!("admin command should parse");
        };
        let AdminSubcommand::Recover(recover) = admin.command else {
            panic!("recover subcommand should parse");
        };
        assert_eq!(recover.user, "kival-user");
        assert!(!recover.revoke_api_keys);

        let cli =
            Cli::try_parse_from(["kivald", "admin", "recover", "kival-user", "--revoke-api-keys"])
                .expect("operator recovery API-key revocation option should parse");
        let Commands::Admin(admin) = cli.command else {
            panic!("admin command should parse");
        };
        let AdminSubcommand::Recover(recover) = admin.command else {
            panic!("recover subcommand should parse");
        };
        assert!(recover.revoke_api_keys);
    }
}
