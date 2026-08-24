//! Linux backend: reads `/proc/self/{status,stat}` plus `getrusage` and
//! `getrlimit` for what those files don't expose.
//!
//! Every field is filled as `Option<T>`: a `None` means the syscall or
//! `/proc` read failed and the collector should skip the metric for this
//! scrape rather than reporting a fake zero.

use std::sync::OnceLock;

use kival_common::fs;

use crate::process::Sample;

/// Sample Linux process metrics from `/proc` and resource syscalls.
pub(crate) fn sample() -> Sample {
    let usage = resource_usage();
    let mut s = Sample {
        cpu_seconds_total: usage.as_ref().map(cpu_seconds),
        minor_page_faults: usage.as_ref().and_then(|usage| u64::try_from(usage.ru_minflt).ok()),
        major_page_faults: usage.as_ref().and_then(|usage| u64::try_from(usage.ru_majflt).ok()),
        max_fds: max_fds(),
        open_fds: count_open_fds(),
        // The Prometheus `process_virtual_memory_max_bytes` metric is the
        // address-space *limit*, not the peak observed usage — read
        // `RLIMIT_AS` directly and pass the soft-limit through verbatim
        // (no `RLIM_INFINITY -> 0` mapping).
        virtual_memory_max_bytes: rlimit_cur(libc::RLIMIT_AS),
        start_time_seconds: start_time_seconds(),
        ..Sample::default()
    };
    fill_from_status(&mut s);
    s
}

/// Read process resource usage via `getrusage(RUSAGE_SELF)`.
fn resource_usage() -> Option<libc::rusage> {
    // SAFETY: `libc::rusage` is plain old data with no invalid bit patterns,
    // and `getrusage` writes a fully initialized struct on success. We pass a
    // valid `who` and a valid out-pointer.
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        (libc::getrusage(libc::RUSAGE_SELF, &raw mut usage) == 0).then_some(usage)
    }
}

/// Sum user and system CPU time from a resource-usage snapshot.
fn cpu_seconds(usage: &libc::rusage) -> f64 {
    timeval_to_secs(usage.ru_utime) + timeval_to_secs(usage.ru_stime)
}

/// Convert a `timeval` to fractional seconds.
const fn timeval_to_secs(tv: libc::timeval) -> f64 {
    tv.tv_sec as f64 + (tv.tv_usec as f64) / 1_000_000.0
}

/// Read the process file-descriptor soft limit.
fn max_fds() -> Option<u64> {
    rlimit_cur(libc::RLIMIT_NOFILE)
}

/// Read the soft limit of `resource` via `getrlimit` and return the raw
/// `rlim_cur` value. A syscall failure returns `None` so the collector
/// skips the metric for this scrape; `RLIM_INFINITY` is passed through
/// verbatim so an unlimited rlimit renders as the sentinel value rather
/// than being silently rewritten to `0`.
fn rlimit_cur(resource: libc::__rlimit_resource_t) -> Option<u64> {
    // SAFETY: `getrlimit` writes a fully initialized `rlimit` on success.
    // `lim` is also zero-initialized so reading `rlim_cur` on the failure
    // branch is safe (the result is just discarded by `then_some`).
    unsafe {
        let mut lim: libc::rlimit = std::mem::zeroed();
        let ok = libc::getrlimit(resource, &raw mut lim) == 0;
        ok.then_some(lim.rlim_cur)
    }
}

/// Count currently open process file descriptors.
fn count_open_fds() -> Option<u64> {
    // Each entry in /proc/self/fd corresponds to one open descriptor; `.` /
    // `..` are skipped by `read_dir`. Subtract 1 for the directory FD that
    // `read_dir` itself opens during enumeration — without this we'd
    // consistently overcount by one.
    fs::read_dir("/proc/self/fd").ok().map(|it| it.count().saturating_sub(1) as u64)
}

/// Fill process fields parsed from `/proc/self/status`.
fn fill_from_status(out: &mut Sample) {
    let Ok(text) = fs::read_to_string("/proc/self/status") else { return };
    fill_from_status_text(out, &text);
}

