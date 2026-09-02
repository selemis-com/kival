//! A Tokio-based runner for executing CLI commands.

use std::{
    fmt::Display, io::Error, path::PathBuf, pin::pin, sync::mpsc, thread::Builder, time::Duration,
};

use kival_tasks::{PanickedTaskError, Runtime, RuntimeBuildError};
use kival_tracing::{error, info, warn};
use tokio::{signal::ctrl_c, task::JoinHandle};

/// Executes CLI commands.
///
/// Provides utilities for running a CLI command to completion.
#[derive(Debug)]
pub struct CliRunner {
    /// Runtime used to execute command futures and background tasks.
    runtime: Runtime,

    /// Resolved data directory for the current CLI invocation.
    datadir: PathBuf,
}

impl CliRunner {
    /// Attempts to create a new [`CliRunner`] using the default [`Runtime`].
    ///
    /// The default runtime is multi-threaded, with both I/O and time drivers enabled.
    ///
    /// # Errors
    ///
    /// Returns an error if the default runtime cannot be built.
    pub fn try_default_runtime(datadir: PathBuf) -> Result<Self, RuntimeBuildError> {
        Ok(Self { runtime: Runtime::build()?, datadir })
    }

    /// Executes daemon setup on the Tokio runtime, then waits for `SIGINT`, `SIGTERM`, or a
    /// critical task failure.
    ///
    /// Once setup returns successfully, tasks spawned via the [`Runtime`] own the daemon
    /// lifecycle. They are signalled and driven to completion during shutdown.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails, waiting for Ctrl-C fails, or a critical task panics.
    pub fn run_command_until_exit<F, E>(
        self,
        command: impl FnOnce(CliContext) -> F,
    ) -> Result<(), E>
    where
        F: Future<Output = Result<Duration, E>>,
        E: Send + Sync + Display + From<Error> + From<PanickedTaskError> + 'static,
    {
        let (context, task_manager_handle) = cli_context(&self.runtime, self.datadir.clone());

        let mut shutdown_timeout = None;
        let daemon = async {
            shutdown_timeout = Some(command(context).await?);
            std::future::pending::<Result<(), E>>().await
        };

        // Run setup, then keep supervising its critical tasks until an exit signal is received.
        let command_res = self
            .runtime
            .handle()
            .block_on(run_to_completion_or_panic(task_manager_handle, run_until_ctrl_c(daemon)));

        if let Err(err) = &command_res {
            error!(target: "kival::cli", %err, "Shutting down due to error");
        } else {
            info!(target: "kival::cli", "Shutting down gracefully");
        }
        // After the command has finished, an exit signal was received, or a critical task failed,
        // fire the shutdown signal and wait for tasks registered for graceful shutdown.
        self.runtime
            .graceful_shutdown_with_timeout(shutdown_timeout.unwrap_or(Duration::from_secs(30)));

        runtime_shutdown(self.runtime, true);

        command_res
    }

    /// Executes the given _async_ command on the tokio runtime until the command future resolves or
    /// until the process receives a `SIGINT` or `SIGTERM` signal.
    ///
    /// The command is provided with a [`CliContext`], but tasks spawned by the command via the
    /// [`Runtime`] are not driven through the task-manager shutdown path. Use
    /// [`Self::run_command_until_exit`] for daemon-style commands that require coordinated
    /// background task shutdown.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails or waiting for Ctrl-C fails.
    pub fn run_command_until_ctrl_c<F, E>(
        self,
        command: impl FnOnce(CliContext) -> F,
    ) -> Result<(), E>
    where
        F: Future<Output = Result<(), E>>,
        E: Send + Sync + From<Error> + 'static,
    {
        let context =
            CliContext { task_executor: self.runtime.clone(), datadir: self.datadir.clone() };

        self.runtime.handle().block_on(run_until_ctrl_c(command(context)))?;
        runtime_shutdown(self.runtime, false);

        Ok(())
    }

