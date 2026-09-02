//! Layers for tracing subscribers, including stdout, file, and journald layers.

use std::path::{Path, PathBuf};

use kival_common::fs;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::{EnvFilter, Layer, Registry, filter::Directive};

use crate::{file::RollingFileAppender, formatter::LogFormat};

/// A worker guard returned by the file layer.
///
///  When a guard is dropped, all events currently in-memory are flushed to the log file this guard
///  belongs to.
pub type FileWorkerGuard = WorkerGuard;

///  A boxed tracing [Layer].
pub(crate) type BoxedLayer<S> = Box<dyn Layer<S> + Send + Sync>;

/// Default [directives](Directive) for [`EnvFilter`].
const DEFAULT_ENV_FILTER_DIRECTIVES: [&str; 1] = ["hyper::proto::h1=off"];

/// Manages the collection of layers for a tracing subscriber.
///
/// `Layers` acts as a container for different logging layers such as stdout, file, or journald.
/// Each layer can be configured separately and then combined into a tracing subscriber.
#[derive(Default)]
pub struct Layers {
    /// Ordered tracing layers to install into the subscriber.
    inner: Vec<BoxedLayer<Registry>>,
}

impl std::fmt::Debug for Layers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Layers").field("layers_count", &self.inner.len()).finish()
    }
}

impl Layers {
    /// Creates a new `Layers` instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a layer to the collection of layers.
    pub fn add_layer<L>(&mut self, layer: L)
    where
        L: Layer<Registry> + Send + Sync,
    {
        self.inner.push(layer.boxed());
    }

    /// Consumes the `Layers` instance, returning the inner vector of layers.
    pub(crate) fn into_inner(self) -> Vec<BoxedLayer<Registry>> {
        self.inner
    }

    /// Adds a journald layer to the layers collection.
    ///
    /// # Arguments
    /// * `filter` - A string containing additional filter directives for this layer.
    ///
    /// # Returns
    /// An `eyre::Result<()>` indicating the success or failure of the operation.
    pub(crate) fn journald(&mut self, filter: &str) -> eyre::Result<()> {
        let journald_filter = build_env_filter(None, filter)?;
        let layer = tracing_journald::layer()?.with_filter(journald_filter);
        self.add_layer(layer);
        Ok(())
    }

    /// Adds a stdout layer with specified formatting and filtering.
    ///
    /// # Type Parameters
    /// * `S` - The type of subscriber that will use these layers.
    ///
    /// # Arguments
    /// * `format` - The log message format.
    /// * `directive` - Directive for the default logging level.
    /// * `filter` - Additional filter directives as a string.
    /// * `color` - Optional color configuration for the log messages.
    ///
    /// # Returns
    /// An `eyre::Result<()>` indicating the success or failure of the operation.
    pub(crate) fn stdout(
        &mut self,
        format: LogFormat,
        default_directive: Directive,
        filters: &str,
        color: Option<String>,
    ) -> eyre::Result<()> {
        let filter = build_env_filter(Some(default_directive), filters)?;
        let layer = format.apply(filter, color, None);
        self.add_layer(layer);
        Ok(())
    }

    /// Adds a file logging layer to the layers collection.
    ///
    /// # Arguments
    /// * `format` - The format for log messages.
    /// * `filter` - Additional filter directives as a string.
    /// * `file_info` - Information about the log file including path and rotation strategy.
    ///
    /// # Returns
    /// An `eyre::Result<FileWorkerGuard>` representing the file logging worker.
    pub(crate) fn file(
        &mut self,
        format: LogFormat,
        filter: &str,
        file_info: &FileInfo,
    ) -> eyre::Result<FileWorkerGuard> {
        let (writer, guard) = file_info.create_log_writer()?;
        let file_filter = build_env_filter(None, filter)?;
        let layer = format.apply(file_filter, None, Some(writer));
        self.add_layer(layer);
        Ok(guard)
    }
}

/// Holds configuration information for file logging.
///
/// Contains details about the log file's path, name, size, and rotation strategy.
#[derive(Debug, Clone)]
pub struct FileInfo {
    /// Directory where log files are written.
    dir: PathBuf,
    /// Base log file name within `dir`.
    file_name: String,
    /// Maximum active logfile size before rotation.
    max_size_bytes: u64,
    /// Maximum number of rotated log files to retain.
    max_files: usize,
}

impl FileInfo {
    /// Creates a new `FileInfo` instance.
    pub const fn new(
        dir: PathBuf,
        file_name: String,
        max_size_bytes: u64,
        max_files: usize,
    ) -> Self {
        Self { dir, file_name, max_size_bytes, max_files }
    }

    /// Creates the log directory if it doesn't exist.
    fn create_log_dir(&self) -> eyre::Result<&Path> {
        let log_dir: &Path = self.dir.as_ref();
        if !log_dir.exists() {
            fs::create_dir_all(log_dir)
                .map_err(|err| eyre::eyre!("Could not create log directory {log_dir:?}: {err}"))?;
        }
        Ok(log_dir)
    }

    /// Creates a non-blocking writer for the log file.
    fn create_log_writer(&self) -> eyre::Result<(NonBlocking, WorkerGuard)> {
        let log_dir = self.create_log_dir()?;
        let (writer, guard) = tracing_appender::non_blocking(
            RollingFileAppender::new(
                log_dir.join(&self.file_name),
                self.max_size_bytes,
                self.max_files,
            )
            .map_err(|err| eyre::eyre!("Could not initialize file logging: {err}"))?,
        );
        Ok((writer, guard))
    }
}

/// Builds an environment filter for logging.
///
/// The events are filtered by `default_directive`, unless overridden by `RUST_LOG`.
///
/// # Arguments
/// * `default_directive` - An optional `Directive` that sets the default directive.
/// * `directives` - Additional directives as a comma-separated string.
///
/// # Returns
/// An `eyre::Result<EnvFilter>` that can be used to configure a tracing subscriber.
fn build_env_filter(
    default_directive: Option<Directive>,
    directives: &str,
) -> eyre::Result<EnvFilter> {
    let env_filter = default_directive.map_or_else(
        || EnvFilter::builder().from_env_lossy(),
        |default_directive| {
            EnvFilter::builder().with_default_directive(default_directive).from_env_lossy()
        },
    );

    DEFAULT_ENV_FILTER_DIRECTIVES
        .into_iter()
        .chain(directives.split(',').filter(|d| !d.is_empty()))
        .try_fold(env_filter, |env_filter, directive| {
            Ok(env_filter.add_directive(directive.parse()?))
        })
}
