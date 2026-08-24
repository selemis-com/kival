//! Hooks for collecting metrics.

use std::{
    fmt::{Debug, Formatter, Result},
    sync::Arc,
};

use crate::process::Collector;

/// The simple alias for function types that are `'static`, `Send`, and `Sync`.
pub trait HookTr: Fn() + Send + Sync + 'static {}
impl<T: 'static + Fn() + Send + Sync> HookTr for T {}

/// The type alias for a boxed [`HookTr`] trait object.
pub(crate) type Hook = Box<dyn HookTr<Output = ()>>;

/// A builder-like type to create a new [`Hooks`] instance.
pub struct HooksBuilder {
    /// Hooks executed before each metrics scrape is rendered.
    hooks: Vec<Hook>,
}

impl HooksBuilder {
    /// Registers a new scrape hook implementing [`HookTr`].
    pub fn with_hook(mut self, hook: impl HookTr) -> Self {
        self.hooks.push(Box::new(hook));
        self
    }

    /// Builds the [`Hooks`] collection from the registered hooks.
    pub fn build(self) -> Hooks {
        Hooks { hooks: Arc::new(self.hooks) }
    }
}

impl Default for HooksBuilder {
    fn default() -> Self {
        Self {
            hooks: vec![Box::new(|| Collector::default().collect()), Box::new(collect_io_stats)],
        }
    }
}

impl Debug for HooksBuilder {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.debug_struct("HooksBuilder")
            .field("hooks", &format_args!("Vec<Box<dyn HookTr>>, len: {}", self.hooks.len()))
            .finish()
    }
}

/// Helper type for managing hooks
#[derive(Clone)]
pub struct Hooks {
    /// Shared sync hook list cloned into metrics server scrape closures.
    hooks: Arc<Vec<Box<dyn HookTr<Output = ()>>>>,
}

impl Hooks {
    /// Creates a new [`HooksBuilder`] instance.
    #[inline]
    pub fn builder() -> HooksBuilder {
        HooksBuilder::default()
    }

    /// Runs all registered scrape hooks.
    pub fn run(&self) {
        for hook in self.hooks.iter() {
            hook();
        }
    }
}

impl Debug for Hooks {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let hooks_len = self.hooks.len();
        f.debug_struct("Hooks")
            .field("hooks", &format_args!("Arc<Vec<Box<dyn HookTr>>>, len: {hooks_len}"))
            .finish()
    }
}

#[cfg(target_os = "linux")]
/// Collect process IO statistics from `/proc/self/io` into counters.
fn collect_io_stats() {
    use kival_common::fs;
    use kival_tracing::error;

    use crate::counter;

    let Ok(text) = fs::read_to_string("/proc/self/io")
        .map_err(|error| error!(%error, "Failed to read IO stats for the current process"))
    else {
        return;
    };

    let Ok(io) = ProcessIo::parse(&text)
        .map_err(|error| error!(%error, "Failed to parse IO stats for the current process"))
    else {
        return;
    };

    counter!("io.rchar").absolute(io.rchar);
    counter!("io.wchar").absolute(io.wchar);
    counter!("io.syscr").absolute(io.syscr);
    counter!("io.syscw").absolute(io.syscw);
    counter!("io.read_bytes").absolute(io.read_bytes);
    counter!("io.write_bytes").absolute(io.write_bytes);
    counter!("io.cancelled_write_bytes").absolute(io.cancelled_write_bytes);
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Snapshot of fields exposed by `/proc/self/io`.
struct ProcessIo {
    /// Characters read.
    rchar: u64,
    /// Characters written.
    wchar: u64,
    /// Read syscall count.
    syscr: u64,
    /// Write syscall count.
    syscw: u64,
    /// Bytes fetched from storage.
    read_bytes: u64,
    /// Bytes sent to storage.
    write_bytes: u64,
    /// Bytes whose writeback was cancelled.
    cancelled_write_bytes: u64,
}

#[cfg(target_os = "linux")]
impl ProcessIo {
    /// Parse the Linux `/proc/<pid>/io` key-value format.
    fn parse(text: &str) -> std::result::Result<Self, &'static str> {
        Ok(Self {
            rchar: parse_io_field(text, "rchar")?,
            wchar: parse_io_field(text, "wchar")?,
            syscr: parse_io_field(text, "syscr")?,
            syscw: parse_io_field(text, "syscw")?,
            read_bytes: parse_io_field(text, "read_bytes")?,
            write_bytes: parse_io_field(text, "write_bytes")?,
            cancelled_write_bytes: parse_io_field(text, "cancelled_write_bytes")?,
        })
    }
}

#[cfg(target_os = "linux")]
/// Parse one required numeric field from `/proc/<pid>/io`.
fn parse_io_field(text: &str, name: &'static str) -> std::result::Result<u64, &'static str> {
    let prefix = format!("{name}:");
    let value = text.lines().find_map(|line| line.strip_prefix(&prefix)).ok_or(name)?.trim();

    if value.split_whitespace().count() != 1 {
        return Err(name);
    }

    value.parse().map_err(|_| name)
}

#[cfg(not(target_os = "linux"))]
/// Collect process IO statistics from `/proc/self/io` into counters (disabled).
const fn collect_io_stats() {}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    //! Tests for the local `/proc/self/io` parser.

    use super::*;

    /// A complete kernel-shaped snapshot parses all IO counters.
    #[test]
    fn parse_process_io_snapshot() {
        let text = "\
rchar: 1\nwchar: 2\nsyscr: 3\nsyscw: 4\nread_bytes: 5\nwrite_bytes: 6\ncancelled_write_bytes: 7\n";

        assert_eq!(
            ProcessIo::parse(text),
            Ok(ProcessIo {
                rchar: 1,
                wchar: 2,
                syscr: 3,
                syscw: 4,
                read_bytes: 5,
                write_bytes: 6,
                cancelled_write_bytes: 7,
            })
        );
    }

    /// Missing required fields reject the whole snapshot.
    #[test]
    fn parse_process_io_rejects_missing_fields() {
        let text = "rchar: 1\nwchar: 2\nsyscr: 3\nsyscw: 4\nread_bytes: 5\nwrite_bytes: 6\n";

        assert_eq!(ProcessIo::parse(text), Err("cancelled_write_bytes"));
    }

    /// Malformed field values reject the whole snapshot.
    #[test]
    fn parse_process_io_rejects_malformed_values() {
        let text = "\
rchar: 1\nwchar: two\nsyscr: 3\nsyscw: 4\nread_bytes: 5\nwrite_bytes: 6\ncancelled_write_bytes: 7\n";

        assert_eq!(ProcessIo::parse(text), Err("wchar"));
    }
}
