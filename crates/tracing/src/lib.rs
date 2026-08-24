//!  The `tracing` module provides functionalities for setting up and configuring logging.
//!
//!  It includes structures and functions to create and manage various logging layers: stdout,
//!  file, or journald. The module's primary entry point is the `Tracer` struct, which can be
//!  configured to use different logging formats and destinations. If no layer is specified, it will
//!  default to stdout.

#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub use formatter::LogFormat;
pub use layers::{FileInfo, FileWorkerGuard, Layers};
use tracing::level_filters::LevelFilter;
pub use tracing::*;
pub use tracing_appender;
use tracing_appender::non_blocking::WorkerGuard;
pub use tracing_subscriber;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod file;
mod formatter;
mod layers;

///  Tracer for application logging.
///
///  Manages the configuration and initialization of logging layers,
/// including standard output, optional journald, and optional file logging.
#[derive(Debug, Clone)]
pub struct KivalTracer {
    /// Stdout logging layer configuration.
    stdout: LayerInfo,
    /// Optional journald filter configuration.
    journald: Option<String>,
    /// Optional file logging layer configuration and rotation settings.
    file: Option<(LayerInfo, FileInfo)>,
}

impl KivalTracer {
    /// Constructs a new `Tracer` with default settings.
    ///
    /// Initializes with default stdout layer configuration.
    /// Journald and file layers are not set by default.
    pub fn new() -> Self {
        Self { stdout: LayerInfo::default(), journald: None, file: None }
    }

    /// Sets a custom configuration for the stdout layer.
    ///
    /// # Arguments
    /// * `config` - The `LayerInfo` to use for the stdout layer.
    pub fn with_stdout(mut self, config: LayerInfo) -> Self {
        self.stdout = config;
        self
    }

    /// Sets the journald layer filter.
    ///
    /// # Arguments
    /// * `filter` - The `filter` to use for the journald layer.
    pub fn with_journald(mut self, filter: String) -> Self {
        self.journald = Some(filter);
        self
    }

    /// Sets the file layer configuration and associated file info.
    ///
    ///  # Arguments
    /// * `config` - The `LayerInfo` to use for the file layer.
    /// * `file_info` - The `FileInfo` containing details about the log file.
    pub fn with_file(mut self, config: LayerInfo, file_info: FileInfo) -> Self {
        self.file = Some((config, file_info));
        self
    }
}

impl Default for KivalTracer {
    fn default() -> Self {
        Self::new()
    }
}

///  Configuration for a logging layer.
///
///  This struct holds configuration parameters for a tracing layer, including
///  the format, filtering directives, optional coloring, and directive.
#[derive(Debug, Clone)]
pub struct LayerInfo {
    /// Output format for this layer.
    format: LogFormat,
    /// Default tracing filter directive for this layer.
    default_directive: String,
    /// Additional comma-separated tracing filter directives.
    filters: String,
    /// Optional terminal color mode for this layer.
    color: Option<String>,
}

impl LayerInfo {
    ///  Constructs a new `LayerInfo`.
    ///
    ///  # Arguments
    ///  * `format` - Specifies the format for log messages. Possible values are:
    ///      - `LogFormat::Json` for JSON formatting.
    ///      - `LogFormat::Terminal` for human-readable, terminal-friendly formatting.
    ///  * `default_directive` - Directive for filtering log messages.
    ///  * `filters` - Additional filtering parameters as a string.
    ///  * `color` - Optional color configuration for the log messages.
    pub const fn new(
        format: LogFormat,
        default_directive: String,
        filters: String,
        color: Option<String>,
    ) -> Self {
        Self { format, default_directive, filters, color }
    }
}

impl Default for LayerInfo {
    ///  Provides default values for `LayerInfo`.
    ///
    ///  By default, it uses terminal format, INFO level filter,
    ///  no additional filters, and no color configuration.
    fn default() -> Self {
        Self {
            format: LogFormat::Terminal,
            default_directive: LevelFilter::INFO.to_string(),
            filters: String::new(),
            color: Some("always".to_owned()),
        }
    }
}

/// Trait defining a general interface for logging configuration.
///
/// The `Tracer` trait provides a standardized way to initialize logging configurations
/// in an application. Implementations of this trait can specify different logging setups,
/// such as standard output logging, file logging, journald logging, or custom logging
/// configurations tailored for specific environments (like testing).
pub trait Tracer: Sized {
    /// Initialize the logging configuration.
    ///
    /// # Returns
    /// An `eyre::Result` which is `Ok` with an optional `WorkerGuard` if a file layer is used.
    ///
    /// # Errors
    ///
    /// Returns an error if logging initialization fails.
    fn init(self) -> eyre::Result<Option<WorkerGuard>>;
}

impl Tracer for KivalTracer {
    /// Initializes the logging system based on the configured layers.
    ///
    /// This method sets up the global tracing subscriber with the specified
    /// stdout, journald, and file layers.
    ///
    /// The default layer is stdout.
    ///
    /// # Returns
    /// An `eyre::Result` which is `Ok` with an optional `WorkerGuard` if a file layer is used,
    /// or an `Err` in case of an error during initialization.
    fn init(self) -> eyre::Result<Option<WorkerGuard>> {
        let mut layers = Layers::new();

        layers.stdout(
            self.stdout.format,
            self.stdout.default_directive.parse()?,
            &self.stdout.filters,
            self.stdout.color,
        )?;

        if let Some(config) = self.journald {
            layers.journald(&config)?;
        }

        let file_guard = if let Some((config, file_info)) = self.file {
            Some(layers.file(config.format, &config.filters, &file_info)?)
        } else {
            None
        };

        // The error is returned if the global default subscriber is already set,
        // so it's safe to ignore it
        let _ = tracing_subscriber::registry().with(layers.into_inner()).try_init();

        Ok(file_guard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracer_default() {
        let tracer = KivalTracer::default();
        assert_eq!(tracer.stdout.format, LogFormat::Terminal);
        assert_eq!(tracer.stdout.default_directive, "info");
        assert_eq!(tracer.stdout.filters, "");
        assert_eq!(tracer.stdout.color, Some("always".to_owned()));
        assert_eq!(tracer.journald, None);
    }
}
