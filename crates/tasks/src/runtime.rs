//! Centralized async execution management.
//!
//! Provides [`Runtime`], a cheaply cloneable handle that owns a Tokio runtime and offers task
//! spawning with shutdown awareness and panic monitoring.

use std::{
    any::Any,
    io::Error,
    panic::AssertUnwindSafe,
    pin::Pin,
    sync::{
        Arc, Mutex, Once,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, ready},
    thread,
    time::{Duration, Instant},
};

use futures_util::{FutureExt, TryFutureExt};
use kival_metrics::{counter, describe_counter, describe_gauge, gauge};
use kival_tracing::{Instrument, debug, error, info, warn};
use tokio::{
    runtime::{self, Handle, Runtime as TokioRuntime},
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    task::JoinHandle,
};

use crate::shutdown::{GracefulShutdown, GracefulShutdownGuard, Shutdown, Signal, signal};

/// Ensures task runtime metric descriptions are emitted once.
static DESCRIBE_TASK_METRICS: Once = Once::new();

/// Register task runtime metric descriptions once per process.
fn describe_task_metrics() {
    DESCRIBE_TASK_METRICS.call_once(|| {
        describe_counter!("tasks.spawned_total", "Tasks spawned through the Kival runtime.");
        describe_counter!(
            "tasks.panics_total",
            "Critical task panics observed by the Kival runtime."
        );
        describe_counter!("tasks.graceful_shutdown_timeouts_total", "Graceful shutdown timeouts.");
        describe_gauge!("tasks.graceful_shutdown.active", "Graceful shutdown tasks still active.");
    });
}

/// Record one runtime task spawn.
fn record_task_spawn(kind: &'static str) {
    describe_task_metrics();
    counter!("tasks.spawned_total", "kind" => kind).increment(1);
}

/// Record one critical task panic.
fn record_task_panic(name: &'static str) {
    describe_task_metrics();
    counter!("tasks.panics_total", "name" => name).increment(1);
}

/// Record the current number of active graceful-shutdown tasks.
fn record_graceful_shutdown_active(active: usize) {
    describe_task_metrics();
    gauge!("tasks.graceful_shutdown.active").set(active as f64);
}

/// Record a graceful shutdown timeout.
fn record_graceful_shutdown_timeout() {
    describe_task_metrics();
    counter!("tasks.graceful_shutdown_timeouts_total").increment(1);
}

/// Error returned when [`Runtime::build`] fails.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeBuildError {
    /// Failed to build the Tokio runtime.
    #[error("failed to build tokio runtime: {0}")]
    TokioBuild(#[from] Error),
}

/// Error with the name of the task that panicked and an error downcasted to string, if
/// possible.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub struct PanickedTaskError {
    /// The static name of the critical task that panicked.
    pub task_name: &'static str,
    /// The panic payload, downcast to [`String`] if possible.
    pub error: Option<String>,
}

impl std::fmt::Display for PanickedTaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = self.task_name;
        match &self.error {
            Some(e) => write!(f, "Critical task `{name}` panicked: `{e}`"),
            None => write!(f, "Critical task `{name}` panicked"),
        }
    }
}

impl PanickedTaskError {
    /// Creates an error from a critical task panic payload.
    fn new(task_name: &'static str, error: Box<dyn Any + Send>) -> Self {
        let error = match error.downcast::<String>() {
            Ok(s) => Some(*s),
            Err(error) => error.downcast::<&'static str>().map_or(None, |s| Some((*s).to_owned())),
        };
        Self { task_name, error }
    }
}

/// Events sent to the [`TaskManager`]'s background future.
#[derive(Debug)]
enum TaskEvent {
    /// A critical task has panicked.
    Panic(PanickedTaskError),
    /// Request a graceful shutdown of the [`TaskManager`].
    GracefulShutdown,
}

/// Monitors critical tasks for panics and manages graceful shutdown.
///
/// The future resolves with `Err(PanickedTaskError)` if a critical task panicked, or
/// `Ok(())` when a graceful shutdown was requested. Automatically spawned as a background
/// task by [`Runtime::build`]; retrieve the join handle via
/// [`Runtime::take_task_manager_handle`] if you need to react to panics.
#[derive(Debug)]
#[must_use = "TaskManager must be polled to monitor critical tasks"]
struct TaskManager {
    /// Receiver for panic and graceful-shutdown events.
    task_events_rx: UnboundedReceiver<TaskEvent>,
    /// The shutdown signal fired when the manager terminates.
    signal: Option<Signal>,
}

impl Future for TaskManager {
    type Output = Result<(), PanickedTaskError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match ready!(self.as_mut().get_mut().task_events_rx.poll_recv(cx)) {
            Some(TaskEvent::Panic(err)) => Poll::Ready(Err(err)),
            Some(TaskEvent::GracefulShutdown) | None => {
                if let Some(signal) = self.get_mut().signal.take() {
                    signal.fire();
                }
                Poll::Ready(Ok(()))
            }
        }
    }
}

