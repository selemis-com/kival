//! Tasks for Kival.

#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod durable;
pub mod runtime;
pub mod shutdown;

pub use durable::DurableTasks;
pub use runtime::{PanickedTaskError, Runtime, RuntimeBuildError};
pub use shutdown::{GracefulShutdown, GracefulShutdownGuard, Shutdown, Signal, signal};