/// Fill memory, thread, and scheduler fields from `/proc/self/status` text.
fn fill_from_status_text(out: &mut Sample, text: &str) {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            out.resident_memory_bytes = parse_kib(rest);
        } else if let Some(rest) = line.strip_prefix("VmSize:") {
            out.virtual_memory_bytes = parse_kib(rest);
        } else if let Some(rest) = line.strip_prefix("Threads:") {
            out.threads = parse_u64(rest);
        } else if let Some(rest) = line.strip_prefix("voluntary_ctxt_switches:") {
            out.voluntary_context_switches = parse_u64(rest);
        } else if let Some(rest) = line.strip_prefix("nonvoluntary_ctxt_switches:") {
            out.nonvoluntary_context_switches = parse_u64(rest);
        }
    }
}

/// Parse a strict unsigned integer field from `/proc/self/status`.
fn parse_u64(rest: &str) -> Option<u64> {
    let mut parts = rest.split_whitespace();
    let value = parts.next()?.parse::<u64>().ok()?;
    parts.next().is_none().then_some(value)
}

/// Parse a `/proc/self/status` line value like `"   1234 kB"` into bytes.
/// Returns `None` if the line does not have the expected `<integer> kB` shape
/// so the caller can skip the corresponding gauge update.
fn parse_kib(rest: &str) -> Option<u64> {
    let mut parts = rest.split_whitespace();

    let kib = parts.next()?.parse::<u64>().ok()?;
    let unit = parts.next()?;

    if unit != "kB" || parts.next().is_some() {
        return None;
    }

    kib.checked_mul(1024)
}

/// Unix-epoch start time of the process. Computed once and cached.
///
/// Derived from `/proc/stat`'s `btime` line (kernel-reported boot time in
/// whole Unix epoch seconds — stable across the process lifetime) plus
/// `/proc/self/stat` field 22 (process start time in clock ticks since
/// boot). The fractional part is preserved so the gauge reflects the
/// kernel's sub-second start-time resolution.
fn start_time_seconds() -> Option<f64> {
    static CACHED: OnceLock<f64> = OnceLock::new();
    if let Some(v) = CACHED.get() {
        return Some(*v);
    }
    let computed = read_start_time()?;
    Some(*CACHED.get_or_init(|| computed))
}

/// Read process start time from `/proc/stat` and `/proc/self/stat`.
fn read_start_time() -> Option<f64> {
    let btime = parse_btime_from_proc_stat(&fs::read_to_string("/proc/stat").ok()?)?;
    let starttime_ticks = parse_self_stat_starttime(&fs::read_to_string("/proc/self/stat").ok()?)?;
    let hz = clock_ticks_per_second();
    Some(btime as f64 + (starttime_ticks as f64 / hz as f64))
}

/// Parse the `btime` line out of a `/proc/stat` snapshot — the boot time
/// of the system in whole seconds since the Unix epoch.
fn parse_btime_from_proc_stat(text: &str) -> Option<u64> {
    text.lines().find_map(|line| line.strip_prefix("btime ")).and_then(|v| v.trim().parse().ok())
}

/// Parse field 22 (`starttime`, in clock ticks since boot) from a
/// `/proc/self/stat` snapshot, splitting after the *last* `)` so that comm
/// values containing spaces or parens don't shift the field indices.
fn parse_self_stat_starttime(stat: &str) -> Option<u64> {
    let (_, after_comm) = stat.rsplit_once(')')?;
    // After comm, field 3 is index 0; field 22 → index 22 - 3 = 19.
    after_comm.split_whitespace().nth(19)?.parse().ok()
}

