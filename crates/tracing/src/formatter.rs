//! Formatting utilities for tracing subscribers.

use std::{fmt, fmt::Display};

use clap::ValueEnum;
use tracing_appender::non_blocking::NonBlocking;
use tracing_subscriber::{EnvFilter, Layer, Registry, fmt::layer};

use crate::layers::BoxedLayer;

/// Represents the logging format.
///
/// This enum defines the supported formats for logging output.
/// It is used to configure the format layer of a tracing subscriber.
#[derive(Debug, Copy, Clone, ValueEnum, Eq, PartialEq)]
pub enum LogFormat {
    /// Represents JSON formatting for logs.
    /// This format outputs log records as JSON objects,
    /// making it suitable for structured logging.
    Json,

    /// Represents terminal-friendly formatting for logs.
    Terminal,
}

impl LogFormat {
    /// Applies the specified logging format to create a new layer.
    ///
    /// This method constructs a tracing layer with the selected format,
    /// along with additional configurations for filtering and output.
    ///
    /// # Arguments
    /// * `filter` - An `EnvFilter` used to determine which log records to output.
    /// * `color` - An optional string that enables or disables ANSI color codes in the logs.
    /// * `file_writer` - An optional `NonBlocking` writer for directing logs to a file.
    ///
    /// # Returns
    /// A `BoxedLayer<Registry>` that can be added to a tracing subscriber.
    pub fn apply(
        &self,
        filter: EnvFilter,
        color: Option<String>,
        file_writer: Option<NonBlocking>,
    ) -> BoxedLayer<Registry> {
        let ansi = color.is_some_and(|color| {
            std::env::var("RUST_LOG_STYLE").map(|val| val != "never").unwrap_or(color != "never")
        });
        let target = std::env::var("RUST_LOG_TARGET")
            // `RUST_LOG_TARGET` always overrides default behaviour
            .map(|val| val != "0")
            .unwrap_or(true);

        match self {
            Self::Json => {
                let layer = layer().json().with_ansi(ansi).with_target(target);

                if let Some(writer) = file_writer {
                    layer.with_writer(writer).with_filter(filter).boxed()
                } else {
                    layer.with_filter(filter).boxed()
                }
            }
            Self::Terminal => {
                let layer = layer().with_ansi(ansi).with_target(target);

                if let Some(writer) = file_writer {
                    layer.with_writer(writer).with_filter(filter).boxed()
                } else {
                    layer.with_filter(filter).boxed()
                }
            }
        }
    }
}

impl Display for LogFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json => write!(f, "json"),
            Self::Terminal => write!(f, "terminal"),
        }
    }
}
