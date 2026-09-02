//! Helper for shutdown signals.

use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, ready},
};

use futures_util::{
    FutureExt,
    future::{FusedFuture, Shared},
};
use tokio::sync::oneshot;

/// A future that resolves when the shutdown event has been fired and that holds a
/// [`GracefulShutdownGuard`] which keeps runtime shutdown waiting until the guard is dropped.
#[derive(Debug)]
pub struct GracefulShutdown {
    /// Shared shutdown future watched by this graceful shutdown handle.
    shutdown: Shutdown,
    /// Guard that keeps the task manager waiting until cleanup finishes.
    guard: Option<GracefulShutdownGuard>,
}

impl GracefulShutdown {
    /// Creates a graceful shutdown future with an active completion guard.
    pub(crate) const fn new(shutdown: Shutdown, guard: GracefulShutdownGuard) -> Self {
        Self { shutdown, guard: Some(guard) }
    }

    /// Returns a new shutdown future that drops the returned [`GracefulShutdownGuard`]
    /// immediately on completion.
    pub fn ignore_guard(self) -> impl Future<Output = ()> + Send + Sync + Unpin + 'static {
        self.map(drop)
    }
}

impl Future for GracefulShutdown {
    type Output = GracefulShutdownGuard;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        ready!(self.shutdown.poll_unpin(cx));
        Poll::Ready(self.get_mut().guard.take().expect("Future polled after completion"))
    }
}

impl Clone for GracefulShutdown {
    fn clone(&self) -> Self {
        Self {
            shutdown: self.shutdown.clone(),
            guard: self.guard.as_ref().map(|g| GracefulShutdownGuard::new(Arc::clone(&g.0))),
        }
    }
}

/// A guard that decrements the active-graceful-tasks counter on drop, signalling the
/// runtime that this graceful shutdown task has completed.
#[derive(Debug)]
#[must_use = "if unused the task will not be gracefully shutdown"]
pub struct GracefulShutdownGuard(Arc<AtomicUsize>);

impl GracefulShutdownGuard {
    /// Increments the active graceful-task counter and returns its guard.
    pub(crate) fn new(counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(counter)
    }
}

impl Drop for GracefulShutdownGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A cheaply cloneable future that resolves when the shutdown signal has been fired.
#[derive(Debug, Clone)]
pub struct Shutdown(Shared<oneshot::Receiver<()>>);

impl Future for Shutdown {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let pin = self.get_mut();
        if pin.0.is_terminated() || pin.0.poll_unpin(cx).is_ready() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// Shutdown signal that fires either manually via [`Signal::fire`] or on drop by closing
/// the underlying channel.
#[derive(Debug)]
pub struct Signal(oneshot::Sender<()>);

impl Signal {
    /// Fire the signal manually.
    pub fn fire(self) {
        let _ = self.0.send(());
    }
}

/// Create a [`Signal`]/[`Shutdown`] pair used to propagate a one-shot shutdown event.
pub fn signal() -> (Signal, Shutdown) {
    let (sender, receiver) = oneshot::channel();
    (Signal(sender), Shutdown(receiver.shared()))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures_util::future::join_all;
    use tokio::{task, time::sleep};

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn drop_signal() {
        let (signal, shutdown) = signal();

        task::spawn(async move {
            sleep(Duration::from_millis(50)).await;
            drop(signal)
        });

        shutdown.await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn multi_shutdowns() {
        let (signal, shutdown) = signal();

        let mut tasks = Vec::with_capacity(100);
        for _ in 0..100 {
            let shutdown = shutdown.clone();
            tasks.push(task::spawn(async move {
                shutdown.await;
            }));
        }

        drop(signal);
        for result in join_all(tasks).await {
            result.expect("shutdown waiter task should complete");
        }
    }
}
