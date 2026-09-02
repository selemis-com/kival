//! Process-level metrics collector for Kival.
//!
//! Emits the standard Prometheus process metric set:
//!
//! * `process_cpu_seconds_total` (counter, seconds — fractional CPU time is truncated to whole
//!   seconds when stored in the u64 counter)
//! * `process_open_fds` (gauge)
//! * `process_max_fds` (gauge)
//! * `process_resident_memory_bytes` (gauge, bytes)
//! * `process_virtual_memory_bytes` (gauge, bytes)
//! * `process_virtual_memory_max_bytes` (gauge, bytes — address-space *limit* from `RLIMIT_AS`;
//!   "Maximum amount of virtual memory available in bytes", not peak observed usage)
//! * `process_threads` (gauge)
//! * `process_start_time_seconds` (gauge, seconds since Unix epoch)
//! * `process_context_switches_total` (counter, labeled by kind)
//! * `process_page_faults_total` (counter, labeled by kind)
//!
//! Linux only. Other targets compile but every sample field is `None`, so `collect()` is a no-op.
//!
//! Typical use: construct a [`Collector`], call [`Collector::describe`] once
//! at startup, and call [`Collector::collect`] periodically (e.g., per scrape).

#[cfg(target_os = "linux")]
mod linux;

use crate::{counter, describe_counter, describe_gauge, gauge};

/// One read of the kernel/runtime process statistics.
///
/// Every field is `Option<T>`: a `None` means the read failed for this
/// scrape and the collector should *skip* the corresponding metric update
/// rather than emit `0` (transient `/proc` or sysctl failures must not
/// appear as drops-to-zero on dashboards).
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Sample {
    /// Total user + system CPU time, in seconds.
    pub(crate) cpu_seconds_total: Option<f64>,
    /// Resident set size, in bytes.
    pub(crate) resident_memory_bytes: Option<u64>,
    /// Virtual memory size, in bytes.
    pub(crate) virtual_memory_bytes: Option<u64>,
    /// Address-space *limit* (i.e. maximum virtual memory the process is
    /// allowed to allocate), in bytes. Read from `RLIMIT_AS` on  Linux only,
    /// this is the value behind the Prometheus `process_virtual_memory_max_bytes`
    /// metric ("Maximum amount of virtual memory available in bytes"), *not* `VmPeak` / peak
    /// observed usage.
    pub(crate) virtual_memory_max_bytes: Option<u64>,
    /// Open file descriptors.
    pub(crate) open_fds: Option<u64>,
    /// Maximum file descriptors (`RLIMIT_NOFILE`).
    pub(crate) max_fds: Option<u64>,
    /// Number of threads in the process.
    pub(crate) threads: Option<u64>,
    /// Process start time in seconds since the Unix epoch.
    pub(crate) start_time_seconds: Option<f64>,
    /// Voluntary context switches.
    pub(crate) voluntary_context_switches: Option<u64>,
    /// Nonvoluntary context switches.
    pub(crate) nonvoluntary_context_switches: Option<u64>,
    /// Minor page faults.
    pub(crate) minor_page_faults: Option<u64>,
    /// Major page faults.
    pub(crate) major_page_faults: Option<u64>,
}

/// Collector that samples the current process's resource usage and emits
/// metrics through the global recorder.
///
/// Cheap to construct; constructing a fresh one per `collect()` is fine.
#[derive(Debug, Default, Clone, Copy)]
pub struct Collector {
    /// Prevent external construction while keeping the type zero-sized.
    _private: (),
}

impl Collector {
    /// Emit `describe_*` calls for every metric this collector produces.
    /// Safe to call repeatedly; intended to be called once at startup.
    pub fn describe(&self) {
        describe_counter!(
            "process_cpu_seconds_total",
            "Total user and system CPU time spent in seconds."
        );
        describe_gauge!("process_open_fds", "Number of open file descriptors.");
        describe_gauge!("process_max_fds", "Maximum number of open file descriptors.");
        describe_gauge!("process_resident_memory_bytes", "Resident memory size in bytes.");
        describe_gauge!("process_virtual_memory_bytes", "Virtual memory size in bytes.");
        describe_gauge!(
            "process_virtual_memory_max_bytes",
            "Maximum amount of virtual memory available in bytes."
        );
        describe_gauge!("process_threads", "Number of OS threads in the process.");
        describe_gauge!(
            "process_start_time_seconds",
            "Start time of the process since unix epoch in seconds."
        );
        describe_counter!("process_context_switches_total", "Process context switches.");
        describe_counter!("process_page_faults_total", "Process page faults.");
    }

    /// Sample the OS and emit gauges + counters into the current recorder.
    ///
    /// Each metric is only emitted when its underlying read succeeded for
    /// this scrape. A transient `/proc` or sysctl failure leaves the
    /// previous gauge value in place rather than dropping the metric to
    /// zero.
    pub fn collect(&self) {
        let s = sample();
        if let Some(v) = s.cpu_seconds_total {
            // Counter `absolute()` semantics: latest value, monotonically
            // increasing. The cast to `u64` truncates fractional CPU
            // seconds.
            counter!("process_cpu_seconds_total").absolute(v as u64);
        }
        if let Some(v) = s.open_fds {
            gauge!("process_open_fds").set(v as f64);
        }
        if let Some(v) = s.max_fds {
            gauge!("process_max_fds").set(v as f64);
        }
        if let Some(v) = s.resident_memory_bytes {
            gauge!("process_resident_memory_bytes").set(v as f64);
        }
        if let Some(v) = s.virtual_memory_bytes {
            gauge!("process_virtual_memory_bytes").set(v as f64);
        }
        if let Some(v) = s.virtual_memory_max_bytes {
            gauge!("process_virtual_memory_max_bytes").set(v as f64);
        }
        if let Some(v) = s.threads {
            gauge!("process_threads").set(v as f64);
        }
        if let Some(v) = s.start_time_seconds {
            gauge!("process_start_time_seconds").set(v);
        }
        if let Some(v) = s.voluntary_context_switches {
            counter!("process_context_switches_total", [("kind", "voluntary")]).absolute(v);
        }
        if let Some(v) = s.nonvoluntary_context_switches {
            counter!("process_context_switches_total", [("kind", "nonvoluntary")]).absolute(v);
        }
        if let Some(v) = s.minor_page_faults {
            counter!("process_page_faults_total", [("kind", "minor")]).absolute(v);
        }
        if let Some(v) = s.major_page_faults {
            counter!("process_page_faults_total", [("kind", "major")]).absolute(v);
        }
    }
}

/// Platform-dispatched process sample.
fn sample() -> Sample {
    #[cfg(target_os = "linux")]
    {
        linux::sample()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Sample::default()
    }
}