    /// Executes a regular future until completion or until external signal received.
    ///
    /// # Errors
    ///
    /// Returns an error if waiting for Ctrl-C fails or the future returns an error.
    pub fn run_until_ctrl_c<F, E>(self, fut: F) -> Result<(), E>
    where
        F: Future<Output = Result<(), E>>,
        E: Send + Sync + From<Error> + 'static,
    {
        self.runtime.handle().block_on(run_until_ctrl_c(fut))?;
        Ok(())
    }
}

/// Extracts the task manager handle from the runtime and creates the [`CliContext`].
fn cli_context(
    runtime: &Runtime,
    datadir: PathBuf,
) -> (CliContext, JoinHandle<Result<(), PanickedTaskError>>) {
    let handle =
        runtime.take_task_manager_handle().expect("Runtime must contain a TaskManager handle");

    let context = CliContext { task_executor: runtime.clone(), datadir };

    (context, handle)
}

/// Additional context provided by the [`CliRunner`] when executing commands
#[derive(Debug)]
pub struct CliContext {
    /// Used to execute/spawn tasks.
    pub task_executor: Runtime,

    /// Resolved data directory for the current CLI invocation.
    pub datadir: PathBuf,
}

/// Runs the given future to completion or until critical-task supervision terminates.
///
/// Returns the error if a critical task panics, the task manager stops unexpectedly, or the given
/// future returns an error.
async fn run_to_completion_or_panic<F, E>(
    task_manager_handle: JoinHandle<Result<(), PanickedTaskError>>,
    fut: F,
) -> Result<(), E>
where
    F: Future<Output = Result<(), E>>,
    E: Send + Sync + From<Error> + From<PanickedTaskError> + 'static,
{
    let fut = pin!(fut);
    tokio::select! {
        task_manager_result = task_manager_handle => {
            match task_manager_result {
                Ok(Err(panicked_error)) => return Err(panicked_error.into()),
                Ok(Ok(())) => {
                    return Err(Error::other("critical task manager stopped unexpectedly").into());
                }
                Err(error) => {
                    return Err(Error::other(format!("critical task manager failed: {error}")).into());
                }
            }
        },
        res = fut => res?,
    }
    Ok(())
}

/// Runs the future to completion or until:
/// - `ctrl-c` is received.
/// - `SIGTERM` is received (unix only).
async fn run_until_ctrl_c<F, E>(fut: F) -> Result<(), E>
where
    F: Future<Output = Result<(), E>>,
    E: Send + Sync + 'static + From<Error>,
{
    let ctrl_c = ctrl_c();

    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut stream = signal(SignalKind::terminate())?;
        let sigterm = stream.recv();
        let sigterm = pin!(sigterm);
        let ctrl_c = pin!(ctrl_c);
        let fut = pin!(fut);

        tokio::select! {
            _ = ctrl_c => {
                info!(target: "kival::cli", "Received ctrl-c");
            },
            _ = sigterm => {
                info!(target: "kival::cli", "Received SIGTERM");
            },
            res = fut => res?,
        }
    }

    #[cfg(not(unix))]
    {
        let ctrl_c = pin!(ctrl_c);
        let fut = pin!(fut);

        tokio::select! {
            _ = ctrl_c => {
                info!(target: "kival::cli", "Received ctrl-c");
            },
            res = fut => res?,
        }
    }

    Ok(())
}

/// Default timeout for waiting on the tokio runtime to shut down.
const DEFAULT_RUNTIME_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Shut down the given [`Runtime`], and wait for it if `wait` is set.
///
/// Dropping the runtime on the current thread could block due to tokio pool teardown.
/// Instead, we drop it on a separate thread and optionally wait for completion.
fn runtime_shutdown(rt: Runtime, wait: bool) {
    let (tx, rx) = mpsc::channel();
    Builder::new()
        .name("rt-shutdown".to_owned())
        .spawn(move || {
            drop(rt);
            let _ = tx.send(());
        })
        .unwrap();

    if wait {
        let _ = rx.recv_timeout(DEFAULT_RUNTIME_SHUTDOWN_TIMEOUT).inspect_err(|err| {
            warn!(target: "kival::cli", %err, "runtime shutdown timed out");
        });
    }
}