/// Return kernel clock ticks per second, falling back to Linux's common `100`.
fn clock_ticks_per_second() -> i64 {
    // SAFETY: `sysconf` is async-signal-safe and accepts the standard name.
    let v = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if v <= 0 { 100 } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kib_handles_valid_status_lines() {
        // The leading whitespace and `kB` suffix in `/proc/self/status`
        // values are part of the format we parse — verify both work.
        assert_eq!(parse_kib("   1234 kB"), Some(1234 * 1024));
        assert_eq!(parse_kib("0 kB"), Some(0));
        assert_eq!(parse_kib("\t42  kB"), Some(42 * 1024));
    }

    #[test]
    fn parse_kib_rejects_malformed_input() {
        assert_eq!(parse_kib(""), None);
        assert_eq!(parse_kib("abc"), None);
        assert_eq!(parse_kib("-1 kB"), None);

        assert_eq!(parse_kib("1234"), None);
        assert_eq!(parse_kib("1234 MB"), None);
        assert_eq!(parse_kib("1234 garbage"), None);
        assert_eq!(parse_kib("1234 kB extra"), None);

        // Integer overflow when multiplied by 1024 must not panic or wrap.
        assert_eq!(parse_kib("18446744073709551615 kB"), None);

        assert_eq!(parse_kib("   1234 kB"), Some(1234 * 1024));
    }

    #[test]
    fn fill_from_status_text_extracts_context_switches() {
        let text = "\
VmRSS:\t10 kB\nVmSize:\t20 kB\nThreads:\t3\nvoluntary_ctxt_switches:\t4\nnonvoluntary_ctxt_switches:\t5\n";
        let mut sample = Sample::default();

        fill_from_status_text(&mut sample, text);

        assert_eq!(sample.resident_memory_bytes, Some(10 * 1024));
        assert_eq!(sample.virtual_memory_bytes, Some(20 * 1024));
        assert_eq!(sample.threads, Some(3));
        assert_eq!(sample.voluntary_context_switches, Some(4));
        assert_eq!(sample.nonvoluntary_context_switches, Some(5));
    }

    #[test]
    fn parse_u64_rejects_malformed_status_fields() {
        assert_eq!(parse_u64(""), None);
        assert_eq!(parse_u64("abc"), None);
        assert_eq!(parse_u64("1 extra"), None);
        assert_eq!(parse_u64("-1"), None);
    }

    #[test]
    fn parse_btime_extracts_boot_time_from_proc_stat() {
        let snapshot = "\
cpu  100 0 200 300 0 0 0 0 0 0
cpu0 50 0 100 150 0 0 0 0 0 0
intr 12345
ctxt 67890
btime 1700000000
processes 4242
";
        assert_eq!(parse_btime_from_proc_stat(snapshot), Some(1_700_000_000));
    }

    #[test]
    fn parse_btime_returns_none_when_btime_missing() {
        assert_eq!(parse_btime_from_proc_stat("cpu 0 0 0 0\nctxt 1\n"), None);
    }

    #[test]
    fn parse_btime_returns_none_when_value_garbled() {
        assert_eq!(parse_btime_from_proc_stat("btime not_a_number\n"), None);
    }

    #[test]
    fn parse_self_stat_starttime_basic_case() {
        // Real-shape /proc/self/stat with a simple comm. Field 22
        // (starttime) is `12345`. Fields after comm are indexed 0-based,
        // so we need 19 whitespace-separated tokens before `12345`.
        let stat = "\
1234 (bash) S 1 1234 1234 34816 1234 4194304 100 0 0 0 1 2 0 0 \
20 0 1 0 12345 1234567 567 18446744073709551615 1 1 0 0 0 0 0\n";
        assert_eq!(parse_self_stat_starttime(stat), Some(12345));
    }

    /// Regression: a process whose `comm` contains spaces *and* nested
    /// parentheses must NOT shift the field indices. This is the canonical
    /// /proc/self/stat parsing footgun (see e.g. CVE-class issues across
    /// many language ecosystems where naive `split(' ')` was used).
    #[test]
    fn parse_self_stat_starttime_handles_comm_with_spaces_and_parens() {
        // Comm = "weird (na me)" with a literal space and inner parens.
        // The kernel still wraps the whole thing in `(` ... `)`, so we
        // must `rsplit_once(')')` to find the *last* close-paren.
        let stat = "\
1234 (weird (na me)) S 1 1234 1234 34816 1234 4194304 100 0 0 0 1 2 0 0 \
20 0 1 0 99999 1234567 567 18446744073709551615 1 1 0 0 0 0 0\n";
        assert_eq!(parse_self_stat_starttime(stat), Some(99999));
    }

    #[test]
    fn parse_self_stat_starttime_returns_none_when_close_paren_missing() {
        assert_eq!(parse_self_stat_starttime("1234 broken_comm S 1 1\n"), None);
    }

    #[test]
    fn parse_self_stat_starttime_returns_none_when_too_few_fields() {
        // Fewer than 20 whitespace tokens after the close-paren.
        assert_eq!(parse_self_stat_starttime("1234 (x) S 1 2 3\n"), None);
    }

    #[test]
    fn parse_self_stat_starttime_returns_none_when_starttime_garbled() {
        let stat = "\
1234 (x) S 1 1 1 1 1 1 1 1 1 1 1 1 1 1 \
1 1 1 1 not_a_number rest rest rest\n";
        assert_eq!(parse_self_stat_starttime(stat), None);
    }
}
