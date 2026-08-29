//! The `serve` command for the `kivald` CLI.

use std::{
    net::SocketAddr,
    num::{NonZeroU32, NonZeroU64},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use argx::Args;
use eyre::Result;
use kival_cli::{
    commands::config::{DEFAULT_CONFIG_FILENAME, load_config},
    runner::CliContext,
};
use kival_kernel::{DatabasePoolSettings, open_pool_with_settings};
use kival_metrics::{
    Hooks, VersionInfo, counter, describe_counter, describe_gauge, gauge, start_metrics_server,
};
use kival_server::{Server, ServerState, WebAuthnConfig};
use kival_storage::BlobStore;
use kival_tasks::DurableTasks;
use kival_tracing::{error, info};

use crate::{
    ServerConfig,
    database::connect_options_from_env,
    utils::{
        banner::BANNER,
        version::{
            KIVAL_BUILD_PROFILE, KIVAL_BUILD_TIMESTAMP, KIVAL_CARGO_FEATURES,
            KIVAL_CARGO_TARGET_TRIPLE, KIVAL_GIT_SHA, KIVAL_RELEASE_VERSION, LONG_VERSION,
            SHORT_VERSION,
        },
    },
};

/// The `serve` command arguments.
#[derive(Debug, Args)]
pub struct ServeCommand {
    /// The path to the configuration file to use.
    #[argx(long)]
    pub config: Option<PathBuf>,

    /// Enable Prometheus metrics on this address.
    #[argx(long)]
    pub metrics: Option<SocketAddr>,

    /// Address the HTTP server should bind to.
    #[argx(long)]
    pub listen: Option<SocketAddr>,

    /// Canonical URL of this Kival deployment.
    #[argx(long)]
    pub canonical_url: Option<String>,

    /// Additional exact browser origins allowed to perform passkey ceremonies.
    #[argx(long = "allowed-origin")]
    pub allowed_origins: Option<Vec<String>>,

    /// Maximum PostgreSQL connections owned by this Kival process.
    #[argx(long = "database-max-connections")]
    pub database_max_connections: Option<NonZeroU32>,

    /// Maximum seconds a request waits for an available PostgreSQL connection.
    #[argx(long = "database-acquire-timeout-seconds")]
    pub database_acquire_timeout_seconds: Option<NonZeroU64>,

    /// Maximum seconds to wait for graceful shutdown.
    #[argx(long = "graceful-shutdown-timeout-seconds")]
    pub graceful_shutdown_timeout_seconds: Option<NonZeroU64>,
}

/// Minimum pool size for a server: realtime holds one connection while request and worker work
/// must still be able to acquire another.
const MIN_SERVER_DATABASE_CONNECTIONS: u32 = 2;

/// Resolves and validates the PostgreSQL pool budget for the server process.
fn database_pool_settings(config: &ServerConfig) -> Result<DatabasePoolSettings> {
    let max_connections = config.database_max_connections;
    eyre::ensure!(
        max_connections.get() >= MIN_SERVER_DATABASE_CONNECTIONS,
        "database_max_connections must be at least {MIN_SERVER_DATABASE_CONNECTIONS} for `kivald serve` because realtime holds one pool connection"
    );

    Ok(DatabasePoolSettings {
        max_connections,
        acquire_timeout: Duration::from_secs(config.database_acquire_timeout_seconds.get()),
    })
}

impl ServeCommand {
    /// Run the `serve` command.
    ///
    /// # Errors
    ///
    /// Returns an error if configuration loading, database or blob-store setup, or metrics-server
    /// startup fails.
    ///
    /// # Panics
    ///
    /// The supervised HTTP or metrics task panics if its server fails after setup so the task
    /// manager can propagate the critical failure to the CLI runner.
    pub async fn run(&self, ctx: CliContext, quiet: bool) -> Result<Duration> {
        let Self { config, metrics, .. } = self;

        if !quiet {
            println!("{BANNER}\n\n{LONG_VERSION}\n");
        }

        let config_path =
            config.clone().unwrap_or_else(|| ctx.datadir.join(DEFAULT_CONFIG_FILENAME));
        let config = match load_config::<ServerConfig>(&config_path) {
            Ok(mut config) => {
                if let Some(listen) = self.listen {
                    config.listen = listen;
                }
                if let Some(canonical_url) = &self.canonical_url {
                    config.canonical_url.clone_from(canonical_url);
                }
                if let Some(allowed_origins) = &self.allowed_origins {
                    config.allowed_origins.clone_from(allowed_origins);
                }
                if let Some(max_connections) = self.database_max_connections {
                    config.database_max_connections = max_connections;
                }
                if let Some(timeout) = self.database_acquire_timeout_seconds {
                    config.database_acquire_timeout_seconds = timeout;
                }
                if let Some(timeout) = self.graceful_shutdown_timeout_seconds {
                    config.graceful_shutdown_timeout_seconds = timeout;
                }
                config
            },
            Err(err) => {
                error!(target: "kival::cli", path = %config_path.display(), error = ?err, "Failed to load configuration file");
                return Err(err);
            }
        };

        info!(target: "kival::cli", "Configuration loaded from: {}", config_path.display());
        info!(target: "kival::cli", version = ?SHORT_VERSION, "Starting Kival server");

        let canonical_url = config.canonical_url.clone();
        let webauthn = WebAuthnConfig::from_canonical_url_with_allowed_origins(
            &canonical_url,
            &config.allowed_origins,
        )?;

        let database_pool_settings = database_pool_settings(&config)?;
        let db_pool =
            open_pool_with_settings(connect_options_from_env()?, database_pool_settings).await?;

        info!(
            target: "kival::cli",
            max_connections = database_pool_settings.max_connections.get(),
            acquire_timeout_seconds = database_pool_settings.acquire_timeout.as_secs(),
            "Database connection established"
        );

        let durable_tasks = DurableTasks::bootstrap(db_pool.clone()).await?;
        info!(
            target: "kival::cli",
            queue = durable_tasks.queue().name(),
            "Durable task queue ready"
        );

        let blob_path = ctx.datadir.join("blobs");
        let blob_store = BlobStore::filesystem(&blob_path)?;
        info!(target: "kival::cli", "Blob store opened at: {}", blob_path.display());

        if let Some(metrics_address) = metrics {
            let db_pool_metrics = db_pool.clone();
            let durable_task_metrics = durable_tasks.queue().metrics();
            let hooks = Hooks::builder()
                .with_hook(move || {
                    describe_gauge!("db.pool.size", "Current number of database pool connections.");
                    describe_gauge!(
                        "db.pool.idle",
                        "Current number of idle database pool connections."
                    );
                    describe_gauge!(
                        "db.pool.active",
                        "Current number of checked-out database pool connections."
                    );
                    describe_gauge!(
                        "db.pool.max",
                        "Configured maximum number of database pool connections."
                    );
                    describe_gauge!(
                        "db.pool.utilization",
                        "Fraction of the database pool currently checked out."
                    );
                    describe_counter!(
                        "durable_tasks.claimed_runs_total",
                        "Steda runs successfully claimed by this Kival process."
                    );
                    describe_counter!(
                        "durable_tasks.claim_errors_total",
                        "Failed Steda run claim attempts in this Kival process."
                    );
                    describe_counter!(
                        "durable_tasks.executions_total",
                        "Steda task execution attempts started by this Kival process."
                    );
                    describe_counter!(
                        "durable_tasks.completed_executions_total",
                        "Steda task execution attempts completed successfully by this Kival process."
                    );
                    describe_counter!(
                        "durable_tasks.failed_executions_total",
                        "Steda task execution attempts that failed in this Kival process."
                    );
                    describe_counter!(
                        "durable_tasks.lease_lost_executions_total",
                        "Steda task execution attempts that lost their lease in this Kival process."
                    );
                    describe_counter!(
                        "durable_tasks.cancelled_executions_total",
                        "Steda task execution attempts cancelled in this Kival process."
                    );
                    describe_counter!(
                        "durable_tasks.suspended_executions_total",
                        "Steda task execution attempts durably suspended in this Kival process."
                    );
                    describe_counter!(
                        "durable_tasks.unhandled_executions_total",
                        "Steda task execution attempts that ended without a bounded outcome in this Kival process."
                    );
                    describe_counter!(
                        "durable_tasks.execution_duration_nanoseconds_total",
                        "Cumulative Steda task execution time observed by this Kival process."
                    );

                    let size = db_pool_metrics.size() as f64;
                    let idle = db_pool_metrics.num_idle() as f64;
                    let active = (size - idle).max(0.0);
                    let max = f64::from(db_pool_metrics.options().get_max_connections());

                    gauge!("db.pool.size").set(size);
                    gauge!("db.pool.idle").set(idle);
                    gauge!("db.pool.active").set(active);
                    gauge!("db.pool.max").set(max);
                    gauge!("db.pool.utilization").set(if max > 0.0 { active / max } else { 0.0 });

                    counter!("durable_tasks.claimed_runs_total")
                        .absolute(durable_task_metrics.claimed_runs());
                    counter!("durable_tasks.claim_errors_total")
                        .absolute(durable_task_metrics.claim_errors());
                    counter!("durable_tasks.executions_total")
                        .absolute(durable_task_metrics.executions());
                    counter!("durable_tasks.completed_executions_total")
                        .absolute(durable_task_metrics.completed_executions());
                    counter!("durable_tasks.failed_executions_total")
                        .absolute(durable_task_metrics.failed_executions());
                    counter!("durable_tasks.lease_lost_executions_total")
                        .absolute(durable_task_metrics.lease_lost_executions());
                    counter!("durable_tasks.cancelled_executions_total")
                        .absolute(durable_task_metrics.cancelled_executions());
                    counter!("durable_tasks.suspended_executions_total")
                        .absolute(durable_task_metrics.suspended_executions());
                    counter!("durable_tasks.unhandled_executions_total")
                        .absolute(durable_task_metrics.unhandled_executions());
                    counter!("durable_tasks.execution_duration_nanoseconds_total")
                        .absolute(durable_task_metrics.execution_duration_nanoseconds());
                })
                .build();

            let metrics_handle = start_metrics_server(
                "kival",
                metrics_address,
                VersionInfo {
                    version: KIVAL_RELEASE_VERSION,
                    build_timestamp: KIVAL_BUILD_TIMESTAMP,
                    cargo_features: KIVAL_CARGO_FEATURES,
                    git_sha: KIVAL_GIT_SHA,
                    target_triple: KIVAL_CARGO_TARGET_TRIPLE,
                    build_profile: KIVAL_BUILD_PROFILE,
                },
                hooks,
                ctx.task_executor.on_shutdown_signal().clone(),
            )
            .await?;

            ctx.task_executor.spawn_critical_with_graceful_shutdown_signal(
                "metrics-server",
                async move |shutdown| {
                    // The endpoint receives the runtime's shared shutdown signal directly. Keep
                    // this guard alive until its task has drained and exited.
                    let _shutdown = shutdown;
                    match metrics_handle.await {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => panic!("metrics server failed: {error:#}"),
                        Err(error) => panic!("metrics server task failed: {error}"),
                    }
                },
            );
        }

        let db_pool_shutdown = db_pool.clone();
        let server = Server::new(Arc::new(ServerState::with_webauthn(
            db_pool,
            blob_store,
            durable_tasks,
            webauthn,
        )));
        let server_address = config.listen();

        ctx.task_executor.spawn_critical_with_graceful_shutdown_signal(
            "http-server",
            async move |shutdown| {
                // Axum only needs a unit-output signal. The original graceful handle remains in
                // this task so its guard covers both request draining and database pool closure.
                let shutdown_signal = shutdown.clone().ignore_guard();
                let result =
                    server.run_with_graceful_shutdown(server_address, shutdown_signal).await;

                db_pool_shutdown.close().await;
                drop(shutdown);

                if let Err(error) = result {
                    panic!("HTTP server failed: {error}");
                }
            },
        );

        Ok(Duration::from_secs(config.graceful_shutdown_timeout_seconds.get()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_pool_requires_capacity_beyond_realtime_listener() {
        let mut config = ServerConfig::loader()
            .layer(argx::Defaults)
            .resolve()
            .expect("defaults should resolve");
        config.database_max_connections = NonZeroU32::new(1).expect("non-zero test value");

        let error = database_pool_settings(&config)
            .expect_err("one connection would be monopolized by the realtime listener");
        assert!(error.to_string().contains("must be at least 2"));
    }
}
