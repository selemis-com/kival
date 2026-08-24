//! Logging configuration and initialization for the CLI.

use std::{fmt, fmt::Display, sync::OnceLock};

use clap::{ArgAction, Args, ValueEnum};
use kival_tracing::{
    FileInfo, FileWorkerGuard, KivalTracer, LayerInfo, Level, LogFormat, Tracer,
    level_filters::LevelFilter, tracing_subscriber::filter::Directive,
};

use crate::dirs::{LogsDir, PlatformPath};

/// Constant to convert megabytes to bytes
const MB_TO_BYTES: u64 = 1024 * 1024;

/// Lazily initialized log defaults
static LOG_DEFAULTS: OnceLock<DefaultLogArgs> = OnceLock::new();

/// The log configuration.
#[derive(Debug, Args)]
#[command(next_help_heading = "Logging")]
pub struct LogArgs {
    /// The format to use for logs written to stdout.
    #[arg(long = "log.stdout.format", value_name = "FORMAT", global = true, default_value_t = DefaultLogArgs::get_global().log_stdout_format)]
    pub log_stdout_format: LogFormat,

    /// The filter to use for logs written to stdout.
    #[arg(long = "log.stdout.filter", value_name = "FILTER", global = true, default_value_t = DefaultLogArgs::get_global().log_stdout_filter.clone())]
    pub log_stdout_filter: String,

    /// The format to use for logs written to the log file.
    #[arg(long = "log.file.format", value_name = "FORMAT", global = true, default_value_t = DefaultLogArgs::get_global().log_file_format)]
    pub log_file_format: LogFormat,

    /// The filter to use for logs written to the log file.
    #[arg(long = "log.file.filter", value_name = "FILTER", global = true, default_value_t = DefaultLogArgs::get_global().log_file_filter.clone())]
    pub log_file_filter: String,

    /// The path to put log files in.
    #[arg(long = "log.file.directory", value_name = "PATH", global = true, default_value_t)]
    pub log_file_directory: PlatformPath<LogsDir>,

    /// The prefix name of the log files.
    #[arg(long = "log.file.name", value_name = "NAME", global = true, default_value_t = DefaultLogArgs::get_global().log_file_name.clone())]
    pub log_file_name: String,

    /// The maximum size (in MB) of one log file.
    #[arg(long = "log.file.max-size", value_name = "SIZE", global = true, default_value_t = DefaultLogArgs::get_global().log_file_max_size)]
    pub log_file_max_size: u64,

    /// The maximum amount of log files that will be stored. If set to 0, background file logging
    /// is disabled.
    #[arg(long = "log.file.max-files", value_name = "COUNT", global = true, default_value_t = 5)]
    pub log_file_max_files: usize,

    /// Write logs to journald.
    #[arg(long = "log.journald", global = true, default_value_t = DefaultLogArgs::get_global().journald)]
    pub journald: bool,

    /// The filter to use for logs written to journald.
    #[arg(
        long = "log.journald.filter",
        value_name = "FILTER",
        global = true,
        default_value_t = DefaultLogArgs::get_global().journald_filter.clone()
    )]
    pub journald_filter: String,

    /// Sets whether or not the formatter emits ANSI terminal escape codes for colors and other
    /// text formatting.
    #[arg(
        long,
        value_name = "COLOR",
        global = true,
        default_value_t = DefaultLogArgs::get_global().color
    )]
    pub color: ColorMode,

    /// The verbosity settings for the tracer.
    #[command(flatten)]
    pub verbosity: Verbosity,
}

impl LogArgs {
    /// Creates a [`LayerInfo`] instance.
    fn layer_info(&self, format: LogFormat, filter: String, use_color: bool) -> LayerInfo {
        LayerInfo::new(
            format,
            self.verbosity.directive().to_string(),
            filter,
            use_color.then(|| self.color.to_string()),
        )
    }

    /// File info from the current log options.
    fn file_info(&self) -> FileInfo {
        FileInfo::new(
            self.log_file_directory.clone().into(),
            self.log_file_name.clone(),
            self.log_file_max_size * MB_TO_BYTES,
            self.log_file_max_files,
        )
    }

