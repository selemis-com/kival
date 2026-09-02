//! Metrics gathering for Kival.
//!
//! The crate exposes Kival's Prometheus exporter, scrape hooks, version labels,
//! and lightweight recording macros. Normal callsites use the macros directly
//! and stay independent from the installed backend:
//!
//! ```
//! use kival_metrics::{counter, describe_counter};
//!
//! describe_counter!("network.connected_peers", "Number of connected peers.");
//! counter!("network.connected_peers").increment(1);
//! counter!("requests", "method" => "GET").increment(1);
//! ```
//!
//! Metric names are stored with dot-separated Kival scopes and sanitized by the
//! Prometheus renderer (`network.connected_peers` renders as
//! `network_connected_peers`). Labels are canonicalized for identity, so label
//! order does not change which metric instance is registered.
//!
//! Public surface:
//!
//! * Exporter startup: [`start_metrics_server`].
//! * Scrape hooks: [`Hooks`] and [`HooksBuilder`].
//! * Build labels: [`VersionInfo`].
//! * Recording macros: [`counter!`], [`gauge!`], [`histogram!`], and their [`describe_counter!`] /
//!   [`describe_gauge!`] / [`describe_histogram!`] counterparts.
//!
//! Recorder internals remain available for macro expansion and tests, but they
//! are not part of the normal application-facing API.

mod backend;
mod handles;
mod hooks;
mod key;
mod label;
mod recorder;
mod server;
mod version;

#[doc(hidden)]
pub mod process;
#[doc(hidden)]
pub mod prometheus;

#[doc(hidden)]
pub use backend::{
    Layer, LocalRecorderGuard, NoopRecorder, Prefix, PrefixLayer, Recorder, SetRecorderError,
    Stack, set_default_local_recorder, set_global_recorder, with_recorder,
};
#[doc(hidden)]
pub use handles::{AtomicU64, Counter, CounterFn, Gauge, GaugeFn, Histogram, HistogramFn};
pub use hooks::{HookTr, Hooks, HooksBuilder};
#[doc(hidden)]
pub use key::{Key, KeyName};
#[doc(hidden)]
pub use label::{IntoLabels, Label};
pub use server::start_metrics_server;
pub use version::VersionInfo;