/// Shared runtime state behind cloneable [`Runtime`] handles.
struct RuntimeInner {
    /// Owned Tokio runtime kept alive via the `Arc<RuntimeInner>`.
    _tokio_runtime: TokioRuntime,
    /// Handle to the Tokio runtime.
    handle: Handle,
    /// Receiver of the shutdown signal.
    on_shutdown: Shutdown,
    /// Sender used to dispatch events to the [`TaskManager`].
    task_events_tx: UnboundedSender<TaskEvent>,
    /// Number of currently active [`GracefulShutdown`] tasks.
    graceful_tasks: Arc<AtomicUsize>,
    /// Background [`TaskManager`] join handle. Can be taken via
    /// [`Runtime::take_task_manager_handle`].
    task_manager_handle: Mutex<Option<JoinHandle<Result<(), PanickedTaskError>>>>,
}

/// A cheaply cloneable handle to an owned Tokio runtime, a shutdown signal, and internal panic
/// monitoring.
#[derive(Clone)]
pub struct Runtime(Arc<RuntimeInner>);

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime").field("handle", &self.0.handle).finish()
    }
}

impl Runtime {
    /// Builds a multi-threaded Tokio runtime and its task manager.
    ///
    /// # Errors
    ///
    /// Returns an error if the Tokio runtime cannot be built.
    pub fn build() -> Result<Self, RuntimeBuildError> {
        let tokio_runtime =
            runtime::Builder::new_multi_thread().enable_all().thread_name("tokio-rt").build()?;
        let handle = tokio_runtime.handle().clone();
        let (task_events_tx, task_events_rx) = unbounded_channel();
        let (signal, on_shutdown) = signal();
        let graceful_tasks = Arc::new(AtomicUsize::new(0));
        let manager = TaskManager { task_events_rx, signal: Some(signal) };

        let task_manager_handle = handle.spawn(async move {
            let result = manager.await;
            if let Err(err) = &result {
                debug!("{err}");
            }
            result
        });

        let inner = RuntimeInner {
            _tokio_runtime: tokio_runtime,
            handle,
            on_shutdown,
            task_events_tx,
            graceful_tasks,
            task_manager_handle: Mutex::new(Some(task_manager_handle)),
        };

        Ok(Self(Arc::new(inner)))
    }

    /// Returns the Tokio runtime [`Handle`].
    pub fn handle(&self) -> &Handle {
        &self.0.handle
    }

    /// Returns the shared [`Shutdown`] receiver future.
    pub fn on_shutdown_signal(&self) -> &Shutdown {
        &self.0.on_shutdown
    }

    /// Takes the internal panic-monitoring task join handle, if not already taken.
    ///
    /// The handle resolves with `Err(PanickedTaskError)` if a critical task panicked,
    /// or `Ok(())` if shutdown was requested.
    ///
    /// # Panics
    ///
    /// Panics if the task manager handle mutex is poisoned.
    pub fn take_task_manager_handle(&self) -> Option<JoinHandle<Result<(), PanickedTaskError>>> {
        self.0.task_manager_handle.lock().unwrap().take()
    }

    /// Spawns a critical task that the runtime will wait for during graceful shutdown. The closure
    /// receives a [`GracefulShutdown`] future and guard; keep the guard alive while doing cleanup,
    /// then drop it to signal completion.
    pub fn spawn_critical_with_graceful_shutdown_signal<F>(
        &self,
        name: &'static str,
        f: impl FnOnce(GracefulShutdown) -> F,
    ) -> JoinHandle<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        record_task_spawn("critical_graceful");
        let panicked_tx = self.0.task_events_tx.clone();
        let on_shutdown = GracefulShutdown::new(
            self.0.on_shutdown.clone(),
            GracefulShutdownGuard::new(Arc::clone(&self.0.graceful_tasks)),
        );
        let fut = f(on_shutdown);

        let task = AssertUnwindSafe(fut)
            .catch_unwind()
            .map_err(move |error| {
                record_task_panic(name);
                let task_error = PanickedTaskError::new(name, error);
                error!("{task_error}");
                let _ = panicked_tx.send(TaskEvent::Panic(task_error));
            })
            .map(drop)
            .in_current_span();

        self.0.handle.spawn(task)
    }

    /// Fires the shutdown signal and blocks until all graceful tasks complete or the
    /// timeout elapses. Returns `true` if all tasks completed before the timeout.
    pub fn graceful_shutdown_with_timeout(&self, timeout: Duration) -> bool {
        let _ = self.0.task_events_tx.send(TaskEvent::GracefulShutdown);
        let deadline = Instant::now().checked_add(timeout);
        loop {
            let active = self.0.graceful_tasks.load(Ordering::SeqCst);
            record_graceful_shutdown_active(active);
            if active == 0 {
                break;
            }
            if deadline.is_some_and(|deadline| Instant::now() > deadline) {
                record_graceful_shutdown_timeout();
                warn!("Graceful shutdown timed out");
                return false;
            }
            thread::yield_now();
        }
        info!("Gracefully shut down");
        true
    }
}