    /// Initializes tracing with the configured options from CLI args.
    ///
    /// Returns the file worker guard if a file worker was configured.
    ///
    /// # Errors
    ///
    /// Returns an error if tracing cannot be initialized from the configured options.
    pub fn init_tracing(&self) -> eyre::Result<Option<FileWorkerGuard>> {
        let mut tracer = KivalTracer::new();

        let stdout = self.layer_info(self.log_stdout_format, self.log_stdout_filter.clone(), true);
        tracer = tracer.with_stdout(stdout);

        if self.journald {
            tracer = tracer.with_journald(self.journald_filter.clone());
        }

        if self.log_file_max_files > 0 {
            let info = self.file_info();
            let file = self.layer_info(self.log_file_format, self.log_file_filter.clone(), false);
            tracer = tracer.with_file(file, info);
        }

        tracer.init()
    }
}

/// Default values for log configuration.
#[derive(Debug, Clone)]
struct DefaultLogArgs {
    /// Default stdout log format.
    log_stdout_format: LogFormat,
    /// Default stdout filter directives.
    log_stdout_filter: String,
    /// Default file log format.
    log_file_format: LogFormat,
    /// Default file log filter directives.
    log_file_filter: String,
    /// Default file log name.
    log_file_name: String,
    /// Default maximum file log size in megabytes.
    log_file_max_size: u64,
    /// Whether journald logging is enabled by default.
    journald: bool,
    /// Default journald filter directives.
    journald_filter: String,
    /// Default terminal color mode.
    color: ColorMode,
}

impl DefaultLogArgs {
    /// Get a reference to the log defaults.
    fn get_global() -> &'static Self {
        LOG_DEFAULTS.get_or_init(Self::default)
    }
}

impl Default for DefaultLogArgs {
    fn default() -> Self {
        Self {
            log_stdout_format: LogFormat::Terminal,
            log_stdout_filter: String::new(),
            log_file_format: LogFormat::Terminal,
            log_file_filter: "debug".to_owned(),
            log_file_name: "kival.log".to_owned(),
            log_file_max_size: 200,
            journald: false,
            journald_filter: "error".to_owned(),
            color: ColorMode::Always,
        }
    }
}

/// The color mode for the CLI.
#[derive(Debug, Copy, Clone, ValueEnum, Eq, PartialEq)]
pub enum ColorMode {
    /// Colors on
    Always,
    /// Auto-detect
    Auto,
    /// Colors off
    Never,
}

impl Display for ColorMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Always => write!(f, "always"),
            Self::Auto => write!(f, "auto"),
            Self::Never => write!(f, "never"),
        }
    }
}

/// The verbosity settings for the CLI.
#[derive(Debug, Copy, Clone, Args)]
#[command(next_help_heading = "Display")]
pub struct Verbosity {
    /// Set the minimum log level.
    ///
    /// -v      Errors
    /// -vv     Warnings
    /// -vvv    Info
    /// -vvvv   Debug
    /// -vvvvv  Traces (warning: very verbose!)
    #[arg(short, long, action = ArgAction::Count, global = true, default_value_t = 3, verbatim_doc_comment, help_heading = "Display")]
    verbosity: u8,

    /// Silence all log output.
    #[arg(long, alias = "silent", short = 'q', global = true, help_heading = "Display")]
    quiet: bool,
}

impl Verbosity {
    /// Whether all terminal output controlled by the verbosity settings should be suppressed.
    #[must_use]
    pub const fn is_quiet(&self) -> bool {
        self.quiet
    }

    /// Get the corresponding [Directive] for the given verbosity, or none if the verbosity
    /// corresponds to silent.
    pub fn directive(&self) -> Directive {
        if self.quiet {
            LevelFilter::OFF.into()
        } else {
            let level = match self.verbosity - 1 {
                0 => Level::ERROR,
                1 => Level::WARN,
                2 => Level::INFO,
                3 => Level::DEBUG,
                _ => Level::TRACE,
            };

            level.into()
        }
    }
}
