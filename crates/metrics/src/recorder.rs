//! Prometheus metrics recorder.

use std::{
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use eyre::{Result, WrapErr};
use tokio::time::sleep;

use crate::{
    backend::{PrefixLayer, Stack},
    prometheus::{PrometheusBuilder, PrometheusHandle},
};

/// The global Prometheus recorder handle.
/// This is set exactly once, typically at application startup.
static PROMETHEUS_RECORDER_HANDLE: OnceLock<PrometheusRecorder> = OnceLock::new();

/// Installs the Prometheus recorder as the global recorder with the given prefix.
///
/// Note: This must be installed before any metrics are `described`.
/// Returns an error if called more than once or if installation fails.
///
/// Caution: This only installs the recorder; you must call [`PrometheusRecorder::spawn_upkeep`]
/// manually.
///
/// # Errors
///
/// Returns an error when the global metrics recorder has already been installed
/// or when the process-wide Prometheus recorder handle cannot be initialized.
pub(crate) fn install_prometheus_recorder(prefix: &str) -> Result<&'static PrometheusRecorder> {
    if let Some(recorder) = PROMETHEUS_RECORDER_HANDLE.get() {
        return Ok(recorder);
    }

    let recorder = PrometheusRecorder::install(prefix)?;
    PROMETHEUS_RECORDER_HANDLE
        .set(recorder)
        .map_err(|_| eyre::eyre!("Prometheus recorder already initialized"))?;

    PROMETHEUS_RECORDER_HANDLE
        .get()
        .ok_or_else(|| eyre::eyre!("Prometheus recorder handle was not initialized"))
}

/// A handle to the Prometheus recorder.
///
/// This is intended to be used as the global recorder.
/// Callers must ensure that [`PrometheusRecorder::spawn_upkeep`] is called once.
#[derive(Debug)]
pub(crate) struct PrometheusRecorder {
    /// Handle used to render Prometheus text exposition and drive upkeep.
    handle: PrometheusHandle,
    /// Whether the background upkeep task has already been spawned.
    upkeep: AtomicBool,
}

impl PrometheusRecorder {
    /// Create a recorder wrapper from a Prometheus handle.
    const fn new(handle: PrometheusHandle) -> Self {
        Self { handle, upkeep: AtomicBool::new(false) }
    }

    /// Returns a reference to the [`PrometheusHandle`].
    pub(crate) const fn handle(&self) -> &PrometheusHandle {
        &self.handle
    }

    /// Spawns the upkeep task if there hasn't been one spawned already.
    ///
    /// See also [`PrometheusHandle::run_upkeep`]
    pub(crate) fn spawn_upkeep(&self) {
        if self.upkeep.compare_exchange(false, true, Ordering::SeqCst, Ordering::Acquire).is_err() {
            return;
        }

        let handle = self.handle.clone();
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(5)).await;
                handle.run_upkeep();
            }
        });
    }

    /// Installs Prometheus as the metrics recorder.
    ///
    /// Caution: This only configures the global recorder and does not spawn the exporter.
    /// Callers must run [`Self::spawn_upkeep`] manually.
    ///
    /// # Errors
    ///
    /// Returns an error when the global recorder has already been installed.
    pub(crate) fn install(prefix: &str) -> Result<Self> {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();

        // Build metrics stack
        Stack::new(recorder)
            .push(&PrefixLayer::new(prefix))
            .install()
            .wrap_err("Couldn't set metrics recorder.")?;

        Ok(Self::new(handle))
    }
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use super::*;
    use crate::process::Collector;

    /// Test that process metrics are collected correctly.
    #[test]
    fn process_metrics() {
        // Install the recorder with a test prefix (idempotent)
        let recorder = install_prometheus_recorder("test").expect("should install recorder");

        let process = Collector::default();
        process.describe();
        process.collect();

        let metrics = recorder.handle.render();
        assert!(metrics.contains("process_cpu_seconds_total"), "{metrics:?}");
    }
}
